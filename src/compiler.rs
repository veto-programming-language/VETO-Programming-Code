pub use crate::ast::Program;
pub use crate::codegen::compile_program;
use crate::error::{ErrList, ErrorBag};
use crate::lexer::Lexer;
pub use crate::parser::Parser;

pub fn compile_source(
    source: &str,
    file: &str,
    base_dir: &str,
    is_kernel: bool,
) -> Result<String, ErrList> {
    let mut bag = ErrorBag::default();
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize();

    let mut parser = Parser::with_context(tokens, file, base_dir);
    let mut ast = parser.parse_main(&mut bag);
    if !bag.is_empty() {
        return Err(bag.errors);
    }

    let _ = compile_program(&ast, file, source, is_kernel, &mut bag);
    if !bag.is_empty() {
        return Err(bag.errors);
    }

    crate::optimizer::optimize_program(&mut ast);
    let mut opt_bag = ErrorBag::default();
    let c_code = compile_program(&ast, file, source, is_kernel, &mut opt_bag);
    Ok(c_code)
}

pub fn compile_to_asm(
    source: &str,
    file: &str,
    base_dir: &str,
    is_kernel: bool,
) -> Result<String, ErrList> {
    let mut bag = ErrorBag::default();
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize();

    let mut parser = Parser::with_context(tokens, file, base_dir);
    let mut ast = parser.parse_main(&mut bag);
    if !bag.is_empty() {
        return Err(bag.errors);
    }

    let _ = compile_program(&ast, file, source, is_kernel, &mut bag);
    if !bag.is_empty() {
        return Err(bag.errors);
    }

    crate::optimizer::optimize_program(&mut ast);
    Ok(crate::asm_backend::generate_x86_asm(&ast, is_kernel))
}

pub fn compile_test_runner(
    source: &str,
    file: &str,
    base_dir: &str,
) -> Result<(String, usize), ErrList> {
    let mut bag = ErrorBag::default();
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize();

    let mut parser = Parser::with_context(tokens, file, base_dir);
    let mut ast = parser.parse_main(&mut bag);
    if !bag.is_empty() {
        return Err(bag.errors);
    }

    let test_names: Vec<String> = ast
        .functions
        .iter()
        .filter(|f| f.attrs.iter().any(|a| a == "test"))
        .map(|f| f.name.clone())
        .collect();

    ast.functions.retain(|f| f.name != "main");

    let mut c_code = compile_program(&ast, file, source, false, &mut bag);
    if !bag.is_empty() {
        return Err(bag.errors);
    }

    c_code.push_str("#include <stdio.h>\nint main(void) {\n");
    c_code.push_str("    int passed = 0, failed = 0;\n");
    for tname in &test_names {
        c_code.push_str(&format!(
            "    if ({}() == 0) {{ printf(\"  [PASS] {}\\n\"); passed++; }} else {{ printf(\"  [FAIL] {}\\n\"); failed++; }}\n",
            tname, tname, tname
        ));
    }
    c_code.push_str(&format!(
        "    printf(\"[VETO TEST] summary: %d passed, %d failed (total {})\\n\", passed, failed);\n",
        test_names.len()
    ));
    c_code.push_str("    return failed == 0 ? 0 : 1;\n}\n");

    Ok((c_code, test_names.len()))
}

pub fn compile_to_asm_arch(
    source: &str,
    file: &str,
    base_dir: &str,
    is_kernel: bool,
    arch: &str,
) -> Result<String, ErrList> {
    let mut bag = ErrorBag::default();
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize();

    let mut parser = Parser::with_context(tokens, file, base_dir);
    let mut ast = parser.parse_main(&mut bag);
    if !bag.is_empty() {
        return Err(bag.errors);
    }

    let _ = compile_program(&ast, file, source, is_kernel, &mut bag);
    if !bag.is_empty() {
        return Err(bag.errors);
    }

    crate::optimizer::optimize_program(&mut ast);
    Ok(match arch {
        "x86_64" | "amd64" => crate::asm_backend::generate_x86_64_asm(&ast, is_kernel),
        "arm64" | "aarch64" => crate::asm_backend::generate_arm64_asm(&ast),
        _ => crate::asm_backend::generate_x86_asm(&ast, is_kernel),
    })
}

pub fn compile_project_manifest(manifest_toml: &str, base_dir: &str) -> Result<String, ErrList> {
    let mut entry = "main.vt".to_string();
    let mut is_kernel = false;
    for line in manifest_toml.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("entry") {
            if let Some((_, val)) = rest.split_once('=') {
                entry = val.trim().trim_matches('"').to_string();
            }
        }
        if let Some(rest) = t.strip_prefix("target") {
            if let Some((_, val)) = rest.split_once('=') {
                if val.contains("kernel") {
                    is_kernel = true;
                }
            }
        }
    }
    let full = std::path::Path::new(base_dir).join(&entry);
    let src = std::fs::read_to_string(&full)
        .unwrap_or_else(|_| "fn main() -> i32<0..0> { return 0; }".into());
    compile_source(&src, &entry, base_dir, is_kernel)
}
