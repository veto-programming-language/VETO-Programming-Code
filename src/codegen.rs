use crate::ast::*;
use crate::verifier::{eval_expr_range, refine_envs, ArrayInfo, FnSig};
use std::collections::HashMap;

fn block_always_returns(stmts: &[Stmt]) -> bool {
    match stmts.last() {
        Some(Stmt::Return { .. }) => true,
        Some(Stmt::If {
            then_body,
            else_body,
            ..
        }) => {
            !else_body.is_empty()
                && block_always_returns(then_body)
                && block_always_returns(else_body)
        }
        _ => false,
    }
}

fn expr_to_c(expr: &Expr) -> String {
    match expr {
        Expr::Number(n) => n.to_string(),
        Expr::Str(s) => format!("\"{}\"", s.replace('"', "\\\"")),
        Expr::Var(v) => v.clone(),
        Expr::Field(obj, f) => format!("{}.{}", obj, f),
        Expr::StructInit(st, inits) => {
            let fs: StrList = inits
                .iter()
                .map(|(k, v)| format!(".{} = {}", k, expr_to_c(v)))
                .collect();
            format!("(struct {}){{ {} }}", st, fs.join(", "))
        }
        Expr::Index(arr, idx) => format!("{}[{}]", arr, expr_to_c(idx)),
        Expr::UnaryNot(inner) => format!("(!{})", expr_to_c(inner)),
        Expr::Call(name, args) => {
            let args_c: StrList = args.iter().map(expr_to_c).collect();
            format!("{}({})", name, args_c.join(", "))
        }
        Expr::BinOp(l, op, r) => format!("({} {} {})", expr_to_c(l), op, expr_to_c(r)),
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_stmts(
    stmts: &[Stmt],
    func: &Function,
    env: &mut HashMap<String, (i128, i128)>,
    declared: &mut HashMap<String, (i128, i128)>,
    arrays: &mut HashMap<String, ArrayInfo>,
    sigs: &HashMap<String, FnSig>,
    source: &str,
    indent: &str,
    out: &mut String,
) {
    let _ = source;
    for stmt in stmts {
        match stmt {
            Stmt::Let {
                line,
                name,
                type_name,
                min_val,
                max_val,
                value,
            } => {
                if let Expr::StructInit(st_name, inits) = value {
                    if *min_val == i64::MIN && *max_val == i64::MAX {
                        for (fname, fexpr) in inits {
                            let key = format!("{}.{}", name, fname);
                            let (fmin, fmax) = eval_expr_range(fexpr, env, arrays, sigs)
                                .unwrap_or((i32::MIN as i128, i32::MAX as i128));
                            if let Some(&(cmin, cmax)) =
                                declared.get(&format!("__struct_{}.{}", st_name, fname))
                            {
                                if fmin < cmin || fmax > cmax {
                                    eprintln!("\n[VETO | строка {}]: Поле {}.{} нарушает контракт <{}..{}> (даёт [{}..{}])!", line, st_name, fname, cmin, cmax, fmin, fmax);
                                    std::process::exit(1);
                                }
                                env.insert(key.clone(), (fmin, fmax));
                                declared.insert(key, (cmin, cmax));
                            }
                        }
                        out.push_str(&format!(
                            "{}struct {} {} = {};\n",
                            indent,
                            type_name,
                            name,
                            expr_to_c(value)
                        ));
                        continue;
                    }
                }
                if *min_val > *max_val {
                    eprintln!("\n[VETO | строка {}]: Некорректный диапазон у {}: минимум ({}) > максимума ({})!", line, name, min_val, max_val);
                    std::process::exit(1);
                }
                match eval_expr_range(value, env, arrays, sigs) {
                    Ok((expr_min, expr_max)) => {
                        if expr_min < *min_val as i128 || expr_max > *max_val as i128 {
                            eprintln!("\n[VETO | строка {}]: Ошибка диапазона! {} объявлена как <{}..{}>, но выражение даёт [{}..{}]!", line, name, min_val, max_val, expr_min, expr_max);
                            std::process::exit(1);
                        }
                    }
                    Err(err) => {
                        if *min_val != i64::MIN || *max_val != i64::MAX {
                            eprintln!("\n[VETO | строка {}]: Не могу доказать диапазон для {} (<{}..{}>): {}", line, name, min_val, max_val, err);
                            std::process::exit(1);
                        }
                    }
                }
                if !(*min_val == i64::MIN && *max_val == i64::MAX) {
                    let bounds = (*min_val as i128, *max_val as i128);
                    env.insert(name.clone(), bounds);
                    declared.insert(name.clone(), bounds);
                }
                out.push_str(&format!(
                    "{}int32_t {} = {};\n",
                    indent,
                    name,
                    expr_to_c(value)
                ));
            }
            Stmt::LetArray {
                line,
                name,
                min_val,
                max_val,
                len,
                elements,
            } => {
                if elements.len() != *len {
                    eprintln!("\n[VETO | строка {}]: Массив {} объявлен длиной {}, но передано {} элементов!", line, name, len, elements.len());
                    std::process::exit(1);
                }
                let mut elems_c = Vec::new();
                for (i, el) in elements.iter().enumerate() {
                    match eval_expr_range(el, env, arrays, sigs) {
                        Ok((emin, emax)) => {
                            if emin < *min_val as i128 || emax > *max_val as i128 {
                                eprintln!("\n[VETO | строка {}]: Элемент {}[{}] выходит за диапазон <{}..{}>!", line, name, i, min_val, max_val);
                                std::process::exit(1);
                            }
                        }
                        Err(err) => {
                            eprintln!(
                                "\n[VETO | строка {}]: Ошибка в элементе {}[{}]: {}",
                                line, name, i, err
                            );
                            std::process::exit(1);
                        }
                    }
                    elems_c.push(expr_to_c(el));
                }
                arrays.insert(
                    name.clone(),
                    ArrayInfo {
                        len: *len,
                        elem_min: *min_val as i128,
                        elem_max: *max_val as i128,
                    },
                );
                out.push_str(&format!(
                    "{}int32_t {}[{}] = {{{}}};\n",
                    indent,
                    name,
                    len,
                    elems_c.join(", ")
                ));
            }
            Stmt::FieldAssign {
                line,
                obj,
                field,
                value,
            } => {
                let key = format!("{}.{}", obj, field);
                if let Some(&(decl_min, decl_max)) = declared.get(&key) {
                    match eval_expr_range(value, env, arrays, sigs) {
                        Ok((emin, emax)) => {
                            if emin < decl_min || emax > decl_max {
                                eprintln!("\n[VETO | строка {}]: Поле {} нарушает контракт <{}..{}> (даёт [{}..{}])!", line, key, decl_min, decl_max, emin, emax);
                                std::process::exit(1);
                            }
                            env.insert(key, (emin, emax));
                        }
                        Err(err) => {
                            eprintln!(
                                "\n[VETO | строка {}]: Не могу доказать поле {}: {}",
                                line, key, err
                            );
                            std::process::exit(1);
                        }
                    }
                }
                out.push_str(&format!(
                    "{}{}.{} = {};\n",
                    indent,
                    obj,
                    field,
                    expr_to_c(value)
                ));
            }
            Stmt::Assign { line, name, value } => {
                if let Some(&(decl_min, decl_max)) = declared.get(name) {
                    match eval_expr_range(value, env, arrays, sigs) {
                        Ok((expr_min, expr_max)) => {
                            if expr_min < decl_min || expr_max > decl_max {
                                eprintln!("\n[VETO | строка {}]: Присваивание в {} нарушает контракт <{}..{}> (даёт [{}..{}])!", line, name, decl_min, decl_max, expr_min, expr_max);
                                std::process::exit(1);
                            }
                            env.insert(name.clone(), (expr_min, expr_max));
                        }
                        Err(err) => {
                            eprintln!(
                                "\n[VETO | строка {}]: Не могу доказать присваивание в {}: {}",
                                line, name, err
                            );
                            std::process::exit(1);
                        }
                    }
                }
                out.push_str(&format!("{}{} = {};\n", indent, name, expr_to_c(value)));
            }
            Stmt::IndexAssign {
                line,
                name,
                index,
                value,
            } => {
                if let Some(info) = arrays.get(name) {
                    match eval_expr_range(index, env, arrays, sigs) {
                        Ok((imin, imax)) => {
                            if imin < 0 || imax >= info.len as i128 {
                                eprintln!("\n[VETO | строка {}]: Выход за границы массива {}[0..{}] при записи: [{}..{}]!", line, name, info.len - 1, imin, imax);
                                std::process::exit(1);
                            }
                        }
                        Err(err) => {
                            eprintln!(
                                "\n[VETO | строка {}]: Не могу доказать индекс записи в {}: {}",
                                line, name, err
                            );
                            std::process::exit(1);
                        }
                    }
                    match eval_expr_range(value, env, arrays, sigs) {
                        Ok((vmin, vmax)) => {
                            if vmin < info.elem_min || vmax > info.elem_max {
                                eprintln!("\n[VETO | строка {}]: Значение [{}..{}] выходит за тип массива {}!", line, vmin, vmax, name);
                                std::process::exit(1);
                            }
                        }
                        Err(err) => {
                            eprintln!(
                                "\n[VETO | строка {}]: Не могу доказать значение для {}: {}",
                                line, name, err
                            );
                            std::process::exit(1);
                        }
                    }
                }
                out.push_str(&format!(
                    "{}{}[{}] = {};\n",
                    indent,
                    name,
                    expr_to_c(index),
                    expr_to_c(value)
                ));
            }
            Stmt::Loop { body, .. } => {
                for (k, v) in declared.iter() {
                    env.insert(k.clone(), *v);
                }
                out.push_str(&format!("{}while (1) {{\n", indent));
                emit_stmts(
                    body,
                    func,
                    env,
                    declared,
                    arrays,
                    sigs,
                    source,
                    &format!("{}    ", indent),
                    out,
                );
                out.push_str(&format!("{}}}\n", indent));
            }
            Stmt::While { cond, body, .. } => {
                out.push_str(&format!("{}while ({}) {{\n", indent, expr_to_c(cond)));
                let (mut body_env, _) = refine_envs(cond, env);
                emit_stmts(
                    body,
                    func,
                    &mut body_env,
                    declared,
                    arrays,
                    sigs,
                    source,
                    &format!("{}    ", indent),
                    out,
                );
                out.push_str(&format!("{}}}\n", indent));
            }
            Stmt::Break { .. } => out.push_str(&format!("{}break;\n", indent)),
            Stmt::Continue { .. } => out.push_str(&format!("{}continue;\n", indent)),
            Stmt::ExprStmt { line, expr } => {
                if let Err(err) = eval_expr_range(expr, env, arrays, sigs) {
                    eprintln!("\n[VETO | строка {}]: Ошибка при вызове: {}", line, err);
                    std::process::exit(1);
                }
                out.push_str(&format!("{}{};\n", indent, expr_to_c(expr)));
            }
            Stmt::If {
                cond,
                then_body,
                else_body,
                ..
            } => {
                out.push_str(&format!("{}if ({}) {{\n", indent, expr_to_c(cond)));
                let (mut then_env, mut else_env) = refine_envs(cond, env);
                let mut then_decl = declared.clone();
                let mut else_decl = declared.clone();
                emit_stmts(
                    then_body,
                    func,
                    &mut then_env,
                    &mut then_decl,
                    arrays,
                    sigs,
                    source,
                    &format!("{}    ", indent),
                    out,
                );
                if !else_body.is_empty() {
                    out.push_str(&format!("{}}} else {{\n", indent));
                    emit_stmts(
                        else_body,
                        func,
                        &mut else_env,
                        &mut else_decl,
                        arrays,
                        sigs,
                        source,
                        &format!("{}    ", indent),
                        out,
                    );
                }
                out.push_str(&format!("{}}}\n", indent));
                if block_always_returns(then_body) {
                    *env = else_env;
                } else if !else_body.is_empty() && block_always_returns(else_body) {
                    *env = then_env;
                } else {
                    for (k, &(t_lo, t_hi)) in then_env.iter() {
                        if let Some(&(e_lo, e_hi)) = else_env.get(k) {
                            if let Some(&(orig_lo, orig_hi)) = env.get(k) {
                                let new_lo = t_lo.min(e_lo).max(orig_lo);
                                let new_hi = t_hi.max(e_hi).min(orig_hi);
                                if new_lo <= new_hi {
                                    env.insert(k.clone(), (new_lo, new_hi));
                                }
                            }
                        }
                    }
                }
            }
            Stmt::Prove { line, cond } => {
                #[cfg(feature = "inquisitor")]
                {
                    use crate::inquisitor::{Inquisitor, Verdict};
                    let mut inq = Inquisitor::new(cond.trim(), source);
                    match inq.auto_prove_with(env) {
                        Verdict::Approve => {}
                        Verdict::Reject => {
                            eprintln!(
                                "[VETO | строка {}] Инквизитор отклонил доказательство",
                                line
                            );
                            std::process::exit(1);
                        }
                    }
                }
                #[cfg(not(feature = "inquisitor"))]
                {
                    let _ = (&env, line);
                }
            }
            Stmt::Return { line, expr } => {
                if func.ret_min != i64::MIN || func.ret_max != i64::MAX {
                    match eval_expr_range(expr, env, arrays, sigs) {
                        Ok((ret_lo, ret_hi)) => {
                            if ret_lo < func.ret_min as i128 || ret_hi > func.ret_max as i128 {
                                eprintln!("\n[VETO | строка {}]: Нарушен возврат {} (<{}..{}>, даёт [{}..{}])!", line, func.name, func.ret_min, func.ret_max, ret_lo, ret_hi);
                                std::process::exit(1);
                            }
                        }
                        Err(err) => {
                            eprintln!(
                                "\n[VETO | строка {}]: Не могу доказать возврат {}: {}",
                                line, func.name, err
                            );
                            std::process::exit(1);
                        }
                    }
                }
                out.push_str(&format!("{}return {};\n", indent, expr_to_c(expr)));
            }
        }
    }
}
const KERNEL_HEADER: &str = r#"#include <stdint.h>
__attribute__((section(".multiboot"), aligned(4), used))
static const uint32_t multiboot_header[] = {
    0x1BADB002,
    0x00000000,
    0xE4524FFE
};

uint8_t kernel_stack[16384] __attribute__((aligned(16)));

__asm__(".global _start\n_start:\n cli\n mov $(kernel_stack + 16384), %esp\n and $-16, %esp\n call main\n cli\n.hang:\n hlt\n jmp .hang\n");
int32_t vga_poke(int32_t idx, int32_t ch, int32_t color) { volatile uint16_t *v = (volatile uint16_t *)0xB8000; v[idx] = ((uint16_t)(color & 0xFF) << 8) | (uint16_t)(ch & 0xFF); return 0; }
int32_t vga_print(int32_t row, int32_t col, const char *s, int32_t color) { volatile uint16_t *v = (volatile uint16_t *)0xB8000; int32_t p = row * 80 + col; for (int32_t i = 0; s[i] != '\0' && (p + i) < 2000; i++) v[p + i] = ((uint16_t)(color & 0xFF) << 8) | (uint16_t)s[i]; return 0; }
int32_t vga_set_cursor(int32_t row, int32_t col) { uint16_t p = (uint16_t)(row * 80 + col); __asm__ volatile ("outb %0, %1" : : "a"((uint8_t)0x0F), "Nd"((uint16_t)0x3D4)); __asm__ volatile ("outb %0, %1" : : "a"((uint8_t)(p & 0xFF)), "Nd"((uint16_t)0x3D5)); __asm__ volatile ("outb %0, %1" : : "a"((uint8_t)0x0E), "Nd"((uint16_t)0x3D4)); __asm__ volatile ("outb %0, %1" : : "a"((uint8_t)((p >> 8) & 0xFF)), "Nd"((uint16_t)0x3D5)); return 0; }
int32_t port_out8(int32_t port, int32_t val) { __asm__ volatile ("outb %0, %1" : : "a"((uint8_t)val), "Nd"((uint16_t)port)); return 0; }
int32_t port_in8(int32_t port) { uint8_t r; __asm__ volatile ("inb %1, %0" : "=a"(r) : "Nd"((uint16_t)port)); return (int32_t)r; }
int32_t mem_read8(int32_t addr) { return (int32_t)(*(volatile uint8_t *)(uintptr_t)addr); }
int32_t mem_write8(int32_t addr, int32_t val) { *(volatile uint8_t *)(uintptr_t)addr = (uint8_t)(val & 0xFF); return 0; }
int32_t mem_read32(int32_t addr) { return *(volatile int32_t *)(uintptr_t)addr; }
int32_t mem_write32(int32_t addr, int32_t val) { *(volatile int32_t *)(uintptr_t)addr = val; return 0; }
int32_t cpu_cli(void) { __asm__ volatile ("cli"); return 0; }
int32_t cpu_sti(void) { __asm__ volatile ("sti"); return 0; }
int32_t cpu_halt(void) { __asm__ volatile ("hlt"); return 0; }
int32_t cpu_reboot(void) { uint8_t g = 2; while (g & 2) __asm__ volatile ("inb %1, %0" : "=a"(g) : "Nd"((uint16_t)0x64)); __asm__ volatile ("outb %0, %1" : : "a"((uint8_t)0xFE), "Nd"((uint16_t)0x64)); return 0; }
struct idt_entry { uint16_t base_lo; uint16_t sel; uint8_t always0; uint8_t flags; uint16_t base_hi; } __attribute__((packed));
struct idt_ptr { uint16_t limit; uint32_t base; } __attribute__((packed));
static struct idt_entry idt[256]; static struct idt_ptr idtp; static volatile int32_t sys_ticks = 0; static volatile uint8_t key_buf[64]; static volatile uint8_t key_head = 0; static volatile uint8_t key_tail = 0;
void irq0_handler_c(void) { sys_ticks++; __asm__ volatile ("outb %0, %1" : : "a"((uint8_t)0x20), "Nd"((uint16_t)0x20)); }
void irq1_handler_c(void) { uint8_t sc; __asm__ volatile ("inb %1, %0" : "=a"(sc) : "Nd"((uint16_t)0x60)); uint8_t nx = (key_head + 1) & 63; if (nx != key_tail) { key_buf[key_head] = sc; key_head = nx; } __asm__ volatile ("outb %0, %1" : : "a"((uint8_t)0x20), "Nd"((uint16_t)0x20)); }
__asm__(".global isr_default\nisr_default:\n iret\n.global irq0_stub\nirq0_stub:\n pusha\n call irq0_handler_c\n popa\n iret\n.global irq1_stub\nirq1_stub:\n pusha\n call irq1_handler_c\n popa\n iret\n");
extern void isr_default(void); extern void irq0_stub(void); extern void irq1_stub(void);
static void idt_set_gate(uint8_t n, uint32_t b, uint16_t s, uint8_t f) { idt[n].base_lo = b & 0xFFFF; idt[n].base_hi = (b >> 16) & 0xFFFF; idt[n].sel = s; idt[n].always0 = 0; idt[n].flags = f; }
int32_t idt_init(void) {
    uint16_t cs_sel = 0x08;
    __asm__ volatile ("mov %%cs, %0" : "=r"(cs_sel));
    idtp.limit = sizeof(idt) - 1; idtp.base = (uint32_t)&idt;
    for (int i = 0; i < 256; i++) idt_set_gate((uint8_t)i, (uint32_t)isr_default, cs_sel, 0x8E);
    __asm__ volatile ("outb %0, %1" : : "a"((uint8_t)0x11), "Nd"((uint16_t)0x20)); __asm__ volatile ("outb %0, %1" : : "a"((uint8_t)0x11), "Nd"((uint16_t)0xA0));
    __asm__ volatile ("outb %0, %1" : : "a"((uint8_t)0x20), "Nd"((uint16_t)0x21)); __asm__ volatile ("outb %0, %1" : : "a"((uint8_t)0x28), "Nd"((uint16_t)0xA1));
    __asm__ volatile ("outb %0, %1" : : "a"((uint8_t)0x04), "Nd"((uint16_t)0x21)); __asm__ volatile ("outb %0, %1" : : "a"((uint8_t)0x02), "Nd"((uint16_t)0xA1));
    __asm__ volatile ("outb %0, %1" : : "a"((uint8_t)0x01), "Nd"((uint16_t)0x21)); __asm__ volatile ("outb %0, %1" : : "a"((uint8_t)0x01), "Nd"((uint16_t)0xA1));
    __asm__ volatile ("outb %0, %1" : : "a"((uint8_t)0xFC), "Nd"((uint16_t)0x21)); __asm__ volatile ("outb %0, %1" : : "a"((uint8_t)0xFF), "Nd"((uint16_t)0xA1));
    idt_set_gate(32, (uint32_t)irq0_stub, cs_sel, 0x8E); idt_set_gate(33, (uint32_t)irq1_stub, cs_sel, 0x8E);
    uint16_t d = 1193180 / 100;
    __asm__ volatile ("outb %0, %1" : : "a"((uint8_t)0x36), "Nd"((uint16_t)0x43));
    __asm__ volatile ("outb %0, %1" : : "a"((uint8_t)(d & 0xFF)), "Nd"((uint16_t)0x40));
    __asm__ volatile ("outb %0, %1" : : "a"((uint8_t)((d >> 8) & 0xFF)), "Nd"((uint16_t)0x40));
    __asm__ volatile ("lidt (%0)" : : "r"(&idtp)); __asm__ volatile ("sti"); return 0;
}
int32_t irq_ticks(void) { return sys_ticks; }
int32_t irq_pop_key(void) { if (key_head == key_tail) return 0; uint8_t sc = key_buf[key_tail]; key_tail = (key_tail + 1) & 63; return (int32_t)sc; }
"#;

const USER_HEADER: &str = r#"#include <stdint.h>
#include <stdlib.h>
#include <time.h>
#if __has_include(<SDL2/SDL.h>)
#include <SDL2/SDL.h>
#else
#include <SDL.h>
#endif
static SDL_Window *veto_win = NULL; static SDL_Renderer *veto_ren = NULL; static int32_t veto_zbuf[400];
int32_t gfx_open(int32_t w, int32_t h) { srand((unsigned int)time(NULL)); SDL_Init(SDL_INIT_VIDEO); veto_win = SDL_CreateWindow("VETO Engine", SDL_WINDOWPOS_CENTERED, SDL_WINDOWPOS_CENTERED, w, h, 0); veto_ren = SDL_CreateRenderer(veto_win, -1, SDL_RENDERER_ACCELERATED | SDL_RENDERER_PRESENTVSYNC); return 1; }
int32_t veto_rand(int32_t m) { return m <= 0 ? 0 : rand() % (m + 1); }
int32_t gfx_key_down(int32_t c) { SDL_Event e; while (SDL_PollEvent(&e)) if (e.type == SDL_QUIT) { SDL_Quit(); exit(0); } const uint8_t *k = SDL_GetKeyboardState(NULL); if (k[SDL_SCANCODE_ESCAPE]) { SDL_Quit(); exit(0); } if (c == 1) return (k[SDL_SCANCODE_Y] || k[SDL_SCANCODE_W]) ? 1 : 0; if (c == 2) return (k[SDL_SCANCODE_G] || k[SDL_SCANCODE_A]) ? 1 : 0; if (c == 3) return (k[SDL_SCANCODE_H] || k[SDL_SCANCODE_S]) ? 1 : 0; if (c == 4) return (k[SDL_SCANCODE_J] || k[SDL_SCANCODE_D]) ? 1 : 0; if (c == 5) return k[SDL_SCANCODE_SPACE] ? 1 : 0; return 0; }
int32_t gfx_clear(void) { for (int i = 0; i < 400; i++) veto_zbuf[i] = 9999; SDL_SetRenderDrawColor(veto_ren, 14, 16, 24, 255); SDL_Rect c = {0,0,800,300}; SDL_RenderFillRect(veto_ren, &c); SDL_SetRenderDrawColor(veto_ren, 38, 35, 32, 255); SDL_Rect f = {0,300,800,300}; SDL_RenderFillRect(veto_ren, &f); return 0; }
int32_t gfx_draw_vline(int32_t col, int32_t top, int32_t bot, int32_t shade, int32_t dist) { if (col >= 0 && col < 400) veto_zbuf[col] = dist; SDL_SetRenderDrawColor(veto_ren, (uint8_t)shade, (uint8_t)(shade/3), (uint8_t)(shade/4), 255); SDL_Rect r = {col*2, top, 2, bot-top}; SDL_RenderFillRect(veto_ren, &r); return 0; }
int32_t gfx_draw_enemy(int32_t sx, int32_t dist, int32_t fl) { if (dist <= 1) dist = 1; int32_t h = 460/dist; if (h < 16) h = 16; if (h > 520) h = 520; int32_t w = h/2, left = sx - w/2, top = 300 - h/2 + h/8; for (int32_t x = left; x < left+w; x += 2) { int32_t col = x/2; if (col >= 0 && col < 400 && dist < veto_zbuf[col]) { SDL_SetRenderDrawColor(veto_ren, fl>0?255:45, fl>0?255:200, fl>0?255:85, 255); SDL_Rect b = {x, top, 2, h}; SDL_RenderFillRect(veto_ren, &b); } } return 0; }
int32_t gfx_draw_gun(int32_t recoil, int32_t kills) { SDL_SetRenderDrawColor(veto_ren, 0, 255, 170, 255); SDL_Rect c1 = {392,299,16,2}, c2 = {399,292,2,16}; SDL_RenderFillRect(veto_ren, &c1); SDL_RenderFillRect(veto_ren, &c2); int32_t k = recoil*3; SDL_SetRenderDrawColor(veto_ren, 85, 92, 108, 255); SDL_Rect b = {382, 410+k, 36, 120}; SDL_RenderFillRect(veto_ren, &b); return 0; }
int32_t gfx_present(void) { SDL_RenderPresent(veto_ren); SDL_Delay(16); return 0; }
"#;

pub fn generate_c(program: &Program, source: &str, is_kernel: bool) -> String {
    let mut c_out = String::from(if is_kernel {
        KERNEL_HEADER
    } else {
        USER_HEADER
    });
    let mut sigs: HashMap<String, FnSig> = HashMap::new();
    for name in [
        "idt_init",
        "cpu_reboot",
        "cpu_cli",
        "cpu_sti",
        "cpu_halt",
        "gfx_clear",
        "gfx_present",
    ] {
        sigs.insert(
            name.into(),
            FnSig {
                params: vec![],
                ret_min: 0,
                ret_max: 0,
            },
        );
    }
    sigs.insert(
        "irq_ticks".into(),
        FnSig {
            params: vec![],
            ret_min: 0,
            ret_max: i32::MAX as i64,
        },
    );
    sigs.insert(
        "irq_pop_key".into(),
        FnSig {
            params: vec![],
            ret_min: 0,
            ret_max: 255,
        },
    );
    sigs.insert(
        "vga_poke".into(),
        FnSig {
            params: vec![
                ("idx".into(), 0, 1999),
                ("ch".into(), 0, 255),
                ("color".into(), 0, 255),
            ],
            ret_min: 0,
            ret_max: 0,
        },
    );
    sigs.insert(
        "vga_print".into(),
        FnSig {
            params: vec![
                ("row".into(), 0, 24),
                ("col".into(), 0, 79),
                ("str".into(), 0, 0),
                ("color".into(), 0, 255),
            ],
            ret_min: 0,
            ret_max: 0,
        },
    );
    sigs.insert(
        "vga_set_cursor".into(),
        FnSig {
            params: vec![("row".into(), 0, 24), ("col".into(), 0, 79)],
            ret_min: 0,
            ret_max: 0,
        },
    );
    sigs.insert(
        "port_out8".into(),
        FnSig {
            params: vec![("port".into(), 0, 65535), ("val".into(), 0, 255)],
            ret_min: 0,
            ret_max: 0,
        },
    );
    sigs.insert(
        "port_in8".into(),
        FnSig {
            params: vec![("port".into(), 0, 65535)],
            ret_min: 0,
            ret_max: 255,
        },
    );
    sigs.insert(
        "mem_read8".into(),
        FnSig {
            params: vec![("addr".into(), 0, i64::MAX)],
            ret_min: 0,
            ret_max: 255,
        },
    );
    sigs.insert(
        "mem_write8".into(),
        FnSig {
            params: vec![("addr".into(), 0, i64::MAX), ("val".into(), 0, 255)],
            ret_min: 0,
            ret_max: 0,
        },
    );
    sigs.insert(
        "mem_read32".into(),
        FnSig {
            params: vec![("addr".into(), 0, i64::MAX)],
            ret_min: i32::MIN as i64,
            ret_max: i32::MAX as i64,
        },
    );
    sigs.insert(
        "mem_write32".into(),
        FnSig {
            params: vec![
                ("addr".into(), 0, i64::MAX),
                ("val".into(), i64::MIN, i64::MAX),
            ],
            ret_min: 0,
            ret_max: 0,
        },
    );
    sigs.insert(
        "gfx_open".into(),
        FnSig {
            params: vec![("w".into(), 100, 1920), ("h".into(), 100, 1080)],
            ret_min: 0,
            ret_max: 1,
        },
    );
    sigs.insert(
        "veto_rand".into(),
        FnSig {
            params: vec![("max_val".into(), 1, 1000)],
            ret_min: 0,
            ret_max: 1000,
        },
    );
    sigs.insert(
        "gfx_key_down".into(),
        FnSig {
            params: vec![("code".into(), 1, 5)],
            ret_min: 0,
            ret_max: 1,
        },
    );
    sigs.insert(
        "gfx_draw_vline".into(),
        FnSig {
            params: vec![
                ("col".into(), 0, 399),
                ("top".into(), 0, 600),
                ("bot".into(), 0, 600),
                ("shade".into(), 0, 255),
                ("dist".into(), 1, 64),
            ],
            ret_min: 0,
            ret_max: 0,
        },
    );
    sigs.insert(
        "gfx_draw_enemy".into(),
        FnSig {
            params: vec![
                ("screen_x".into(), 0, 800),
                ("dist".into(), 1, 64),
                ("hurt_flash".into(), 0, 10),
            ],
            ret_min: 0,
            ret_max: 0,
        },
    );
    sigs.insert(
        "gfx_draw_gun".into(),
        FnSig {
            params: vec![("recoil".into(), 0, 10), ("kills".into(), 0, 100)],
            ret_min: 0,
            ret_max: 0,
        },
    );
    let mut global_consts: HashMap<String, (i128, i128)> = HashMap::new();
    let mut struct_contracts: HashMap<String, (i128, i128)> = HashMap::new();
    for st in &program.structs {
        c_out.push_str(&format!("struct {} {{\n", st.name));
        for f in &st.fields {
            c_out.push_str(&format!("    int32_t {};\n", f.name));
            struct_contracts.insert(
                format!("__struct_{}.{}", st.name, f.name),
                (f.min_val as i128, f.max_val as i128),
            );
        }
        c_out.push_str("};\n\n");
    }
    let no_arrays: HashMap<String, ArrayInfo> = HashMap::new();
    for c in &program.consts {
        match eval_expr_range(&c.value, &global_consts, &no_arrays, &sigs) {
            Ok((vmin, vmax)) => {
                if vmin < c.min_val as i128 || vmax > c.max_val as i128 {
                    eprintln!(
                        "\n[VETO | строка {}]: Константа {} (<{}..{}>) выходит за [{}..{}]!",
                        c.line, c.name, c.min_val, c.max_val, vmin, vmax
                    );
                    std::process::exit(1);
                }
                global_consts.insert(c.name.clone(), (vmin, vmax));
                c_out.push_str(&format!(
                    "static const int32_t {} = {};\n",
                    c.name,
                    expr_to_c(&c.value)
                ));
            }
            Err(err) => {
                eprintln!(
                    "\n[VETO | строка {}]: Ошибка в константе {}: {}",
                    c.line, c.name, err
                );
                std::process::exit(1);
            }
        }
    }
    for f in &program.functions {
        sigs.insert(
            f.name.clone(),
            FnSig {
                params: f
                    .params
                    .iter()
                    .map(|p| (p.name.clone(), p.min_val, p.max_val))
                    .collect(),
                ret_min: f.ret_min,
                ret_max: f.ret_max,
            },
        );
    }
    for func in &program.functions {
        let mut env = global_consts.clone();
        let mut declared = global_consts.clone();
        declared.extend(struct_contracts.clone());
        let mut arrays: HashMap<String, ArrayInfo> = HashMap::new();
        for p in &func.params {
            if !(p.min_val == i64::MIN && p.max_val == i64::MAX) {
                let bounds = (p.min_val as i128, p.max_val as i128);
                env.insert(p.name.clone(), bounds);
                declared.insert(p.name.clone(), bounds);
            }
        }
        if func.name == "main" {
            c_out.push_str("int main(void) {\n");
        } else {
            let params_c: StrList = func
                .params
                .iter()
                .map(|p| format!("int32_t {}", p.name))
                .collect();
            let params_str = if params_c.is_empty() {
                "void".to_string()
            } else {
                params_c.join(", ")
            };
            c_out.push_str(&format!("int32_t {}({}) {{\n", func.name, params_str));
        }
        emit_stmts(
            &func.body,
            func,
            &mut env,
            &mut declared,
            &mut arrays,
            &sigs,
            source,
            "    ",
            &mut c_out,
        );
        c_out.push_str("}\n\n");
    }
    c_out
}
