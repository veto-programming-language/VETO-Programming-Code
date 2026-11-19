// src/main.rs

use std::env;
use std::fs;
use std::process::{Command, ExitCode};

// main.rs — это отдельный бинарный крейт. Он видит библиотеку `vetoc`
// через `use vetoc::...`. Сами модули (lexer, compiler, inquisitor,
// inquisitor_model) объявлены в src/lib.rs.
use vetoc::compiler::{generate_c, Parser};
use vetoc::lexer::Lexer;

/// Точка входа. Возвращаем ExitCode, чтобы Rust корректно освободил ресурсы.
fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("Использование: vetoc <файл.vt>");
        return ExitCode::from(1);
    }

    let source = match fs::read_to_string(&args[1]) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[VETO] не могу прочитать {}: {}", args[1], e);
            return ExitCode::from(1);
        }
    };

    // 1. Токенизация
    let mut lexer = Lexer::new(&source);
    let tokens = lexer.tokenize();

    // 2. Парсинг + проверка диапазонов
    let mut parser = Parser::new(tokens);
    let ast = parser.parse_main();

    // 3. Генерация C-кода (внутри — вызов Инквизитора на prove, под фичей)
    let c_code = generate_c(&ast, &source);

    let temp_c = "/tmp/veto_out.c";
    if let Err(e) = fs::write(temp_c, c_code) {
        eprintln!("[VETO] не могу записать {}: {}", temp_c, e);
        return ExitCode::from(1);
    }

    // 4. Вызов gcc
    let gcc_result = Command::new("gcc")
        .args([
            temp_c,
            "-O3",
            "-march=native",
            "-fno-stack-protector",
            "-o",
            "veto_app",
        ])
        .status();

    match gcc_result {
        Ok(status) if status.success() => {
            println!("[SUCCESS]: собрано в ./veto_app (0 проверок в рантайме)");
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