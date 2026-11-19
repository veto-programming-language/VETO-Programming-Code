// src/inquisitor.rs
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashMap;
use std::io::{self, Write};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::proofgen::{self, Outcome};

#[cfg(feature = "inquisitor")]
use crate::inquisitor_model::{heuristic_judge, MiniModel};

const WEEK_SECS: i64 = 604_800;
const DB_PATH: &str = "vetoc_inquisitor.db";

pub struct Inquisitor {
    conn: Connection,
    reason: String,
    code_context: String,
    #[cfg(feature = "inquisitor")]
    #[allow(dead_code)]
    model: Option<MiniModel>,
    excuse: Option<String>,
}

pub enum Verdict {
    Approve,
    Reject,
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl Inquisitor {
    // ───────────────────────── конструктор и хранилище ─────────────────────────

    pub fn new(reason: impl AsRef<str>, code_context: impl AsRef<str>) -> Self {
        let conn = Connection::open(DB_PATH).expect("не открылась БД инквизитора");
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS sins (
                 id INTEGER PRIMARY KEY,
                 key TEXT NOT NULL,
                 excuse TEXT NOT NULL,
                 ts INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS approvals (
                 key TEXT PRIMARY KEY,
                 ts INTEGER NOT NULL
             );",
        )
        .expect("не создались таблицы");

        Self {
            conn,
            reason: reason.as_ref().to_string(),
            code_context: code_context.as_ref().to_string(),
            #[cfg(feature = "inquisitor")]
            model: MiniModel::load(),
            excuse: None,
        }
    }

    /// Ключ участка кода: FNV-1a от (reason + контекст).
    fn key(&self) -> String {
        let mut h: u64 = 0xcbf29ce484222325;
        for b in self
            .reason
            .bytes()
            .chain(std::iter::once(b'\n'))
            .chain(self.code_context.bytes())
        {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        format!("{:016x}", h)
    }

    fn cached_approve(&self) -> bool {
        self.conn
            .query_row(
                "SELECT ts FROM approvals WHERE key = ?1 AND ts > ?2",
                params![self.key(), now_secs() - WEEK_SECS],
                |r| r.get::<_, i64>(0),
            )
            .optional()
            .ok()
            .flatten()
            .is_some()
    }

    fn cache_approve(&self) {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO approvals (key, ts) VALUES (?1, ?2)",
                params![self.key(), now_secs()],
            )
            .ok();
    }

    fn record_sin(&self, excuse: &str) {
        self.conn
            .execute(
                "INSERT INTO sins (key, excuse, ts) VALUES (?1, ?2, ?3)",
                params![self.key(), excuse, now_secs()],
            )
            .ok();
    }

    fn recall_past_sins(&self) {
        let past: Option<String> = self
            .conn
            .query_row(
                "SELECT excuse FROM sins WHERE key = ?1 AND ts > ?2 \
                 ORDER BY ts DESC LIMIT 1",
                params![self.key(), now_secs() - WEEK_SECS],
                |r| r.get(0),
            )
            .optional()
            .ok()
            .flatten();
        if let Some(e) = past {
            println!("  Я помню твою прошлую отмазку: «{}». Не повторяйся.", e);
        }
    }

    // ───────────────────────────── оценка аргумента ─────────────────────────────

    /// Локальная оценка. ВРЕМЕННАЯ эвристика (без MiniModel), чтобы всё работало.
    fn local_judge(&self, proof: &str) -> (bool, f32) {
        match &self.model {
            Some(m) => m.judge(proof),
            None => heuristic_judge(proof), // если src/inquisitor_model.bin не найден
        }
    }

    /// Саркастичные реплики на разные типы отмазок.
    fn mock_reply(&self, proof: &str) -> &'static str {
        let p = proof.to_lowercase();

        // Явные отмазки — жёстко
        if p.contains("хз") || p.contains("не знаю") || p.contains("не уверен") {
            return "«Не знаю» — это не доказательство, это признание в некомпетентности. \
                    Компилятор не гадает. Докажи или иди читай учебник по дискретке.";
        }
        if p.contains("работает") || p.contains("проверял") || p.contains("тестировал")
        {
            return "«Работает у меня на машине» — это не математика, это лотерея. \
                    У меня на машине твой код упадёт при первом же edge case. Докажи формально.";
        }
        if p.contains("честно") || p.contains("поверь") || p.contains("доверься")
        {
            return "Я не верю. Я — компилятор. У меня нет веры, у меня есть только аксиомы. \
                    Дай мне аксиомы, а не слово чести.";
        }
        if p.contains("вроде") || p.contains("наверное") || p.contains("кажется")
        {
            return "«Вроде» и «наверное» — это язык для обсуждения погоды, не безопасности памяти. \
                    Мне нужна уверенность, а не догадки.";
        }

        // Слишком короткие аргументы
        if proof.len() < 20 {
            return "Это не доказательство, это заголовок. Разверни мысль, покажи границы, \
                    покажи переход. Три слова — это не аргумент.";
        }

        // Есть цифры, но нет логики — придираемся
        let has_digits = p.chars().any(|c| c.is_ascii_digit());
        let has_logic = p.contains("=>")
            || p.contains("следовательно")
            || p.contains("следует")
            || p.contains("тогда")
            || p.contains("отсюда")
            || p.contains("∈")
            || p.contains("∀")
            || p.contains("⊂");
        if has_digits && !has_logic {
            return "Ты кинул числа, но не связал их логикой. Числа без связки — это набор \
                    цифр, а не доказательство. Где переход от посылки к следствию?";
        }
        if has_logic && !has_digits {
            return "Логика есть, но где границы? Докажи, что твои интервалы реальны, \
                    а не выдуманы. Покажи конкретные min и max.";
        }

        // Общий сарказм для всего остального
        "Слабовато. Модель говорит, что это ближе к отмазке, чем к теореме. \
         Переформулируй: покажи границы переменных, покажи операцию, покажи результат, \
         покажи сравнение с порогом. По пунктам, без воды."
    }

    // ─────────────────────────────── главный суд ────────────────────────────────

    /// Суд без человека: границы берутся из исходника по шаблону `имя: тип<lo..hi>`.
    pub fn auto_prove(&mut self) -> Verdict {
        let env = proofgen::ranges_from_source(&self.code_context);
        self.auto_prove_with(&env)
    }

    /// То же, но границы переменных передаёт компилятор (из разобранных `let`).
    pub fn auto_prove_with(&mut self, env: &HashMap<String, (i128, i128)>) -> Verdict {
        let rep = proofgen::prove(&self.reason, env);
        println!("\n[VETO // AI-INQUISITOR] Участок: {}", self.reason);
        match rep.outcome {
            Outcome::Proven => {
                println!("  Доказательство (из типов):\n  {}", rep.text);
                println!("[APPROVE] Условие доказано, сборка продолжается.");
                Verdict::Approve
            }
            Outcome::Refuted => {
                println!("  {}", rep.text);
                println!("[VETO] Условие ложно по типам. Исправь код.");
                Verdict::Reject
            }
            Outcome::Unknown => {
                println!("  Не доказано: {}", rep.text);
                if std::env::var("VETOC_ALLOW_MANUAL").as_deref() == Ok("1") {
                    println!("  VETOC_ALLOW_MANUAL=1 → открываю суд с человеком.");
                    self.interrogate()
                } else {
                    println!("[VETO] Условие может нарушиться. Сузь типы или измени условие.");
                    Verdict::Reject
                }
            }
        }
    }

    pub fn interrogate(&mut self) -> Verdict {
        if self.cached_approve() {
            println!("[VETO] условие уже доказано (кэш) → пропуск суда");
            return Verdict::Approve;
        }

        self.recall_past_sins();
        println!("\n╔══════════════════════════════════════════════════════════════╗");
        println!("║  [VETO // AI-INQUISITOR]                                     ║");
        println!("║  Участок: {:<50}║", self.reason);
        println!("╚══════════════════════════════════════════════════════════════╝");
        println!("  В Veto нет рантайм-проверок. Ошибка = мёртвый процесс.");
        println!("  Докажи, что здесь её нет. У тебя 3 попытки.\n");

        let mut attempts = 0usize;
        while attempts < 3 {
            print!(
                "\n[попытка {}/3] Твой аргумент (или 'fix' для отмены) > ",
                attempts + 1
            );
            io::stdout().flush().ok();

            let mut input = String::new();
            if io::stdin().read_line(&mut input).is_err() {
                eprintln!("[VETO] stdin сломался → отказ");
                return Verdict::Reject;
            }
            let proof = input.trim().to_string();

            if proof.is_empty() || proof == "fix" {
                println!(
                    "\n[VETO] «fix» — единственное честное слово за весь суд. \
                         Иди переписывай код, я подожду."
                );
                return Verdict::Reject;
            }

            let (local_ok, conf) = self.local_judge(&proof);
            println!(
                "  [local] verdict={} conf={:.2}",
                if local_ok { "APPROVE" } else { "VETO" },
                conf
            );

            if local_ok && conf >= 0.85 {
                self.record_sin(&proof);
                self.cache_approve();
                println!(
                    "\n[APPROVE] Ладно. На этот раз поверил. \
                         Модель уверена на {:.0}%. Сборка продолжается.",
                    conf * 100.0
                );
                return Verdict::Approve;
            }

            // Отказ — с характером
            println!("\n[VETO] {}", self.mock_reply(&proof));
            if conf < 0.4 {
                println!(
                    "  (модель уверена на {:.0}%, что это отмазка. Ты не стараешься.)",
                    (1.0 - conf) * 100.0
                );
            }

            attempts += 1;
            self.excuse = Some(proof);

            if attempts == 2 {
                println!("\n  Последняя попытка. Если снова провал — запомню на неделю.");
            }
        }

        if let Some(last) = self.excuse.clone() {
            self.record_sin(&last);
        }
        println!("\n╔══════════════════════════════════════════════════════════════╗");
        println!("║  [VETO] Три попытки исчерпаны.                               ║");
        println!("║  Твоя последняя фраза записана в память на 7 дней.           ║");
        println!("║  В следующий раз я её припомню.                              ║");
        println!("╚══════════════════════════════════════════════════════════════╝");
        Verdict::Reject
    }
}
