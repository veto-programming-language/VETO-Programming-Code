use std::fs::OpenOptions;
use std::io::{self, BufRead, Write};
use vetoc::inquisitor_model::{heuristic_judge, MiniModel};

fn main() {
    let model = MiniModel::load();
    println!("{}", if model.is_some() { "[test] модель загружена" } else { "[test] .bin не найден, работает эвристика" });
    println!("[test] формат: `1|текст` (ждём approve), `0|текст` (ждём veto)\n");

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
                if good { ok += 1; }
                println!("{} {} conf={:.2}  {}", if good { "OK  " } else { "FAIL" }, verdict, conf, text);
                if !good {
                    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open("manual.csv") {
                        let _ = writeln!(f, "{},\"{}\"", if e { 1 } else { 0 }, text.replace('"', "\"\""));
                        println!("      ^ записано в manual.csv");
                    }
                }
            }
            None => {
                println!("{} conf={:.2}", verdict, conf);
                print!("  вердикт верный? [y / n / Enter=пропуск] > ");
                io::stdout().flush().ok();
                let mut ans = String::new();
                stdin.lock().read_line(&mut ans).ok();
                if ans.trim() == "n" {
                    let correct = if approve { 0 } else { 1 }; // раз вердикт неверный, метка обратная
                    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open("manual.csv") {
                        let _ = writeln!(f, "{},\"{}\"", correct, text.replace('"', "\"\""));
                        println!("      ^ записано в manual.csv (метка {})", correct);
                    }
                }
            }
        }
    }
    if total > 0 {
        println!("\n[test] верно {}/{} ({:.0}%)", ok, total, 100.0 * ok as f32 / total as f32);
    }
}
