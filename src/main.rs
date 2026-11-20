use std::env;
use std::fs;
use std::path::Path;
use std::process::{Command, ExitCode};

use vetoc::compiler::{generate_c, Parser};
use vetoc::lexer::Lexer;

type StrVec = Vec<String>;

fn main() -> ExitCode {
    let args: StrVec = env::args().collect();

    let target_file = match args.get(1) {
        Some(f) => f,
        None => {
            eprintln!("Использование: vetoc <файл.vt> [--kernel]");
            return ExitCode::from(1);
        }
    };

    let is_kernel = args.iter().any(|a| a == "--kernel");

    let source = match fs::read_to_string(target_file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[VETO] не могу прочитать {}: {}", target_file, e);
            return ExitCode::from(1);
        }
    };

    // Определяем базовую директорию файла для работы модульных импортов `use "module.vt";`
    let base_dir = Path::new(target_file)
        .parent()
        .and_then(|p| p.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(".");

    // 1. Токенизация (с трекингом номеров строк)
    let mut lexer = Lexer::new(&source);
    let tokens = lexer.tokenize();

    // 2. Парсинг + рекурсивная загрузка модулей `use`
    let mut parser = Parser::with_base_dir(tokens, base_dir);
    let ast = parser.parse_main();

    // 3. Верификация диапазонов + генерация C-кода
    let c_code = generate_c(&ast, &source, is_kernel);

    let temp_dir = env::temp_dir();
    let temp_c = temp_dir.join("veto_out.c");
    if let Err(e) = fs::write(&temp_c, c_code) {
        eprintln!("[VETO] не могу записать {}: {}", temp_c.display(), e);
        return ExitCode::from(1);
    }

    if is_kernel {
        let linker_ld = temp_dir.join("veto_linker.ld");
        let ld_script = r#"ENTRY(_start)
SECTIONS {
    . = 1M;
    .text BLOCK(4K) : ALIGN(4K) {
        *(.multiboot)
        *(.text)
    }
    .rodata BLOCK(4K) : ALIGN(4K) { *(.rodata) }
    .data BLOCK(4K) : ALIGN(4K) { *(.data) }
    .bss BLOCK(4K) : ALIGN(4K) { *(COMMON) *(.bss) }
}
"#;
        let _ = fs::write(&linker_ld, ld_script);

        let gcc_result = Command::new("gcc")
            .args([
                "-m32",
                "-mno-sse",
                "-mno-sse2",
                "-mno-mmx",
                "-ffreestanding",
                "-nostdlib",
                "-fno-pie",
                "-no-pie",
                "-O2",
                "-T",
                linker_ld.to_str().unwrap(),
                temp_c.to_str().unwrap(),
                "-o",
                "veto_kernel.elf",
            ])
            .status();

        match gcc_result {
            Ok(status) if status.success() => {
                println!("[SUCCESS]: Ядро ОС собрано в ./veto_kernel.elf (0 проверок в рантайме)");
                println!("Запуск в эмуляторе: qemu-system-i386 -kernel veto_kernel.elf");
                ExitCode::SUCCESS
            }
            Ok(status) => {
                eprintln!("[VETO] gcc (kernel) завершился с кодом {:?}", status.code());
                ExitCode::from(1)
            }
            Err(e) => {
                eprintln!("[VETO] не могу запустить gcc: {}", e);
                ExitCode::from(1)
            }
        }
    } else {
        let out_bin = if cfg!(windows) {
            "veto_app.exe"
        } else {
            "veto_app"
        };

        let gcc_result = Command::new("gcc")
            .args([
                temp_c.to_str().unwrap(),
                "-O3",
                "-march=native",
                "-fno-stack-protector",
                "-lSDL2",
                "-o",
                out_bin,
            ])
            .status();

        match gcc_result {
            Ok(status) if status.success() => {
                println!("[SUCCESS]: собрано в ./{} (0 проверок в рантайме)", out_bin);
                ExitCode::SUCCESS
            }
            Ok(status) => {
                eprintln!("[VETO] gcc завершился с кодом {:?}", status.code());
                ExitCode::from(1)
            }
            Err(e) => {
                eprintln!("[VETO] не могу запустить gcc: {}", e);
                ExitCode::from(1)
            }
        }
    }
}
