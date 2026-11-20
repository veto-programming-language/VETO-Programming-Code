#![allow(unreachable_code)]
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashMap;
use std::io::{self, Write};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::proofgen::{self, Outcome};

#[cfg(feature = "inquisitor")]
use crate::inquisitor_model::{heuristic_judge, MiniModel};

const WEEK_SECS: i64 = 604_800;

fn db_path() -> PathBuf {
    std::env::temp_dir().join("vetoc_inquisitor.db")
}

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
    pub fn new(reason: impl AsRef<str>, code_context: impl AsRef<str>) -> Self {
        let conn = Connection::open(db_path()).expect("failed to open Inquisitor database");
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
        .expect("failed to initialize Inquisitor tables");

        Self {
            conn,
            reason: reason.as_ref().to_string(),
            code_context: code_context.as_ref().to_string(),
            #[cfg(feature = "inquisitor")]
            model: MiniModel::load(),
            excuse: None,
        }
    }

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
            println!(
                "  I remember your previous excuse: \"{}\". Do not repeat it.",
                e
            );
        }
    }

    #[cfg(feature = "inquisitor")]
    fn local_judge(&self, proof: &str) -> (bool, f32) {
        match &self.model {
            Some(m) => m.judge(proof),
            None => heuristic_judge(proof),
        }
    }

    #[cfg(not(feature = "inquisitor"))]
    fn local_judge(&self, proof: &str) -> (bool, f32) {
        let ok = proof.len() >= 25 && proof.chars().any(|c| c.is_ascii_digit());
        (ok, if ok { 0.86 } else { 0.2 })
    }

    fn mock_reply(&self, proof: &str) -> &'static str {
        let p = proof.to_lowercase();

        if p.contains("idk") || p.contains("don't know") || p.contains("not sure") {
            return "\"I don't know\" is not a proof—it is a confession. \
                    A compiler does not guess. Prove the bounds or narrow your types.";
        }
        if p.contains("works") || p.contains("tested") || p.contains("my machine") {
            return "\"It works on my machine\" is a lottery, not mathematics. \
                    It will fault on the first edge case. Give me formal bounds.";
        }
        if p.contains("trust") || p.contains("promise") || p.contains("honestly") {
            return "I do not trust. I am a compiler. I run on axioms, not promises. \
                    Show me the interval invariants.";
        }
        if p.contains("maybe") || p.contains("probably") || p.contains("guess") {
            return "\"Maybe\" and \"probably\" belong in weather forecasts, not Ring-0 memory safety. \
                    Provide exact lower and upper bounds.";
        }

        if proof.len() < 20 {
            return "That is a headline, not a proof. State the variable ranges, \
                    the arithmetic step, and why the threshold holds.";
        }

        let has_digits = p.chars().any(|c| c.is_ascii_digit());
        let has_logic = p.contains("=>")
            || p.contains("therefore")
            || p.contains("implies")
            || p.contains("hence")
            || p.contains("since")
            || p.contains("<=")
            || p.contains(">=");
        if has_digits && !has_logic {
            return "You threw numbers at me without logical deduction. \
                    Show the transition from premises to the target invariant.";
        }
        if has_logic && !has_digits {
            return "Logic without bounds is empty. Prove the intervals are real—state the exact min and max.";
        }

        "Too weak. That reads closer to an excuse than a theorem. \
         Break it down step by step: variable ranges, operation bounds, and threshold comparison."
    }

    pub fn auto_prove(&mut self) -> Verdict {
        let env = proofgen::ranges_from_source(&self.code_context);
        self.auto_prove_with(&env)
    }

    pub fn auto_prove_with(&mut self, env: &HashMap<String, (i128, i128)>) -> Verdict {
        let rep = proofgen::prove(&self.reason, env);
        println!("\n[VETO // AI-INQUISITOR] Target: {}", self.reason);
        match rep.outcome {
            Outcome::Proven => {
                println!("  Proof (from type intervals):\n  {}", rep.text);
                println!("[APPROVE] Invariant proven, continuing build.");
                Verdict::Approve
            }
            Outcome::Refuted => {
                println!("  {}", rep.text);
                println!("[VETO] Invariant refuted by type bounds. Fix your code.");
                Verdict::Reject
            }
            Outcome::Unknown => {
                println!("  Unproven: {}", rep.text);
                if std::env::var("VETOC_ALLOW_MANUAL").as_deref() == Ok("1") {
                    println!("  VETOC_ALLOW_MANUAL=1 -> opening interactive trial.");
                    self.interrogate()
                } else {
                    println!("[VETO] Invariant may be violated. Narrow your type ranges or guard the condition.");
                    Verdict::Reject
                }
            }
        }
    }

    pub fn interrogate(&mut self) -> Verdict {
        if self.cached_approve() {
            println!("[VETO] invariant already cached as proven -> skipping trial");
            return Verdict::Approve;
        }

        self.recall_past_sins();
        println!("\n+--------------------------------------------------------------+");
        println!("|  [VETO // AI-INQUISITOR]                                     |");
        println!("|  Target: {:<52}|", self.reason);
        println!("+--------------------------------------------------------------+");
        println!("  VETO emits zero runtime bounds checks. A bug means a dead process.");
        println!("  Prove safety statically. You have 3 attempts.\n");

        let mut attempts = 0usize;
        while attempts < 3 {
            print!(
                "\n[attempt {}/3] Your proof (or 'fix' to abort) > ",
                attempts + 1
            );
            io::stdout().flush().ok();

            let mut input = String::new();
            if io::stdin().read_line(&mut input).is_err() {
                eprintln!("[VETO] stdin error -> reject");
                return Verdict::Reject;
            }
            let proof = input.trim().to_string();

            if proof.is_empty() || proof == "fix" {
                println!(
                    "\n[VETO] \"fix\" is the most honest word spoken in this court. \
                     Go fix your code; I will wait."
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
                self.cache_approve();
                println!(
                    "\n[APPROVE] Accepted. Model confidence: {:.0}%. Continuing compilation.",
                    conf * 100.0
                );
                return Verdict::Approve;
            }
            println!("\n[VETO] {}", self.mock_reply(&proof));
            if conf < 0.4 {
                println!(
                    "  (model is {:.0}% certain this is an excuse. Try harder.)",
                    (1.0 - conf) * 100.0
                );
            }
            attempts += 1;
            self.excuse = Some(proof);
            if attempts == 2 {
                println!("\n  Final attempt. Fail again and your excuse is logged for 7 days.");
            }
        }
        if let Some(last) = self.excuse.clone() {
            self.record_sin(&last);
        }
        println!("\n+--------------------------------------------------------------+");
        println!("|  [VETO] All 3 attempts exhausted.                            |");
        println!("|  Your last excuse has been recorded for 7 days.              |");
        println!("+--------------------------------------------------------------+");
        Verdict::Reject
    }
}
