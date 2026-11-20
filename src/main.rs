use std::env;
use std::fs;
use std::path::Path;
use std::process::{self, Command};
use vetoc::compiler::{
    compile_project_manifest, compile_source, compile_test_runner, compile_to_asm_arch,
};

fn print_help() {
    println!("vetoc 3.0.0 — Zero-Unsafe Systems Language & Static Range Verifier");
    println!();
    println!("USAGE:");
    println!("  vetoc <input.vt | Veto.toml> [OPTIONS]");
    println!();
    println!("OPTIONS:");
    println!("  -o <output>         Specify output binary or assembly path");
    println!("  --kernel            Compile in freestanding Ring-0 kernel mode (Multiboot, 16KB stack check)");
    println!("  --native-asm        Emit native assembly without GCC");
    println!("  --arch <target>     Target architecture: x86 (default), x86_64, arm64");
    println!("  --emit-c            Emit generated C code instead of compiling binary");
    println!("  --test              Compile and run #[test] suite");
    println!("  -h, --help          Show this help message");
}

fn find_c_compiler() -> &'static str {
    for cc in ["gcc", "clang", "cc"] {
        if Command::new(cc).arg("--version").output().is_ok() {
            return cc;
        }
    }
    "gcc"
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() || args.iter().any(|a| a == "-h" || a == "--help") {
        print_help();
        return;
    }

    let mut input_path = String::new();
    let mut output_path: Option<String> = None;
    let mut is_kernel = false;
    let mut native_asm = false;
    let mut emit_c = false;
    let mut run_tests = false;
    let mut arch = "x86".to_string();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-o" => {
                if i + 1 < args.len() {
                    output_path = Some(args[i + 1].clone());
                    i += 1;
                }
            }
            "--kernel" => is_kernel = true,
            "--native-asm" => native_asm = true,
            "--emit-c" => emit_c = true,
            "--test" => run_tests = true,
            "--arch" => {
                if i + 1 < args.len() {
                    arch = args[i + 1].clone();
                    i += 1;
                }
            }
            other if !other.starts_with('-') => {
                input_path = other.to_string();
            }
            unknown => {
                eprintln!("[vetoc] unknown option: {}", unknown);
                process::exit(1);
            }
        }
        i += 1;
    }

    if input_path.is_empty() {
        eprintln!("[vetoc] no input file specified");
        process::exit(1);
    }

    let path = Path::new(&input_path);
    let base_dir = path
        .parent()
        .and_then(|p| {
            if p.as_os_str().is_empty() {
                None
            } else {
                p.to_str()
            }
        })
        .unwrap_or(".");

    let source = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[vetoc] cannot read '{}': {}", input_path, e);
            process::exit(1);
        }
    };

    if input_path.ends_with("Veto.toml") {
        match compile_project_manifest(&source, base_dir) {
            Ok(c_code) => {
                let out_c = output_path.unwrap_or_else(|| "veto_out.c".into());
                fs::write(&out_c, c_code).expect("failed to write output");
                println!("[vetoc] manifest build succeeded -> {}", out_c);
                return;
            }
            Err(errs) => {
                for e in errs {
                    eprintln!("{}", e);
                }
                process::exit(1);
            }
        }
    }

    if run_tests {
        match compile_test_runner(&source, &input_path, base_dir) {
            Ok((c_code, count)) => {
                let tmp_c = env::temp_dir().join("veto_test_runner.c");
                let tmp_bin = env::temp_dir().join("veto_test_runner");
                fs::write(&tmp_c, c_code).expect("failed to write temp test runner");
                let cc = find_c_compiler();
                let st = Command::new(cc)
                    .arg(&tmp_c)
                    .arg("-o")
                    .arg(&tmp_bin)
                    .status()
                    .expect("failed to invoke C compiler");
                if !st.success() {
                    process::exit(1);
                }
                println!("[vetoc] running {} verified tests...", count);
                let run_st = Command::new(&tmp_bin)
                    .status()
                    .expect("failed to execute test runner");
                if !run_st.success() {
                    process::exit(1);
                }
                return;
            }
            Err(errs) => {
                for e in errs {
                    eprintln!("{}", e);
                }
                process::exit(1);
            }
        }
    }

    if native_asm {
        match compile_to_asm_arch(&source, &input_path, base_dir, is_kernel, &arch) {
            Ok(asm) => {
                let out_s = output_path.unwrap_or_else(|| "out.s".into());
                fs::write(&out_s, asm).expect("failed to write assembly output");
                println!("[vetoc] native {} assembly written -> {}", arch, out_s);
                return;
            }
            Err(errs) => {
                for e in errs {
                    eprintln!("{}", e);
                }
                process::exit(1);
            }
        }
    }

    match compile_source(&source, &input_path, base_dir, is_kernel) {
        Ok(c_code) => {
            if emit_c {
                let out_c = output_path.unwrap_or_else(|| "out.c".into());
                fs::write(&out_c, c_code).expect("failed to write C output");
                println!("[vetoc] C output written -> {}", out_c);
                return;
            }

            let tmp_c = env::temp_dir().join("veto_build.c");
            fs::write(&tmp_c, &c_code).expect("failed to write temp C file");

            let out_bin = output_path.unwrap_or_else(|| {
                if is_kernel {
                    "kernel.elf".into()
                } else {
                    "a.out".into()
                }
            });

            let cc = find_c_compiler();
            let mut cmd = Command::new(cc);
            cmd.arg(&tmp_c).arg("-O2").arg("-o").arg(&out_bin);

            if is_kernel {
                cmd.args(["-m32", "-ffreestanding", "-nostdlib", "-fno-pie"]);
            } else if c_code.contains("SDL.h") {
                cmd.arg("-lSDL2");
            }

            let status = cmd.status().expect("failed to invoke C compiler");
            if !status.success() {
                eprintln!("[vetoc] backend compilation failed");
                process::exit(1);
            }
            println!("[vetoc] verified build succeeded -> {}", out_bin);
        }
        Err(errs) => {
            for e in errs {
                eprintln!("{}", e);
            }
            process::exit(1);
        }
    }
}
