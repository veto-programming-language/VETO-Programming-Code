# VETO 4.1 — Zero-Unsafe, Formally Verified Systems Programming Language

> **"No `unsafe` escape hatches. No runtime bounds checks. No silent stack overflows. Every invariant is mathematically proven before a single instruction is emitted."**

**VETO** is a statically verified, ahead-of-time compiled systems programming language built from the ground up for **Ring-0 OS kernels, bare-metal firmware, device drivers, and real-time engines**. Unlike C or Rust—where hardware access, memory-mapped I/O, inline assembly, and complex indexing eventually force developers into `unsafe` blocks or runtime panics—VETO proves safety at compile time using **Dependent Interval Types**, **Linear Resource Tracking**, **Typestate Machines**, **Whole-Program Stack Depth Analysis**, and a hybrid **Formal SMT + AI Inquisitor** proof engine.

---

## Table of Contents

1. [Why VETO Exists (Comparison with C & Rust)](#1-why-veto-exists-comparison-with-c--rust)
2. [Core Pillar 1: Dependent Interval Types & Path-Sensitive Refinement](#2-core-pillar-1-dependent-interval-types--path-sensitive-refinement)
3. [Core Pillar 2: Capability-Based Hardware Security (`#[mmio]` & `#[port]`)](#3-core-pillar-2-capability-based-hardware-security-mmio--port)
4. [Core Pillar 3: Compile-Time Typestate Machines (`#[requires]` & `#[ensures]`)](#4-core-pillar-3-compile-time-typestate-machines-requires--ensures)
5. [Core Pillar 4: Zero Stack Overflow & Bounded Termination (`#[terminates]`)](#5-core-pillar-4-zero-stack-overflow--bounded-termination-terminates)
6. [Core Pillar 5: Linear Types & Must-Consume Resources (`#[linear]`)](#6-core-pillar-5-linear-types--must-consume-resources-linear)
7. [Core Pillar 6: Execution Contexts (`#[irq_safe]`, `#[no_alloc]`, `#[lock_free]`)](#7-core-pillar-6-execution-contexts-irq_safe-no_alloc-lock_free)
8. [Core Pillar 7: Hardware Bitfields (`bitstruct`) & Verified Inline Assembly (`asm!`)](#8-core-pillar-7-hardware-bitfields-bitstruct--verified-inline-assembly-asm)
9. [Core Pillar 8: Self-Hosting Primitives (`arena`, Slices, Niche `Option`/`Result`, `comptime`)](#9-core-pillar-8-self-hosting-primitives-arena-slices-niche-optionresult-comptime)
10. [Core Pillar 9: Formal SMT Solver & The AI Inquisitor (`prove(...)`)](#10-core-pillar-9-formal-smt-solver--the-ai-inquisitor-prove)
11. [Multi-Architecture Native Backends (`x86`, `x86_64`, `arm64`)](#11-multi-architecture-native-backends-x86-x86_64-arm64)
12. [Installation, CLI Reference & Project Manifests (`Veto.toml`)](#12-installation-cli-reference--project-manifests-vetotoml)

---

## 1. Why VETO Exists (Comparison with C & Rust)

| Safety & Hardware Guarantee | C | Rust | **VETO 3.0** |
| :--- | :---: | :---: | :---: |
| **Array Out-of-Bounds Elimination** | ❌ UB | ⚠️ Runtime `panic!` | ✅ **Compile-time mathematical proof (0 runtime cost)** |
| **Division-by-Zero & Overflow Prevention** | ❌ UB | ⚠️ Debug panic / wrapping | ✅ **Statically refuted by interval arithmetic** |
| **MMIO & Port I/O Without `unsafe`** | ❌ Raw pointers | ❌ Requires `unsafe` | ✅ **100% Safe via `#[mmio]` & `#[port]` windows** |
| **Transitive Hardware Capability Tracking** | ❌ None | ❌ None | ✅ **Enforced across the entire call graph** |
| **Hardware Init Order (`IDT` before `STI`)** | ❌ Runtime triple fault | ⚠️ Wrapper boilerplate | ✅ **Native `#[requires]` / `#[ensures]` Typestates** |
| **Static Kernel Stack Overflow Proof** | ❌ None | ❌ None | ✅ **Whole-program call-graph + ISR stack bound** |
| **Interrupt Context Deadlock Prevention** | ❌ None | ❌ Runtime `try_lock` | ✅ **Compile-time `#[irq_safe]` / `#[lock_free]`** |
| **Guaranteed Loop Termination** | ❌ Hangs CPU | ❌ Hangs CPU | ✅ **`#[terminates]` & `#[max_cycles(N)]` proofs** |
| **Unforgettable Resource Cleanup** | ❌ Manual | ⚠️ `mem::forget` leaks | ✅ **True `#[linear]` types (must be consumed once)** |
| **Verified Inline Assembly (`asm!`)** | ❌ Black box | ❌ `unsafe` black box | ✅ **Checked clobbers, port contracts & IRQ rules** |

---

## 2. Core Pillar 1: Dependent Interval Types & Path-Sensitive Refinement

In VETO, every numeric type carries an explicit or inferred compile-time closed interval `[min..max]`.

### Primitive Types
- **Signed Integers**: `i8`, `i16`, `i32`, `i64`
- **Unsigned Integers**: `u8`, `u16`, `u32`, `u64`, `usize`
- **Boolean & Unit**: `bool` (`true` / `false`), `()` (zero-cost unit return)
- **Constrained Intervals**: `i32<0..1999>`, `u8<0..254>`, `i32<-256..256>`

### Path-Sensitive & Relational Refinement
The verifier tracks variable intervals across branches (`if` / `else`, `match`, `while`, `for`) and understands **relational constraints between variables** (`i < len`, Yoda conditions `10 < x`, inequalities `!=`, non-negative modulo `x % N` when `x >= 0`, and bitwise masks `x & 0xFF`).

```rust
fn safe_vga_offset(row: i32<0..24>, col: i32<0..79>) -> i32<0..1999> {
    // Verifier proves: [0..24] * 80 + [0..79] = [0..1999]
    // Fits into the return contract <0..1999> with zero runtime checks.
    return row * 80 + col;
}

fn sum_prefix(buf: &[u8; 32], len: i32<0..32>) -> i32<0..32> {
    let mut i: i32<0..64> = 0;
    while i < len {
        // Inside this branch, relational refinement narrows `i` from [0..64] to [0..31].
        // Indexing `buf[i]` is proven 100% in-bounds!
        let _byte: u8 = buf[i];
        i += 1;
    }
    return i;
}
```

---

## 3. Core Pillar 2: Capability-Based Hardware Security (`#[mmio]` & `#[port]`)

In traditional kernels, any third-party driver or helper function can silently execute `outb` to the keyboard controller (`0x64`) and reboot the CPU, or overwrite page tables via raw pointers.

VETO enforces **Capability-Based Security**:
1. Direct hardware primitives (`mem_read8`, `mem_write8`, `mem_read32`, `mem_write32`, `port_in8`, `port_out8`) are **forbidden** unless the enclosing function declares the exact physical address or port range via `#[mmio(lo..hi)]` or `#[port(lo..hi)]`.
2. The verifier proves that the target address expression falls strictly inside `[lo..hi]`.
3. **Transitive Propagation**: If `driver_beep()` requires `#[port(0x42..0x61)]`, any caller invoking `driver_beep()` must also hold or delegate that capability. A rogue helper cannot secretly touch hardware.

```rust
#[mmio(0xB8000..0xB8FA0)]
#[port(0x3D4..0x3D5)]
fn vga_write_cell(offset: i32<0..1999>, ch: u8, color: u8) -> () {
    let addr: i32<753664..757662> = 0xB8000 + offset * 2;
    mem_write8(addr, ch);
    mem_write8(addr + 1, color);
    port_out8(0x3D4, 0x0F); // Cursor low byte register
    return;
}
```

---

## 4. Core Pillar 3: Compile-Time Typestate Machines (`#[requires]` & `#[ensures]`)

Booting an operating system requires a strict sequence of hardware state transitions (e.g., loading the GDT and IDT before enabling interrupts with `sti`, or enabling paging before mapping kernel heap).

VETO tracks abstract system states statically along all control-flow paths (intersecting states at `if`/`else` merge points):

```rust
#[ensures(GdtReady)]
fn init_gdt() -> () {
    // Load Global Descriptor Table
    return;
}

#[requires(GdtReady)]
#[ensures(IdtReady)]
fn init_idt() -> () {
    idt_init();
    return;
}

#[requires(IdtReady)]
fn start_scheduler() -> () {
    cpu_sti(); // Proven safe: IdtReady holds!
    return;
}
```
Calling `start_scheduler()` or `cpu_sti()` before `init_idt()` fails compilation immediately at the `Verifier` stage.

---

## 5. Core Pillar 4: Zero Stack Overflow & Bounded Termination (`#[terminates]`)

### Static Kernel Stack Analysis (`Zero-Stack-Overflow`)
In Ring-0 mode (`--kernel`), VETO builds the full call graph, computes exact stack frame sizes (locals, arrays, parameters, saved registers, and `pusha` frames for `#[interrupt]` handlers), detects unbounded recursion (requiring `#[max_depth(N)]`), and proves that the worst-case execution path plus nested interrupts fits inside the **16,384-byte kernel stack** (configurable via `#[stack_limit(N)]`).

### Guaranteed Termination (`#[terminates]` & `#[max_cycles(N)]`)
Device drivers and interrupt service routines cannot afford infinite loops. Functions annotated with `#[terminates]` or `#[max_cycles(N)]`:
- **Forbid** unconditional `loop { ... }` statements.
- **Require** `while` loops to have a monotonically progressing induction variable toward the bound.
- **Verify** that `for` loop iteration counts never exceed `N`.

```rust
#[terminates]
#[max_cycles(64)]
fn poll_uart_ready() -> i32<0..64> {
    let mut spins: i32<0..64> = 0;
    while spins < 32 {
        spins += 1;
    }
    return spins;
}
```

---

## 6. Core Pillar 5: Linear Types & Must-Consume Resources (`#[linear]`)

Unlike Rust's affine types—which can be silently dropped or leaked via `std::mem::forget`—a `#[linear]` struct in VETO **must be consumed exactly once** along every execution path before the function returns.

```rust
#[linear]
struct SpinLockGuard {
    lock_id: u8,
}

fn acquire_lock(id: u8) -> SpinLockGuard {
    return SpinLockGuard { lock_id: id };
}

#[consume]
fn release_lock(guard: SpinLockGuard) -> () {
    return;
}

fn critical_section() -> () {
    let g: SpinLockGuard = acquire_lock(0);
    // Forgetting `release_lock(g)` before `return` is a compile-time error!
    release_lock(g);
    return;
}
```

---

## 7. Core Pillar 6: Execution Contexts (`#[irq_safe]`, `#[no_alloc]`, `#[lock_free]`)

Interrupt Service Routines (`#[interrupt]`) automatically inherit `#[irq_safe]`, `#[no_alloc]`, and `#[lock_free]`. Across the entire transitive call chain, the compiler forbids:
- **Sleeping or halting** (`cpu_halt()`, `cpu_sti()`, or `#[may_sleep]` functions) inside `#[irq_safe]`.
• Acquiring locks (acquire_*, lock_*, or #[locks] functions) inside #[lock_free] / #[interrupt] (eliminating IRQ deadlocks).
• Heap allocations (alloc_*, malloc, or #[allocates] functions) inside #[no_alloc].
rust #[irq_safe] #[no_alloc] #[lock_free] fn pit_timer_tick(ticks: u32<0..1000000>) -> u32 { return ticks + 1; } 

8. Core Pillar 7: Hardware Bitfields (bitstruct) & Verified Inline Assembly (asm!)


Native Bitfields (bitstruct)

Define hardware page table entries, IDT descriptors, and PCI configuration registers directly with automatic bit-width range bounds (u1 = <0..1>, u20 = <0..1048575>):
```rust
bitstruct PageTableEntry: u32 {
present: u1,
rw: u1,
user: u1,
reserved: u9,
frame_addr: u20,
}
fn make_kernel_pte(frame: u32<0..1048575>) -> PageTableEntry {
return PageTableEntry {
present: 1,
rw: 1,
user: 0,
reserved: 0,
frame_addr: frame,
};
}
```

Verified Inline Assembly (asm!)

Inline assembly in VETO is never an unchecked unsafe hole:
• Clobbering stack pointers (esp, ebp, rsp, rbp) is rejected.
• Executing inb/outb/outw/outl requires an active #[port(lo..hi)] contract on the function.
• Executing hlt or sti inside #[irq_safe] / #[interrupt] is rejected.
rust #[port(0x80..0x80)] fn io_delay() -> () { asm!("outb %al, %dx", clobbers: "eax, edx"); return; } 

9. Core Pillar 8: Self-Hosting Primitives (arena, Slices, Niche Option/Result, comptime)

VETO includes everything needed to build compilers, parsers, and kernels without a garbage collector or heap allocator:
• First-Class Static Arenas (arena): Allocates fixed-capacity storage in .bss and returns a bounded index Id proven to be within 0..(Capacity - 1).
• Dynamic Slices (&[T]) & Strings (str): Built-in .len tracking integrated directly into the relational interval verifier.
• Niche-Optimized Option<T> & Result<T, E>: Option<u8<0..254>> packs into a single byte using 255 (-1) as None with zero tag overhead.
• Generics (<T>), trait Interfaces & comptime: Monomorphized generics and compile-time constant evaluation (comptime fn).
```rust
struct AstNode {
kind: u8,
value: i32<0..10000>,
}
arena AstPool<AstNode, 512>;
comptime fn default_mask() -> u32 {
return (1 << 8) - 1;
}
fn allocate_node() -> i32<0..511> {
let id: i32<0..511> = AstPool.alloc(AstNode { kind: 1, value: 42 });
return id;
}
```

10. Core Pillar 9: Formal SMT Solver & The AI Inquisitor (prove(...))

When complex non-linear invariants arise, developers write prove(condition);. VETO resolves proofs in a two-stage pipeline:
1. Formal Proof Generator & Linear SMT Simplifier (proofgen): Evaluates interval bounds and algebraic reflexivity/difference axioms (x - y <= c) to emit a step-by-step mathematical proof (Outcome::Proven) or refutation (Outcome::Refuted).
2. The AI Inquisitor (inquisitor): If intervals overlap (Outcome::Unknown) and VETOC_ALLOW_MANUAL=1 is enabled, the embedded neural classifier (MiniModel MLP) puts the developer on trial—evaluating formal mathematical justifications, caching proven invariants in SQLite, and remembering weak excuses ("it works on my machine") for 7 days.

11. Multi-Architecture Native Backends (x86, x86_64, arm64)

VETO can emit freestanding C11 or bypass GCC completely using its built-in Native Assembly Backends with peephole optimization and register fast-paths:
• 32-bit x86 (--arch x86): Full Multiboot kernel bootstrap, 16 KB aligned stack, hardware inb/outb, VGA text buffer primitives, and iret interrupt frames.
• 64-bit x86_64 (--arch x86_64): System V AMD64 ABI (%rdi, %rsi, %rdx, %rcx, %r8, %r9), 64-bit stack frames (pushq %rbp / movq %rsp, %rbp), and 32 KB .bss kernel stack.
• 64-bit ARM64 (--arch arm64): AArch64 ABI (x0..x7, stp x29, x30, [sp, #-16]!, ret).

12. Installation, CLI Reference & Project Manifests (Veto.toml)


Building & Installing from Source

bash git clone https://github.com/veto-programming-language/VETO-Programming-Code.git cd VETO-Programming-Code cargo install --path . --features inquisitor 

CLI Usage

```bash

Compile a user-space application (auto-links SDL2 when gfx_* is used)

vetoc doom.vt -o doom

Compile a freestanding Ring-0 x86 kernel (Multiboot ELF)

vetoc kernel.vt --kernel -o kernel.elf

Emit native x86_64 or ARM64 assembly with zero GCC dependency

vetoc app.vt --native-asm --arch x86_64 -o app_x64.s
vetoc app.vt --native-asm --arch arm64  -o app_arm64.s

Build a multi-file project from Veto.toml

vetoc Veto.toml -o build.c

Run built-in #[test] verification suite

vetoc suite.vt --test
```

Namespaces (mod) & Veto.toml

Organize large kernels and compilers cleanly without symbol collisions:
rust mod uart { #[port(0x3F8..0x3FF)] fn init() -> () { port_out8(0x3F9, 0x00); return; } } 
toml name = "veto_kernel" entry = "kernel.vt" target = "kernel" 

License

Licensed under either of Apache License, Version 2.0 or MIT License at your option.
