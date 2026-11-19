// src/inquisitor_model.rs
//
// Крошечная нейросеть-классификатор: "доказательство vs отмазка".
// Архитектура: bag-of-hashed-words -> hidden(64, ReLU) -> out(2, softmax).
// Веса: src/inquisitor_model.bin, ~66 КБ. Инференс — микросекунды, RAM — сотни КБ.
//
// Обучение: src/bin/train_inquisitor.rs (отдельный бинарник, разово).

use std::fs;
use std::path::Path;

pub const BUCKETS: usize = 256;
pub const HIDDEN: usize = 64;
pub const MODEL_PATH: &str = "src/inquisitor_model.bin";

/// Размер файла модели в байтах. Используется для проверки целостности при load().
const MODEL_BYTES: usize = BUCKETS * HIDDEN * 4   // emb
                         + HIDDEN * 2 * 4          // w2
                         + HIDDEN * 4              // b1
                         + 2 * 4;                  // b2

pub struct MiniModel {
    pub emb: Vec<f32>,   // [BUCKETS][HIDDEN], row-major
    pub w2: Vec<f32>,    // [HIDDEN][2]
    pub b1: Vec<f32>,    // [HIDDEN]
    pub b2: [f32; 2],
}

impl MiniModel {
    /// Пытается загрузить веса с диска. None — если файла нет или размер битый.
    /// Никаких паник: вызывающий сам решает, что делать (обычно fallback на эвристику).
    pub fn load() -> Option<Self> {
        let data = fs::read(MODEL_PATH).ok()?;
        if data.len() != MODEL_BYTES {
            return None;
        }
        let mut off = 0usize;
        let mut take = |n: usize| -> Vec<f32> {
            let mut v = Vec::with_capacity(n);
            for _ in 0..n {
                let bytes = [data[off], data[off + 1], data[off + 2], data[off + 3]];
                v.push(f32::from_le_bytes(bytes));
                off += 4;
            }
            v
        };
        let emb = take(BUCKETS * HIDDEN);
        let w2 = take(HIDDEN * 2);
        let b1 = take(HIDDEN);
        let b2v = take(2);
        Some(MiniModel { emb, w2, b1, b2: [b2v[0], b2v[1]] })
    }

    /// Сохраняет веса в MODEL_PATH. Формат — плоский LE f32, без заголовков.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let mut out = Vec::with_capacity(MODEL_BYTES);
        for v in self.emb.iter().chain(self.w2.iter()).chain(self.b1.iter()) {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(&self.b2[0].to_le_bytes());
        out.extend_from_slice(&self.b2[1].to_le_bytes());
        fs::write(path, out)
    }

    /// Прямой проход. Возвращает два логита [p_veto, p_approve].
    pub fn forward(&self, text: &str) -> [f32; 2] {
        let mut hidden = self.b1.clone();
        let mut count = 0usize;
        for word in tokenize(text) {
            let bucket = hash_bucket(&word, BUCKETS);
            let row = &self.emb[bucket * HIDDEN..(bucket + 1) * HIDDEN];
            for i in 0..HIDDEN {
                hidden[i] += row[i];
            }
            count += 1;
        }
        if count > 0 {
            let inv = 1.0 / count as f32;
            for h in hidden.iter_mut() {
                *h *= inv;
            }
        }
        for h in hidden.iter_mut() {
            if *h < 0.0 {
                *h = 0.0;
            }
        }
        let mut out = [self.b2[0], self.b2[1]];
        for i in 0..HIDDEN {
            let h = hidden[i];
            out[0] += h * self.w2[i * 2];
            out[1] += h * self.w2[i * 2 + 1];
        }
        out
    }

    /// Возвращает (approve, confidence). confidence in [0.5, 1.0].
    pub fn judge(&self, text: &str) -> (bool, f32) {
        let logits = self.forward(text);
        let m = logits[0].max(logits[1]);
        let e0 = (logits[0] - m).exp();
        let e1 = (logits[1] - m).exp();
        let p1 = e1 / (e0 + e1);
        (p1 >= 0.5, p1.max(1.0 - p1))
    }
}

/// Токенизатор: буквы/цифры/`_` — в один токен (lowercase),
/// спецсимволы доказательств — отдельными токенами.
pub fn tokenize(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for ch in text.chars() {
        if ch.is_alphanumeric() || ch == '_' {
            cur.extend(ch.to_lowercase());
        } else {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            if "<=>⇒⊂∈∀∃[]()<>.,+*-/:".contains(ch) {
                out.push(ch.to_string());
            }
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// FNV-1a: быстрый нестабильный хэш в 256 бакетов.
pub fn hash_bucket(word: &str, buckets: usize) -> usize {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in word.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    (h as usize) % buckets
}

/// Эвристический fallback, если модель не обучена (нет .bin).
/// Не нейросеть — набор маркеров, но работает с первого запуска.
pub fn heuristic_judge(text: &str) -> (bool, f32) {
    let t = text.to_lowercase();

    let strong = [
        "=>", "следовательно", "следует", "пусть ", "для всех", "для любого",
        "∈", "⊂", "теорема", "доказано", "интервал", "range", "0<=", "0 <=",
        "мин", "макс", "min", "max", "q.e.d", "ч.т.д", "тогда", "отсюда",
    ];
    let weak = [
        "хз", "не знаю", "работает", "проверял", "честно", "поверь",
        "доверься", "норм", "ок", "вроде", "наверное", "точно работает",
        "я тестировал", "у меня работает", "должно быть",
    ];

    let digits = t.chars().filter(|c| c.is_ascii_digit()).count();
    let strong_hits = strong.iter().filter(|s| t.contains(*s)).count();
    let weak_hits = weak.iter().filter(|s| t.contains(*s)).count();

    let score = strong_hits as f32 * 1.0
        + digits.min(6) as f32 * 0.3
        - weak_hits as f32 * 1.2;
    let p = 1.0 / (1.0 + (-score).exp());
    (p >= 0.5, p.max(1.0 - p))
}