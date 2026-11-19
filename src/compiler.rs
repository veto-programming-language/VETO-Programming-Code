use crate::lexer::Token;

#[derive(Debug)]
pub enum Expr {
    Number(i64),
    Var(String),
    BinOp(Box<Expr>, String, Box<Expr>),
}

#[derive(Debug)]
pub enum Stmt {
    Let {
        name: String,
        min_val: i64,
        max_val: i64,
        value: Expr,
    },
    Prove(String),
    Return(Expr),
}

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> &Token {
        self.tokens.get(self.pos).unwrap_or(&Token::EOF)
    }

    fn advance(&mut self) -> Token {
        let tok = self.peek().clone();
        self.pos += 1;
        tok
    }

    // Парсим функцию main()
    pub fn parse_main(&mut self) -> Vec<Stmt> {
        let mut body = Vec::new();
        // Пропускаем заголовок fn main() -> i32<...> {
        while *self.peek() != Token::LBrace && *self.peek() != Token::EOF {
            self.advance();
        }
        self.advance(); // Съедаем '{'

        while *self.peek() != Token::RBrace && *self.peek() != Token::EOF {
            match self.peek() {
                Token::Let => body.push(self.parse_let()),
                Token::Prove => body.push(self.parse_prove()),
                Token::Return => body.push(self.parse_return()),
                _ => {
                    self.advance();
                }
            }
        }
        body
    }

    /// Число с необязательным минусом (для границ вида <-5..5>).
    fn parse_signed(&mut self) -> Option<i64> {
        let neg = if *self.peek() == Token::Minus {
            self.advance();
            true
        } else {
            false
        };
        match self.advance() {
            Token::Number(n) => Some(if neg { -n } else { n }),
            _ => None,
        }
    }

    fn parse_let(&mut self) -> Stmt {
        self.advance(); // 'let'
        if *self.peek() == Token::Mut {
            self.advance(); // опционально 'mut'
        }

        let name = match self.advance() {
            Token::Ident(n) => n,
            _ => panic!("Ожидалось имя переменной после let"),
        };

        self.advance(); // ':'
        self.advance(); // тип (например 'i32')

        // Парсим строгий диапазон <min..max>
        let mut min_val = i64::MIN;
        let mut max_val = i64::MAX;
        if *self.peek() == Token::Less {
            self.advance(); // '<'
            if let Some(min) = self.parse_signed() {
                min_val = min;
            }
            self.advance(); // '..'
            if let Some(max) = self.parse_signed() {
                max_val = max;
            }
            self.advance(); // '>'
        }

        self.advance(); // '='
        let value = self.parse_expr();
        if *self.peek() == Token::Semicolon {
            self.advance();
        }

        Stmt::Let { name, min_val, max_val, value }
    }

    fn parse_prove(&mut self) -> Stmt {
        self.advance(); // 'prove'
        self.advance(); // '('

        let mut parts: Vec<String> = Vec::new();
        let mut depth = 0usize;
        while *self.peek() != Token::EOF {
            match self.peek() {
                Token::RParen if depth == 0 => break,
                Token::LParen => depth += 1,
                Token::RParen => depth -= 1,
                _ => {}
            }
            let token = self.advance();
            let mut s = token_to_source(&token);
            // Лексер не знает «<=», «>=», «==»: склеиваем символ с идущим следом '='
            if matches!(token, Token::Less | Token::Greater | Token::Assign)
                && *self.peek() == Token::Assign
            {
                self.advance();
                s.push('=');
            }
            parts.push(s);
        }
        self.advance(); // ')'
        if *self.peek() == Token::Semicolon {
            self.advance();
        }
        Stmt::Prove(parts.join(" "))
    }

    fn parse_return(&mut self) -> Stmt {
        self.advance(); // 'return'
        let expr = self.parse_expr();
        if *self.peek() == Token::Semicolon {
            self.advance();
        }
        Stmt::Return(expr)
    }

    // ───────── выражения: + и - (слева направо), * сильнее, скобки, унарный минус ─────────

    fn parse_expr(&mut self) -> Expr {
        let mut left = self.parse_term();
        loop {
            let op = match self.peek() {
                Token::Plus => "+",
                Token::Minus => "-",
                _ => break,
            };
            self.advance();
            let right = self.parse_term();
            left = Expr::BinOp(Box::new(left), op.to_string(), Box::new(right));
        }
        left
    }

    fn parse_term(&mut self) -> Expr {
        let mut left = self.parse_atom();
        while *self.peek() == Token::Star {
            self.advance();
            let right = self.parse_atom();
            left = Expr::BinOp(Box::new(left), "*".to_string(), Box::new(right));
        }
        left
    }

    fn parse_atom(&mut self) -> Expr {
        match self.advance() {
            Token::Number(n) => Expr::Number(n),
            Token::Ident(v) => Expr::Var(v),
            Token::LParen => {
                let e = self.parse_expr();
                if *self.peek() == Token::RParen {
                    self.advance();
                }
                e
            }
            Token::Minus => match self.parse_atom() {
                Expr::Number(n) => Expr::Number(-n),
                e => Expr::BinOp(Box::new(Expr::Number(0)), "-".to_string(), Box::new(e)),
            },
            other => panic!("Ожидалось выражение, найдено {:?}", other),
        }
    }
}

// Генератор сверхбыстрого C-кода без проверок
pub fn generate_c(stmts: &[Stmt], source: &str) -> String {
    let _ = source;
    let mut c_out = String::from("#include <stdint.h>\nint main(void) {\n");

    // Границы переменных, объявленных выше по коду: на них опирается prove(...)
    let mut env: std::collections::HashMap<String, (i128, i128)> =
        std::collections::HashMap::new();

    for stmt in stmts {
        match stmt {
            Stmt::Let { name, min_val, max_val, value } => {
                // Базовая проверка констант на этапе компиляции!
                if let Expr::Number(n) = value {
                    if *n < *min_val || *n > *max_val {
                        eprintln!(
                            "\n[VETO]: Ошибка диапазона! Переменная `{}` объявлена как <{}..{}>, но ты суешь туда `{}`!",
                            name, min_val, max_val, n
                        );
                        std::process::exit(1);
                    }
                }
                // Переменные без явных границ в env не кладём:
                // тогда prove честно скажет, что границ нет.
                if !(*min_val == i64::MIN && *max_val == i64::MAX) {
                    env.insert(name.clone(), (*min_val as i128, *max_val as i128));
                }
                c_out.push_str(&format!("    int32_t {} = {};\n", name, expr_to_c(value)));
            }

            Stmt::Prove(cond) => {
                #[cfg(feature = "inquisitor")]
                {
                    use crate::inquisitor::{Inquisitor, Verdict};
                    let mut inq = Inquisitor::new(cond.trim(), source);
                    match inq.auto_prove_with(&env) {
                        Verdict::Approve => {
                            // доказательство принято — продолжаем генерацию
                        }
                        Verdict::Reject => {
                            eprintln!("[VETO] Инквизитор отклонил доказательство — сборка прервана");
                            std::process::exit(1);
                        }
                    }
                }
                #[cfg(not(feature = "inquisitor"))]
                {
                    let _ = &env;
                    println!("[GOD-MODE]: Пропуск проверки `{}` без споров.", cond.trim());
                }
            }

            Stmt::Return(expr) => {
                c_out.push_str(&format!("    return {};\n", expr_to_c(expr)));
            }
        }
    }
    c_out.push_str("}\n");
    c_out
}

fn expr_to_c(expr: &Expr) -> String {
    match expr {
        Expr::Number(n) => n.to_string(),
        Expr::Var(v) => v.clone(),
        Expr::BinOp(l, op, r) => format!("({} {} {})", expr_to_c(l), op, expr_to_c(r)),
    }
}

fn token_to_source(token: &Token) -> String {
    match token {
        Token::Ident(name) => name.clone(),
        Token::Number(number) => number.to_string(),
        Token::Colon => ":".to_string(),
        Token::Semicolon => ";".to_string(),
        Token::Assign => "=".to_string(),
        Token::Less => "<".to_string(),
        Token::Greater => ">".to_string(),
        Token::DotDot => "..".to_string(),
        Token::LParen => "(".to_string(),
        Token::RParen => ")".to_string(),
        Token::LBrace => "{".to_string(),
        Token::RBrace => "}".to_string(),
        Token::Plus => "+".to_string(),
        Token::Minus => "-".to_string(),
        Token::Star => "*".to_string(),
        Token::Arrow => "->".to_string(),
        Token::Fn => "fn".to_string(),
        Token::Let => "let".to_string(),
        Token::Mut => "mut".to_string(),
        Token::Prove => "prove".to_string(),
        Token::Return => "return".to_string(),
        Token::EOF => "".to_string(),
    }
}