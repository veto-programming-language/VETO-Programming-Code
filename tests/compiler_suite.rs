use vetoc::compiler::{
    compile_project_manifest, compile_source, compile_test_runner, compile_to_asm,
    compile_to_asm_arch,
};
use vetoc::error::Stage;

fn assert_ok(src: &str) -> String {
    match compile_source(src, "test.vt", ".", false) {
        Ok(c) => c,
        Err(errs) => panic!("Expected successful compilation, got errors:\n{:?}", errs),
    }
}

fn assert_err_stage(src: &str, expected: Stage) {
    match compile_source(src, "test.vt", ".", false) {
        Ok(_) => panic!(
            "Expected error at stage {:?}, but compilation succeeded!",
            expected
        ),
        Err(errs) => assert!(
            errs.iter().any(|e| e.stage == expected),
            "Expected stage {:?}, got: {:?}",
            expected,
            errs
        ),
    }
}

#[test]
fn test_ok_basic_ranges_and_flow() {
    assert_ok(
        r#"
        fn clamp_add(a: i32<0..100>, b: i32<0..50>) -> i32<0..150> {
            let sum: i32<0..150> = a + b;
            prove(sum <= 150);
            return sum;
        }
        fn main() -> i32<0..150> {
            return clamp_add(40, 25);
        }
    "#,
    );
}

#[test]
fn test_err_range_overflow() {
    assert_err_stage(
        r#"
        fn main() -> i32<0..100> {
            let a: i32<0..80> = 70;
            let b: i32<0..50> = 40;
            let c: i32<0..100> = a + b;
            return c;
        }
    "#,
        Stage::Verifier,
    );
}

#[test]
fn test_err_div_by_zero() {
    assert_err_stage(
        r#"
        fn main() -> i32 {
            let d: i32<0..10> = 5;
            return 100 / d;
        }
    "#,
        Stage::Verifier,
    );
}

#[test]
fn test_err_array_out_of_bounds() {
    assert_err_stage(
        r#"
        fn main() -> i32 {
            let arr: [i32<0..10>; 4] = [0; 4];
            let idx: i32<0..4> = 4;
            return arr[idx];
        }
    "#,
        Stage::Verifier,
    );
}

#[test]
fn test_err_mut_ref_aliasing() {
    assert_err_stage(
        r#"
        struct Pair { a: i32<0..10>, b: i32<0..10> }
        fn swap_bad(p1: &mut Pair, p2: &mut Pair) -> i32<0..0> { return 0; }
        fn main() -> i32<0..0> {
            let mut p: Pair = Pair { a: 1, b: 2 };
            return swap_bad(&mut p, &mut p);
        }
    "#,
        Stage::TypeCheck,
    );
}

#[test]
fn test_err_non_exhaustive_enum_match() {
    assert_err_stage(
        r#"
        enum Mode { Idle, Active, Error }
        fn check(m: Mode) -> i32<0..1> {
            match m {
                Mode::Idle => return 0,
                Mode::Active => return 1,
            }
            return 0;
        }
        fn main() -> i32<0..1> { return check(Mode::Idle); }
    "#,
        Stage::Verifier,
    );
}

#[test]
fn test_ok_algebraic_enum_payload_and_match_binding() {
    assert_ok(
        r#"
        enum IoResult { Ok(u8<0..200>), Err(u8<1..10>) }
        fn unwrap_or_zero(r: IoResult) -> i32<0..200> {
            match r {
                IoResult::Ok(val) => return val,
                IoResult::Err(_) => return 0,
            }
            return 0;
        }
        fn main() -> i32<0..200> {
            return unwrap_or_zero(IoResult::Ok(150));
        }
    "#,
    );
}

#[test]
fn test_ok_impl_methods_and_mut_self() {
    assert_ok(
        r#"
        struct Cursor { row: i32<0..24>, col: i32<0..79> }
        impl Cursor {
            fn move_to(&mut self, r: i32<0..24>, c: i32<0..79>) -> () {
                self.row = r;
                self.col = c;
                return;
            }
            fn get_col(&self) -> i32<0..79> { return self.col; }
        }
        fn main() -> i32<0..79> {
            let mut cur: Cursor = Cursor { row: 0, col: 0 };
            cur.move_to(10, 42);
            return cur.get_col();
        }
    "#,
    );
}

#[test]
fn test_ok_verified_mmio_and_port_driver_without_unsafe() {
    assert_ok(
        r#"
        #[mmio(0xB8000..0xB8FA0)]
        #[port(0x3D4..0x3D5)]
        fn write_vga_cell(offset: i32<0..1999>, ch: u8) -> () {
            let addr: i32<753664..757662> = 0xB8000 + offset * 2;
            mem_write8(addr, ch);
            port_out8(0x3D4, 15);
            return;
        }
        #[mmio(0xB8000..0xB8FA0)]
        #[port(0x3D4..0x3D5)]
        fn main() -> i32<0..0> {
            write_vga_cell(160, 'V');
            return 0;
        }
    "#,
    );
}

#[test]
fn test_err_transitive_capability_violation() {
    assert_err_stage(
        r#"
        #[port(0x60..0x64)]
        fn reset_kbd() -> () { port_out8(0x64, 0xFE); return; }
        fn innocent_helper() -> () { reset_kbd(); return; }
        fn main() -> i32<0..0> { innocent_helper(); return 0; }
    "#,
        Stage::Verifier,
    );
}

#[test]
fn test_ok_layer1_bool_unit_and_full_integer_types() {
    assert_ok(
        r#"
        struct Flags { ready: bool, small: i8, big: u32, ptr_sz: usize }
        fn check_flag(f: bool, offset: u16) -> () {
            if f && offset < 1000 { return; }
            return;
        }
        fn main() -> u8 {
            let fl: Flags = Flags { ready: true, small: -42, big: 3000000000, ptr_sz: 4096 };
            check_flag(fl.ready, 512);
            if fl.ready != false { return 1; }
            return 0;
        }
    "#,
    );
}

#[test]
fn test_ok_layer1_relational_var_bounds_in_while() {
    assert_ok(
        r#"
        fn last_byte(buf: &[u8; 16], len: i32<0..16>) -> i32 {
            let mut i: i32<0..64> = 0;
            let mut total: i32<0..20000> = 0;
            while i < len {
                total = buf[i];
                i += 1;
            }
            return total;
        }
        fn main() -> i32 {
            let data: [u8; 16] = [1; 16];
            return last_byte(&data, 8);
        }
    "#,
    );
}

#[test]
fn test_ok_layer1_terminates_and_max_cycles() {
    assert_ok(
        r#"
        #[terminates]
        #[max_cycles(64)]
        fn bounded_poll() -> i32<0..64> {
            let mut count: i32<0..64> = 0;
            while count < 32 { count += 1; }
            return count;
        }
        fn main() -> i32<0..64> { return bounded_poll(); }
    "#,
    );
}

#[test]
fn test_err_layer1_infinite_loop_in_terminates_fn() {
    assert_err_stage(
        r#"
        #[terminates]
        fn hang_driver() -> () { loop { cpu_halt(); } return; }
        fn main() -> i32<0..0> { hang_driver(); return 0; }
    "#,
        Stage::Verifier,
    );
}

#[test]
fn test_ok_layer1_linear_resource_consumed_safely() {
    assert_ok(
        r#"
        #[linear]
        struct SpinLockGuard { lock_id: u8 }
        fn acquire_lock(id: u8) -> SpinLockGuard { return SpinLockGuard { lock_id: id }; }
        fn release_lock(g: SpinLockGuard) -> () { return; }
        fn main() -> i32<0..0> {
            let guard: SpinLockGuard = acquire_lock(1);
            release_lock(guard);
            return 0;
        }
    "#,
    );
}

#[test]
fn test_err_layer1_linear_resource_leak_blocked() {
    assert_err_stage(
        r#"
        #[linear]
        struct SpinLockGuard { lock_id: u8 }
        fn acquire_lock(id: u8) -> SpinLockGuard { return SpinLockGuard { lock_id: id }; }
        fn main() -> i32<0..0> {
            let forgotten: SpinLockGuard = acquire_lock(1);
            return 0;
        }
    "#,
        Stage::Verifier,
    );
}

#[test]
fn test_ok_layer2_typestate_machine_init_order() {
    assert_ok(
        r#"
        #[ensures(GdtReady)]
        fn init_gdt() -> () { return; }
        #[requires(GdtReady)]
        #[ensures(PagingOn)]
        fn enable_paging() -> () { return; }
        #[requires(PagingOn)]
        fn map_kernel_heap() -> () { return; }
        fn main() -> i32<0..0> { init_gdt(); enable_paging(); map_kernel_heap(); return 0; }
    "#,
    );
}

#[test]
fn test_err_layer2_typestate_wrong_hw_init_order() {
    assert_err_stage(
        r#"
        #[ensures(GdtReady)]
        fn init_gdt() -> () { return; }
        #[requires(GdtReady)]
        fn enable_paging() -> () { return; }
        fn main() -> i32<0..0> { enable_paging(); init_gdt(); return 0; }
    "#,
        Stage::Verifier,
    );
}

#[test]
fn test_err_layer2_kernel_stack_overflow_blocked() {
    let src = r#"
        fn huge_frame() -> i32<0..0> {
            let buf: [i32; 5000] = [0; 5000];
            return 0;
        }
        fn main() -> i32<0..0> { return huge_frame(); }
    "#;
    let errs = compile_source(src, "stack_ovf.vt", ".", true)
        .expect_err("20KB stack frame must be blocked by kernel stack analyzer");
    assert!(errs
        .iter()
        .any(|e| e.message.contains("Zero-Stack-Overflow")));
}

#[test]
fn test_ok_layer2_bitstruct_hardware_register() {
    let src = r#"
        bitstruct PageEntry: u32 {
            present: u1,
            rw: u1,
            user: u1,
            reserved: u9,
            frame: u20,
        }
        fn main() -> u32 {
            let pte: PageEntry = PageEntry { present: 1, rw: 1, user: 0, reserved: 0, frame: 1024 };
            return pte.frame;
        }
    "#;
    let c_code = compile_source(src, "bitstruct.vt", ".", false).expect("bitstruct must compile");
    assert!(c_code.contains("uint32_t present : 1;"));
    assert!(c_code.contains("uint32_t frame : 20;"));
}

#[test]
fn test_err_layer2_bitstruct_field_overflow() {
    assert_err_stage(
        r#"
        bitstruct PageEntry: u32 { present: u1, frame: u20 }
        fn main() -> i32<0..0> {
            let bad: PageEntry = PageEntry { present: 2, frame: 0 };
            return 0;
        }
    "#,
        Stage::Verifier,
    );
}

#[test]
fn test_ok_layer2_irq_safe_and_no_alloc_handler() {
    assert_ok(
        r#"
        #[irq_safe]
        #[no_alloc]
        #[lock_free]
        fn fast_timer_isr(ticks: u32<0..1000000>) -> u32 { return ticks + 1; }
        fn main() -> u32 { return fast_timer_isr(10); }
    "#,
    );
}

#[test]
fn test_err_layer2_lock_inside_interrupt_blocked() {
    assert_err_stage(
        r#"
#[locks]
fn take_spinlock() -> () { return; }
#[interrupt]
fn bad_isr() -> () { take_spinlock(); return; }
fn main() -> i32<0..0> { return 0; }
"#,
        Stage::Verifier,
    );
}
#[test]
fn test_ok_layer2_verified_inline_asm_with_contracts() {
    let src = r#"
#[port(0x80..0x80)]
fn io_wait_asm() -> () {
asm!("outb %al, %dx", clobbers: "eax, edx");
return;
}
#[port(0x80..0x80)]
fn main() -> i32<0..0> { io_wait_asm(); return 0; }
"#;
    let c_code = compile_source(src, "inline_asm.vt", ".", true)
        .expect("Verified asm! with #[port] must compile");
    assert!(c_code.contains("outb %al, %dx"));
}
#[test]
fn test_err_layer2_inline_asm_port_without_contract_blocked() {
    assert_err_stage(
        r#"
fn rogue_asm() -> () { asm!("outb %al, %dx", clobbers: "eax"); return; }
fn main() -> i32<0..0> { rogue_asm(); return 0; }
"#,
        Stage::Verifier,
    );
}
#[test]
fn test_ok_layer3_dynamic_slices_and_str() {
    assert_ok(
        r#"
fn count_bytes(slice: &[u8], msg: str) -> i32<0..65535> {
if msg.len > 0 && slice.len > 0 { return 1; }
return 0;
}
fn main() -> i32<0..65535> {
let buf: [u8; 4] = [0; 4];
return count_bytes(&buf, "hello veto");
}
"#,
    );
}
#[test]
fn test_ok_layer3_first_class_arena_with_bounded_id() {
    let src = r#"
struct AstNode { kind: u8, val: i32<0..1000> }
arena AstArena<AstNode, 256>;
fn main() -> i32<0..255> {
let id: i32<0..255> = AstArena.alloc(AstNode { kind: 1, val: 42 });
return id;
}
"#;
    let c_code =
        compile_source(src, "arena.vt", ".", false).expect("arena with bounded Id must compile");
    assert!(c_code.contains("struct AstArena_Arena"));
    assert!(c_code.contains("AstArena__alloc("));
}
#[test]
fn test_ok_layer3_niche_option_and_result() {
    assert_ok(
        r#"
fn find_byte(flag: bool) -> Option<u8<0..254>> {
if flag { return Some(42); }
return None;
}
fn main() -> i32<-1..254> {
let res: Option<u8<0..254>> = find_byte(true);
return res;
}
"#,
    );
}
#[test]
fn test_ok_layer3_generics_trait_and_comptime() {
    assert_ok(
        r#"
trait Measurable { fn size(&self) -> u32; }
struct RingBuf { head: u8, tail: u8 }
comptime fn calc_mask() -> u32 { return (1 << 8) - 1; }
fn main() -> u32 {
let mask: u32 = comptime calc_mask();
let rb: RingBuf = RingBuf { head: 0, tail: 0 };
if rb.head == 0 { return mask; }
return 0;
}
"#,
    );
}
#[test]
fn test_ok_native_x86_asm_and_test_runner() {
    let asm = compile_to_asm("fn main() -> i32<0..10> { return 7; }", "ok.vt", ".", false).unwrap();
    assert!(asm.contains("movl $7, %eax"));
    let (_, cnt) =
        compile_test_runner("#[test] fn t1() -> i32<0..0> { return 0; }", "t.vt", ".").unwrap();
    assert_eq!(cnt, 1);
}
#[test]
fn test_ok_layer4_x86_64_and_arm64_backends() {
    let src = "fn add_fast(a: i32<0..100>, b: i32<0..100>) -> i32<0..200> { return a + b; }";
    let asm64 = compile_to_asm_arch(src, "arch64.vt", ".", false, "x86_64").unwrap();
    assert!(asm64.contains(".code64"));
    assert!(asm64.contains("addq %rsi, %rax"));
    let arm64 = compile_to_asm_arch(src, "arm64.vt", ".", false, "arm64").unwrap();
    assert!(arm64.contains("add x0, x0, x1"));
}
#[test]
fn test_ok_layer4_modules_namespaces_and_manifest() {
    assert_ok(
        r#"
mod uart {
fn init() -> u8 { return 1; }
}
mod vga {
fn init() -> u8 { return 2; }
}
fn main() -> u16 {
return uart__init() + vga__init();
}
"#,
    );
    let manifest = r#"
name = "veto_kernel"
entry = "nonexistent_fallback.vt"
target = "kernel"
"#;
    let c_out = compile_project_manifest(manifest, ".").unwrap();
    assert!(c_out.contains("multiboot_header"));
}
#[test]
fn test_ok_layer4_smt_reflexivity_solver() {
    let env = std::collections::HashMap::new();
    let rep = vetoc::proofgen::smt_simplify_diff("idx * 2 + 1", "idx * 2 + 1", &env).unwrap();
    assert_eq!(rep.outcome, vetoc::proofgen::Outcome::Proven);
}
