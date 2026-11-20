use std::fs::OpenOptions;
use std::io::{self, BufRead, Write};
use vetoc::inquisitor_model::{heuristic_judge, MiniModel};

fn main() {
    let model = MiniModel::load();
    println!(
        "{}",
        if model.is_some() {
            "[test] model loaded"
        } else {
            "[test] .bin not found, using heuristic fallback"
        }
    );
    println!("[test] format: `1|text` (expect approve), `0|text` (expect veto)\n");

    let (mut total, mut ok) = (0usize, 0usize);
    let stdin = io::stdin();
    loop {
        print!("> ");
        io::stdout().flush().ok();
        let mut line = String::new();
        if stdin.lock().read_line(&mut line).unwrap_or(0) == 0 {
            break;
        }
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let (expected, text) = match line.split_once('|') {
            Some(("1", t)) => (Some(true), t.trim()),
            Some(("0", t)) => (Some(false), t.trim()),
            _ => (None, line),
        };
        let (approve, conf) = match &model {
            Some(m) => m.judge(text),
            None => heuristic_judge(text),
        };
        let verdict = if approve { "APPROVE" } else { "VETO   " };
        match expected {
            Some(e) => {
                total += 1;
                let good = e == approve;
                if good {
                    ok += 1;
                }
                println!(
                    "{} {} conf={:.2}  {}",
                    if good { "OK  " } else { "FAIL" },
                    verdict,
                    conf,
                    text
                );
                if !good {
                    if let Ok(mut f) = OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open("manual.csv")
                    {
                        let _ = writeln!(
                            f,
                            "{},\"{}\"",
                            if e { 1 } else { 0 },
                            text.replace('"', "\"\"")
                        );
                        println!("      ^ recorded to manual.csv");
                    }
                }
            }
            None => {
                println!("{} conf={:.2}", verdict, conf);
                print!("  verdict correct? [y / n / Enter=skip] > ");
                io::stdout().flush().ok();
                let mut ans = String::new();
                stdin.lock().read_line(&mut ans).ok();
                if ans.trim() == "n" {
                    let correct = if approve { 0 } else { 1 };
                    if let Ok(mut f) = OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open("manual.csv")
                    {
                        let _ = writeln!(f, "{},\"{}\"", correct, text.replace('"', "\"\""));
                        println!("      ^ recorded to manual.csv (label {})", correct);
                    }
                }
            }
        }
    }
    if total > 0 {
        println!(
            "\n[test] accurate {}/{} ({:.0}%)",
            ok,
            total,
            100.0 * ok as f32 / total as f32
        );
    }
}
