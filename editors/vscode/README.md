# VETO Language Support for VS Code

Official syntax highlighting, bracket matching, and code snippets for **VETO 3.0** (`.vt`) — the zero-unsafe, formally verified systems programming language.

## Features

- **Full VETO 3.0 Syntax Highlighting**:
  - Dependent interval types (`i32<0..1999>`, `u8<0..254>`, `bool`, `usize`, `Option`, `Result`).
  - Hardware capability & verification attributes (`#[mmio]`, `#[port]`, `#[requires]`, `#[ensures]`, `#[terminates]`, `#[max_cycles]`, `#[linear]`, `#[irq_safe]`, `#[no_alloc]`, `#[lock_free]`).
  - Systems constructs (`bitstruct`, `arena`, `comptime`, `trait`, `mod`, `asm!`, `prove`).
- **Systems Programming Snippets**:
  - Quick templates for Ring-0 entry points, MMIO/Port drivers, typestate transitions, linear spinlock guards, bitfields, and verified inline assembly.

## Building & Installing `.vsix`

```bash
cd editors/vscode
npx --yes @vscode/vsce package
code --install-extension veto-lang-0.3.0.vsix
```
