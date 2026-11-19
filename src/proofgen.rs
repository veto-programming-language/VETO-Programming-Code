// src/proofgen.rs
use std::collections::HashMap;

pub type Range = (i128, i128);

#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Proven,
    Refuted,
    Unknown,
}

pub struct Report {
    pub outcome: Outcome,
    pub text: String,
}

enum Expr {
    Num(i128),
    Var(String),
    Bin(Box<Expr>, char, Box<Expr>),
}

fn show(e: &Expr) -> String {
    fn child(e: &Expr) -> String {
        match e {
            Expr::Bin(..) => format!("({})", show(e)),
            _ => show(e),
        }
    }
    match e {
        Expr::Num(n) => n.to_string(),
        Expr::Var(v) => v.clone(),
        Expr::Bin(a, op, b) => format!("{} {} {}", child(a), op, child(b)),
    }
}

fn vars(e: &Expr, out: &mut Vec<String>) {
    match e {
        Expr::Var(v) => {
            if !out.contains(v) {
                out.push(v.clone());
            }
        }
        Expr::Bin(a, _, b) => {
            vars(a, out);
            vars(b, out);
        }
        Expr::Num(_) => {}
    }
}

// ───────────── лексер и парсер ─────────────

fn lex(s: &str) -> Result<Vec<String>, String> {
    let cs: Vec<char> = s.chars().collect();
    let mut i = 0;
    let mut out: Vec<String> = Vec::new();
    while i < cs.len() {
        let c = cs[i];
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() {
            let st = i;
            while i < cs.len() && (cs[i].is_ascii_digit() || cs[i] == '_') {
                i += 1;
            }
            out.push(cs[st..i].iter().filter(|c| **c != '_').collect::<String>());
        } else if c.is_alphabetic() || c == '_' {
            let st = i;
            while i < cs.len() && (cs[i].is_alphanumeric() || cs[i] == '_' || cs[i] == '.') {
                i += 1;
            }
            out.push(cs[st..i].iter().collect::<String>());
        } else if "<>=!".contains(c) {
            if i + 1 < cs.len() && cs[i + 1] == '=' {
                out.push(format!("{}=", c));
                i += 2;
            } else {
                out.push(c.to_string());
                i += 1;
            }
        } else if "+-*()".contains(c) {
            out.push(c.to_string());
            i += 1;
        } else {
            return Err(format!("неизвестный символ '{}'", c));
        }
    }
    Ok(out)
}

struct P {
    t: Vec<String>,
    i: usize,
}

impl P {
    fn peek(&self) -> Option<&str> {
        self.t.get(self.i).map(|s| s.as_str())
    }
    fn next(&mut self) -> Option<String> {
        let r = self.t.get(self.i).cloned();
        self.i += 1;
        r
    }
    fn expr(&mut self) -> Result<Expr, String> {
        let mut l = self.term()?;
        while let Some(op) = self.peek() {
            let c = match op {
                "+" => '+',
                "-" => '-',
                _ => break,
            };
            self.i += 1;
            let r = self.term()?;
            l = Expr::Bin(Box::new(l), c, Box::new(r));
        }
        Ok(l)
    }
    fn term(&mut self) -> Result<Expr, String> {
        let mut l = self.atom()?;
        while self.peek() == Some("*") {
            self.i += 1;
            let r = self.atom()?;
            l = Expr::Bin(Box::new(l), '*', Box::new(r));
        }
        Ok(l)
    }
    fn atom(&mut self) -> Result<Expr, String> {
        match self.next() {
            Some(t) if t == "(" => {
                let e = self.expr()?;
                if self.next().as_deref() != Some(")") {
                    return Err("нет закрывающей ')'".into());
                }
                Ok(e)
            }
            Some(t) if t == "-" => match self.atom()? {
                Expr::Num(n) => Ok(Expr::Num(-n)),
                e => Ok(Expr::Bin(Box::new(Expr::Num(0)), '-', Box::new(e))),
            },
            Some(t) if t.chars().next().map_or(false, |c| c.is_ascii_digit()) => {
                t.parse::<i128>().map(Expr::Num).map_err(|e| e.to_string())
            }
            Some(t) if t.chars().next().map_or(false, |c| c.is_alphabetic() || c == '_') => {
                Ok(Expr::Var(t))
            }
            Some(t) => Err(format!("неожиданный токен '{}'", t)),
            None => Err("выражение оборвано".into()),
        }
    }
}

fn parse(cond: &str) -> Result<(Expr, String, Expr), String> {
    let mut p = P { t: lex(cond)?, i: 0 };
    let lhs = p.expr()?;
    let op = p.next().ok_or("нет оператора сравнения")?;
    if !["<", "<=", ">", ">=", "==", "!="].contains(&op.as_str()) {
        return Err(format!("ожидался оператор сравнения, найдено '{}'", op));
    }
    let rhs = p.expr()?;
    if p.i < p.t.len() {
        return Err("лишние символы после условия".into());
    }
    Ok((lhs, op, rhs))
}

// ───────────── интервальная арифметика ─────────────

fn eval(e: &Expr, env: &HashMap<String, Range>, steps: &mut Vec<String>) -> Result<Range, String> {
    match e {
        Expr::Num(n) => Ok((*n, *n)),
        Expr::Var(v) => env.get(v).copied().ok_or_else(|| {
            format!("для «{}» не объявлены границы (нужен тип вида i32<lo..hi>)", v)
        }),
        Expr::Bin(a, op, b) => {
            let (l1, h1) = eval(a, env, steps)?;
            let (l2, h2) = eval(b, env, steps)?;
            let r = match op {
                '+' => (l1.saturating_add(l2), h1.saturating_add(h2)),
                '-' => (l1.saturating_sub(h2), h1.saturating_sub(l2)),
                _ => {
                    let c = [
                        l1.saturating_mul(l2),
                        l1.saturating_mul(h2),
                        h1.saturating_mul(l2),
                        h1.saturating_mul(h2),
                    ];
                    (*c.iter().min().unwrap(), *c.iter().max().unwrap())
                }
            };
            steps.push(format!("{} ∈ [{}..{}]", show(e), r.0, r.1));
            Ok(r)
        }
    }
}

/// Границы переменных из исходника: ищем `имя: тип<lo..hi>`.
pub fn ranges_from_source(src: &str) -> HashMap<String, Range> {
    let mut m = HashMap::new();
    for (idx, _) in src.match_indices(':') {
        let before = src[..idx].trim_end();
        let name: String = before
            .chars()
            .rev()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        if name.is_empty() {
            continue;
        }
        let after = src[idx + 1..].trim_start();
        let after = after.trim_start_matches(|c: char| c.is_alphanumeric() || c == '_');
        let Some(rest) = after.strip_prefix('<') else { continue };
        let Some(end) = rest.find('>') else { continue };
        if let Some((a, b)) = rest[..end].split_once("..") {
            if let (Ok(lo), Ok(hi)) = (a.trim().parse::<i128>(), b.trim().parse::<i128>()) {
                m.insert(name, (lo, hi));
            }
        }
    }
    m
}

/// Главная функция: из условия из `prove(...)` и границ строит доказательство.
pub fn prove(cond: &str, env: &HashMap<String, Range>) -> Report {
    let unknown = |why: String| Report { outcome: Outcome::Unknown, text: why };

    let (lhs, op, rhs) = match parse(cond) {
        Ok(x) => x,
        Err(e) => return unknown(format!("не смог разобрать условие: {}", e)),
    };
    let mut steps = Vec::new();
    let (l1, h1) = match eval(&lhs, env, &mut steps) {
        Ok(r) => r,
        Err(e) => return unknown(e),
    };
    let (l2, h2) = match eval(&rhs, env, &mut steps) {
        Ok(r) => r,
        Err(e) => return unknown(e),
    };

    let (proven, refuted) = match op.as_str() {
        "<" => (h1 < l2, l1 >= h2),
        "<=" => (h1 <= l2, l1 > h2),
        ">" => (l1 > h2, h1 <= l2),
        ">=" => (l1 >= h2, h1 < l2),
        "==" => (l1 == h1 && l2 == h2 && l1 == l2, h1 < l2 || h2 < l1),
        _ => (h1 < l2 || h2 < l1, l1 == h1 && l2 == h2 && l1 == l2), // "!="
    };

    let mut names = Vec::new();
    vars(&lhs, &mut names);
    vars(&rhs, &mut names);
    let given: Vec<String> = names
        .iter()
        .filter_map(|n| env.get(n).map(|(lo, hi)| format!("{} ∈ [{}..{}]", n, lo, hi)))
        .collect();
    let head = if given.is_empty() {
        String::new()
    } else {
        format!("пусть {}; ", given.join(", "))
    };
    let chain = if steps.is_empty() { String::new() } else { format!("тогда {}; ", steps.join("; ")) };
    let bounds = format!("левая часть ∈ [{}..{}], правая ∈ [{}..{}]", l1, h1, l2, h2);

    if proven {
        let why = match op.as_str() {
            "<" => format!("наибольшее значение {} меньше {}", h1, l2),
            "<=" => format!("наибольшее значение {} не больше {}", h1, l2),
            ">" => format!("наименьшее значение {} больше {}", l1, h2),
            ">=" => format!("наименьшее значение {} не меньше {}", l1, h2),
            _ => "интервалы согласованы".to_string(),
        };
        Report {
            outcome: Outcome::Proven,
            text: format!("{head}{chain}{bounds}; {why}, следовательно условие `{cond}` доказано"),
        }
    } else if refuted {
        Report {
            outcome: Outcome::Refuted,
            text: format!("{head}{chain}{bounds}; условие `{cond}` не выполняется ни при каких допустимых значениях"),
        }
    } else {
        Report {
            outcome: Outcome::Unknown,
            text: format!("{head}{chain}{bounds}; интервалы пересекаются, условие `{cond}` может нарушиться (худший случай: левая часть {})", h1),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speed_boost() {
        let src = "let speed: i32<0..100> = 40; let boost: i32<0..50> = 25;";
        let env = ranges_from_source(src);
        assert_eq!(prove("speed + boost < 200", &env).outcome, Outcome::Proven);
        assert_eq!(prove("speed + boost < 100", &env).outcome, Outcome::Unknown);
        assert_eq!(prove("speed + boost < 0", &env).outcome, Outcome::Refuted);
    }
}