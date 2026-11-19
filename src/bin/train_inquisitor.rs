// src/bin/train_inquisitor.rs
//
// Отдельный бинарник для обучения MiniModel. В релиз vetoc не входит.
// Использование:
//     cargo run --release --bin train_inquisitor
// Ожидает train.csv в CWD формата: label,text
//     label = 1 (доказательство) или 0 (отмазка)
//     text  = строка в кавычках (запятые внутри допустимы)
// Пишет src/inquisitor_model.bin.

use std::fs;
use std::path::Path;
use std::time::Instant;

use vetoc::inquisitor_model::{hash_bucket, tokenize, MiniModel, BUCKETS, HIDDEN};

fn main() {
    let csv = fs::read_to_string("train.csv")
        .expect("[train] не могу открыть train.csv — положи его в CWD");

    let mut data: Vec<(u8, Vec<String>)> = Vec::new();
    for (ln, line) in csv.lines().enumerate() {
        if ln == 0 && line.trim_start().starts_with("label") {
            continue; // шапка
        }
        if line.trim().is_empty() {
            continue;
        }
        // split_once(',') — берём первую запятую. Если у тебя в тексте есть запятые,
        // оборачивай text в кавычки и парсь так: label,"text, с запятыми"
        let (lbl, raw) = match line.split_once(',') {
            Some(x) => x,
            None => continue,
        };
        let label: u8 = lbl.trim().parse().unwrap_or(0);
        // срезаем обрамляющие кавычки, если есть
        let text = raw.trim().trim_matches('"');
        if text.is_empty() {
            continue;
        }
        data.push((label, tokenize(text)));
    }

    if data.is_empty() {
        eprintln!("[train] датасет пустой — проверь train.csv");
        return;
    }
    let pos = data.iter().filter(|(l, _)| *l == 1).count();
    println!("[train] samples={} (approve={}, veto={})", data.len(), pos, data.len() - pos);

    // Инициализация весов: маленькие детерминированные числа.
    // Не рандом — чтобы результат сборки был воспроизводим у тебя и у друга.
    let mut model = MiniModel {
        emb: (0..BUCKETS * HIDDEN)
            .map(|i| ((i as f32) * 0.013).sin() * 0.1)
            .collect(),
        w2: (0..HIDDEN * 2)
            .map(|i| ((i as f32) * 0.07).cos() * 0.1)
            .collect(),
        b1: vec![0.0; HIDDEN],
        b2: [0.0, 0.0],
    };

    let lr = 0.05f32;
    let epochs = 200usize;
    let start = Instant::now();

    for epoch in 0..epochs {
        let mut loss_sum = 0.0f32;
        for (label, tokens) in &data {
            // ---- forward ----
            let mut hidden = model.b1.clone();
            let mut used: Vec<usize> = Vec::with_capacity(tokens.len());
            for w in tokens {
                let b = hash_bucket(w, BUCKETS);
                used.push(b);
                for i in 0..HIDDEN {
                    hidden[i] += model.emb[b * HIDDEN + i];
                }
            }
            let inv = if used.is_empty() { 0.0 } else { 1.0 / used.len() as f32 };
            for h in hidden.iter_mut() {
                *h *= inv;
            }
            let pre_relu = hidden.clone();
            for h in hidden.iter_mut() {
                if *h < 0.0 {
                    *h = 0.0;
                }
            }

            let mut logits = [model.b2[0], model.b2[1]];
            for i in 0..HIDDEN {
                logits[0] += hidden[i] * model.w2[i * 2];
                logits[1] += hidden[i] * model.w2[i * 2 + 1];
            }
            let m = logits[0].max(logits[1]);
            let e0 = (logits[0] - m).exp();
            let e1 = (logits[1] - m).exp();
            let s = e0 + e1;
            let p = [e0 / s, e1 / s];
            let y = *label as usize;
            loss_sum -= p[y].max(1e-6).ln();

            // ---- backward (softmax-CE) ----
            let mut d_out = [p[0], p[1]];
            d_out[y] -= 1.0;

            let mut d_hidden = vec![0.0f32; HIDDEN];
            for i in 0..HIDDEN {
                for k in 0..2 {
                    let g = d_out[k] * hidden[i];
                    model.w2[i * 2 + k] -= lr * g;
                    d_hidden[i] += d_out[k] * model.w2[i * 2 + k];
                }
            }
            for k in 0..2 {
                model.b2[k] -= lr * d_out[k];
            }
            // ReLU mask
            for i in 0..HIDDEN {
                if pre_relu[i] <= 0.0 {
                    d_hidden[i] = 0.0;
                }
            }
            // emb: усредняем градиент по всем использованным бакетам
            if !used.is_empty() {
                let scale = lr * inv;
                for b in &used {
                    for i in 0..HIDDEN {
                        model.emb[b * HIDDEN + i] -= scale * d_hidden[i];
                    }
                }
            }
            for i in 0..HIDDEN {
                model.b1[i] -= lr * d_hidden[i] * inv;
            }
        }

        if epoch % 20 == 0 || epoch == epochs - 1 {
            println!(
                "[train] epoch {:>3}  loss {:.4}  ({:.1}s)",
                epoch,
                loss_sum / data.len() as f32,
                start.elapsed().as_secs_f32()
            );
        }
    }

    model
        .save(Path::new("src/inquisitor_model.bin"))
        .expect("[train] не могу записать src/inquisitor_model.bin");
    println!("[train] saved -> src/inquisitor_model.bin ({} байт)", file_size());
}

fn file_size() -> u64 {
    fs::metadata("src/inquisitor_model.bin").map(|m| m.len()).unwrap_or(0)
}