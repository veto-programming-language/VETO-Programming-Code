use crate::lexer::Token;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub enum Expr {
    Number(i64),
    Var(String),
    Index(String, Box<Expr>),
    Call(String, Vec<Expr>),
    BinOp(Box<Expr>, String, Box<Expr>),
}

#[derive(Debug, Clone)]
pub enum Stmt {
    Let {
        name: String,
        min_val: i64,
        max_val: i64,
        value: Expr,
    },
    LetArray {
        name: String,
        min_val: i64,
        max_val: i64,
        len: usize,
        elements: Vec<Expr>,
    },
    Assign {
        name: String,
        value: Expr,
    },
    Loop {
        body: Vec<Stmt>,
    },
    While {
        cond: Expr,
        body: Vec<Stmt>,
    },
    If {
        cond: Expr,
        then_body: Vec<Stmt>,
        else_body: Vec<Stmt>,
    },
    IndexAssign {
        name: String,
        index: Expr,
        value: Expr,
    },
    ExprStmt(Expr),
    Prove(String),
    Return(Expr),
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub min_val: i64,
    pub max_val: i64,
}

#[derive(Debug, Clone)]
pub struct Function {
    pub name: String,
    pub params: Vec<Param>,
    pub ret_min: i64,
    pub ret_max: i64,
    pub body: Vec<Stmt>,
}

#[derive(Debug)]
pub struct Program {
    pub functions: Vec<Function>,
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

    fn peek_next(&self) -> &Token {
        self.tokens.get(self.pos + 1).unwrap_or(&Token::EOF)
    }

    fn advance(&mut self) -> Token {
        let tok = self.peek().clone();
        self.pos += 1;
        tok
    }

    pub fn parse_main(&mut self) -> Program {
        let mut functions = Vec::new();
        while *self.peek() != Token::EOF {
            match self.peek() {
                Token::Use => {
                    while *self.peek() != Token::Semicolon && *self.peek() != Token::EOF {
                        self.advance();
                    }
                    if *self.peek() == Token::Semicolon {
                        self.advance();
                    }
                }
                Token::Naked => {
                    self.advance();
                }
                Token::Fn => {
                    functions.push(self.parse_function());
                }
                _ => {
                    self.advance();
                }
            }
        }
        Program { functions }
    }

    fn parse_range(&mut self) -> (i64, i64) {
        let mut min_val = i64::MIN;
        let mut max_val = i64::MAX;
        if *self.peek() == Token::Less {
            self.advance();
            if let Some(min) = self.parse_signed() {
                min_val = min;
            }
            if *self.peek() == Token::DotDot {
                self.advance();
            }
            if let Some(max) = self.parse_signed() {
                max_val = max;
            }
            if *self.peek() == Token::Greater {
                self.advance();
            }
        }
        (min_val, max_val)
    }

    fn parse_function(&mut self) -> Function {
        self.advance(); // 'fn'
        let name = match self.advance() {
            Token::Ident(n) => n,
            other => panic!("Ожидалось имя функции после fn, найдено {:?}", other),
        };

        if *self.peek() == Token::LParen {
            self.advance();
        }

        let mut params = Vec::new();
        while *self.peek() != Token::RParen && *self.peek() != Token::EOF {
            if *self.peek() == Token::Comma {
                self.advance();
                continue;
            }
            let param_name = match self.advance() {
                Token::Ident(n) => n,
                _ => break,
            };
            if *self.peek() == Token::Colon {
                self.advance();
            }
            if *self.peek() == Token::Raw {
                self.advance();
            }
            self.advance();
            let (min_val, max_val) = self.parse_range();
            params.push(Param {
                name: param_name,
                min_val,
                max_val,
            });
        }

        if *self.peek() == Token::RParen {
            self.advance();
        }

        let mut ret_min = i64::MIN;
        let mut ret_max = i64::MAX;
        if *self.peek() == Token::Arrow {
            self.advance();
            self.advance();
            let (r_min, r_max) = self.parse_range();
            ret_min = r_min;
            ret_max = r_max;
        }

        let body = self.parse_block();
        Function {
            name,
            params,
            ret_min,
            ret_max,
            body,
        }
    }

    fn parse_block(&mut self) -> Vec<Stmt> {
        if *self.peek() == Token::LBrace {
            self.advance();
        }
        let mut body = Vec::new();
        while *self.peek() != Token::RBrace && *self.peek() != Token::EOF {
            match self.peek() {
                Token::Let => body.push(self.parse_let()),
                Token::If => body.push(self.parse_if()),
                Token::Loop => {
                    self.advance();
                    let loop_body = self.parse_block();
                    body.push(Stmt::Loop { body: loop_body });
                }
                Token::While => {
                    self.advance();
                    let cond = self.parse_comparison();
                    let while_body = self.parse_block();
                    body.push(Stmt::While {
                        cond,
                        body: while_body,
                    });
                }
                Token::Prove => body.push(self.parse_prove()),
                Token::Return => body.push(self.parse_return()),
                Token::Ident(_) if *self.peek_next() == Token::Assign => {
                    let name = match self.advance() {
                        Token::Ident(n) => n,
                        _ => unreachable!(),
                    };
                    self.advance();
                    let value = self.parse_expr();
                    if *self.peek() == Token::Semicolon {
                        self.advance();
                    }
                    body.push(Stmt::Assign { name, value });
                }
                Token::Ident(_) if *self.peek_next() == Token::LBracket => {
                    let name = match self.advance() {
                        Token::Ident(n) => n,
                        _ => unreachable!(),
                    };
                    self.advance(); // '['
                    let index = self.parse_expr();
                    if *self.peek() == Token::RBracket {
                        self.advance(); // ']'
                    }
                    if *self.peek() == Token::Assign {
                        self.advance(); // '='
                    }
                    let value = self.parse_expr();
                    if *self.peek() == Token::Semicolon {
                        self.advance();
                    }
                    body.push(Stmt::IndexAssign { name, index, value });
                }
                Token::Ident(_) if *self.peek_next() == Token::LParen => {
                    let expr = self.parse_expr();
                    if *self.peek() == Token::Semicolon {
                        self.advance();
                    }
                    body.push(Stmt::ExprStmt(expr));
                }
                _ => {
                    self.advance();
                }
            }
        }
        if *self.peek() == Token::RBrace {
            self.advance();
        }
        body
    }

    fn parse_if(&mut self) -> Stmt {
        self.advance();
        let cond = self.parse_comparison();
        let then_body = self.parse_block();
        let else_body = if *self.peek() == Token::Else {
            self.advance();
            if *self.peek() == Token::If {
                vec![self.parse_if()]
            } else {
                self.parse_block()
            }
        } else {
            Vec::new()
        };
        Stmt::If {
            cond,
            then_body,
            else_body,
        }
    }

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
            self.advance();
        }

        let name = match self.advance() {
            Token::Ident(n) => n,
            _ => panic!("Ожидалось имя переменной после let"),
        };

        self.advance(); // ':'

        // Проверяем, массив ли это: [i32<min..max>; N]
        if *self.peek() == Token::LBracket {
            self.advance(); // '['
            self.advance(); // тип элемента ('i32')
            let (min_val, max_val) = self.parse_range();
            if *self.peek() == Token::Semicolon {
                self.advance(); // ';'
            }
            let len = match self.advance() {
                Token::Number(n) if n > 0 => n as usize,
                other => panic!("Ожидалась длина массива > 0, найдено {:?}", other),
            };
            if *self.peek() == Token::RBracket {
                self.advance(); // ']'
            }
            if *self.peek() == Token::Assign {
                self.advance(); // '='
            }
            if *self.peek() == Token::LBracket {
                self.advance(); // '['
            }
            let mut elements = Vec::new();
            while *self.peek() != Token::RBracket && *self.peek() != Token::EOF {
                if *self.peek() == Token::Comma {
                    self.advance();
                    continue;
                }
                elements.push(self.parse_expr());
            }
            if *self.peek() == Token::RBracket {
                self.advance(); // ']'
            }
            if *self.peek() == Token::Semicolon {
                self.advance();
            }
            return Stmt::LetArray {
                name,
                min_val,
                max_val,
                len,
                elements,
            };
        }

        self.advance(); // обычный тип ('i32')
        let (min_val, max_val) = self.parse_range();

        self.advance(); // '='
        let value = self.parse_expr();
        if *self.peek() == Token::Semicolon {
            self.advance();
        }

        Stmt::Let {
            name,
            min_val,
            max_val,
            value,
        }
    }

    fn parse_prove(&mut self) -> Stmt {
        self.advance();
        self.advance();
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
            if matches!(
                token,
                Token::Less | Token::Greater | Token::Assign | Token::Bang
            ) && *self.peek() == Token::Assign
            {
                self.advance();
                s.push('=');
            }
            parts.push(s);
        }
        self.advance();
        if *self.peek() == Token::Semicolon {
            self.advance();
        }
        Stmt::Prove(parts.join(" "))
    }
    fn parse_return(&mut self) -> Stmt {
        self.advance();
        let expr = self.parse_expr();
        if *self.peek() == Token::Semicolon {
            self.advance();
        }
        Stmt::Return(expr)
    }
    fn parse_comparison(&mut self) -> Expr {
        let left = self.parse_expr();
        let op = match self.peek() {
            Token::Less => {
                self.advance();
                if *self.peek() == Token::Assign {
                    self.advance();
                    "<="
                } else {
                    "<"
                }
            }
            Token::Greater => {
                self.advance();
                if *self.peek() == Token::Assign {
                    self.advance();
                    ">="
                } else {
                    ">"
                }
            }
            Token::Assign => {
                self.advance();
                if *self.peek() == Token::Assign {
                    self.advance();
                }
                "=="
            }
            Token::Bang => {
                self.advance();
                if *self.peek() == Token::Assign {
                    self.advance();
                }
                "!="
            }
            _ => return left,
        };
        let right = self.parse_expr();
        Expr::BinOp(Box::new(left), op.to_string(), Box::new(right))
    }
    fn parse_expr(&mut self) -> Expr {
        let mut left = self.parse_shift();
        loop {
            let op = match self.peek() {
                Token::Plus => "+",
                Token::Minus => "-",
                Token::Amp => "&",
                Token::Pipe => "|",
                _ => break,
            };
            self.advance();
            let right = self.parse_shift();
            left = Expr::BinOp(Box::new(left), op.to_string(), Box::new(right));
        }
        left
    }
    fn parse_shift(&mut self) -> Expr {
        let mut left = self.parse_term();
        loop {
            let op = match self.peek() {
                Token::Shl => "<<",
                Token::Shr => ">>",
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
        loop {
            let op = match self.peek() {
                Token::Star => "*",
                Token::Slash => "/",
                Token::Percent => "%",
                _ => break,
            };
            self.advance();
            let right = self.parse_atom();
            left = Expr::BinOp(Box::new(left), op.to_string(), Box::new(right));
        }
        left
    }
    fn parse_atom(&mut self) -> Expr {
        match self.advance() {
            Token::Number(n) => Expr::Number(n),
            Token::Ident(name) => {
                if *self.peek() == Token::LParen {
                    self.advance();
                    let mut args = Vec::new();
                    while *self.peek() != Token::RParen && *self.peek() != Token::EOF {
                        if *self.peek() == Token::Comma {
                            self.advance();
                            continue;
                        }
                        args.push(self.parse_expr());
                    }
                    if *self.peek() == Token::RParen {
                        self.advance();
                    }
                    Expr::Call(name, args)
                } else if *self.peek() == Token::LBracket {
                    self.advance(); // '['
                    let idx = self.parse_expr();
                    if *self.peek() == Token::RBracket {
                        self.advance(); // ']'
                    }
                    Expr::Index(name, Box::new(idx))
                } else {
                    Expr::Var(name)
                }
            }
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
#[derive(Clone)]
struct FnSig {
    params: Vec<(String, i64, i64)>,
    ret_min: i64,
    ret_max: i64,
}
#[derive(Clone)]
struct ArrayInfo {
    len: usize,
    elem_min: i128,
    elem_max: i128,
}
fn eval_expr_range(
    expr: &Expr,
    env: &HashMap<String, (i128, i128)>,
    arrays: &HashMap<String, ArrayInfo>,
    sigs: &HashMap<String, FnSig>,
) -> Result<(i128, i128), String> {
    let range = match expr {
        Expr::Number(n) => (*n as i128, *n as i128),
        Expr::Var(v) => env
            .get(v)
            .copied()
            .ok_or_else(|| format!("у переменной {} не задан строгий диапазон", v))?,
        Expr::Index(arr_name, idx_expr) => {
            let info = arrays
                .get(arr_name)
                .ok_or_else(|| format!("неизвестный массив {}", arr_name))?;
            let (idx_min, idx_max) = eval_expr_range(idx_expr, env, arrays, sigs)?;
            if idx_min < 0 || idx_max >= info.len as i128 {
                return Err(format!(
                    "выход за границы массива {}[0..{}]: индекс может быть [{}..{}]!",
                    arr_name,
                    info.len - 1,
                    idx_min,
                    idx_max
                ));
            }
            (info.elem_min, info.elem_max)
        }
        Expr::Call(fn_name, args) => {
            let sig = sigs
                .get(fn_name)
                .ok_or_else(|| format!("неизвестная функция {}", fn_name))?;
            if args.len() != sig.params.len() {
                return Err(format!(
                    "функция {} ожидает {} аргументов, передано {}",
                    fn_name,
                    sig.params.len(),
                    args.len()
                ));
            }
            for (arg_expr, (p_name, p_min, p_max)) in args.iter().zip(sig.params.iter()) {
                let (a_min, a_max) = eval_expr_range(arg_expr, env, arrays, sigs)?;
                if a_min < *p_min as i128 || a_max > *p_max as i128 {
                    return Err(format!(
                        "аргумент {} функции {} требует <{}..{}>, передан [{}..{}]",
                        p_name, fn_name, p_min, p_max, a_min, a_max
                    ));
                }
            }
            (sig.ret_min as i128, sig.ret_max as i128)
        }
        Expr::BinOp(l, op, r) => {
            let (l1, h1) = eval_expr_range(l, env, arrays, sigs)?;
            let (l2, h2) = eval_expr_range(r, env, arrays, sigs)?;
            match op.as_str() {
                "+" => (l1.saturating_add(l2), h1.saturating_add(h2)),
                "-" => (l1.saturating_sub(h2), h1.saturating_sub(l2)),
                "*" => {
                    let candidates = [
                        l1.saturating_mul(l2),
                        l1.saturating_mul(h2),
                        h1.saturating_mul(l2),
                        h1.saturating_mul(h2),
                    ];
                    (
                        *candidates.iter().min().unwrap(),
                        *candidates.iter().max().unwrap(),
                    )
                }
                "/" => {
                    if l2 <= 0 && h2 >= 0 {
                        return Err(format!(
                            "возможно деление на ноль! Делитель в диапазоне [{}..{}]",
                            l2, h2
                        ));
                    }
                    let candidates = [l1 / l2, l1 / h2, h1 / l2, h1 / h2];
                    (
                        *candidates.iter().min().unwrap(),
                        *candidates.iter().max().unwrap(),
                    )
                }
                "%" => {
                    if l2 <= 0 && h2 >= 0 {
                        return Err(format!(
                            "остаток от деления на ноль! Делитель в [{}..{}]",
                            l2, h2
                        ));
                    }
                    let max_mod = l2.abs().max(h2.abs()) - 1;
                    (-max_mod, max_mod)
                }
                "<<" => {
                    if l2 < 0 || h2 > 30 {
                        return Err(format!("небезопасный сдвиг влево на [{}..{}]", l2, h2));
                    }
                    (l1 << l2, h1 << h2)
                }
                ">>" => {
                    if l2 < 0 || h2 > 30 {
                        return Err(format!("небезопасный сдвиг вправо на [{}..{}]", l2, h2));
                    }
                    let candidates = [l1 >> l2, l1 >> h2, h1 >> l2, h1 >> h2];
                    (
                        *candidates.iter().min().unwrap(),
                        *candidates.iter().max().unwrap(),
                    )
                }
                "&" => {
                    if l2 >= 0 && h2 >= 0 {
                        (0, h2)
                    } else {
                        (i32::MIN as i128, i32::MAX as i128)
                    }
                }
                "|" => (l1.min(l2), h1.max(h2).saturating_mul(2)),
                "<" | "<=" | ">" | ">=" | "==" | "!=" => (0, 1),
                _ => return Err(format!("неизвестный оператор {}", op)),
            }
        }
    };
    if range.0 < i32::MIN as i128 || range.1 > i32::MAX as i128 {
        return Err(format!(
            "возможно переполнение i32: результат может попасть в [{}..{}]",
            range.0, range.1
        ));
    }
    Ok(range)
}
fn block_always_returns(stmts: &[Stmt]) -> bool {
    match stmts.last() {
        Some(Stmt::Return(_)) => true,
        Some(Stmt::If {
            then_body,
            else_body,
            ..
        }) => {
            !else_body.is_empty()
                && block_always_returns(then_body)
                && block_always_returns(else_body)
        }
        _ => false,
    }
}
fn refine_envs(
    cond: &Expr,
    base_env: &HashMap<String, (i128, i128)>,
) -> (HashMap<String, (i128, i128)>, HashMap<String, (i128, i128)>) {
    let mut then_env = base_env.clone();
    let mut else_env = base_env.clone();
    if let Expr::BinOp(l, op, r) = cond {
        if let (Expr::Var(var), Expr::Number(num)) = (l.as_ref(), r.as_ref()) {
            if let Some(&(lo, hi)) = base_env.get(var) {
                let c = *num as i128;
                match op.as_str() {
                    ">" => {
                        then_env.insert(var.clone(), (lo.max(c + 1), hi));
                        else_env.insert(var.clone(), (lo, hi.min(c)));
                    }
                    ">=" => {
                        then_env.insert(var.clone(), (lo.max(c), hi));
                        else_env.insert(var.clone(), (lo, hi.min(c - 1)));
                    }
                    "<" => {
                        then_env.insert(var.clone(), (lo, hi.min(c - 1)));
                        else_env.insert(var.clone(), (lo.max(c), hi));
                    }
                    "<=" => {
                        then_env.insert(var.clone(), (lo, hi.min(c)));
                        else_env.insert(var.clone(), (lo.max(c + 1), hi));
                    }
                    "==" => {
                        then_env.insert(var.clone(), (c, c));
                    }
                    _ => {}
                }
            }
        }
    }
    (then_env, else_env)
}
#[allow(clippy::too_many_arguments)]
fn emit_stmts(
    stmts: &[Stmt],
    func: &Function,
    env: &mut HashMap<String, (i128, i128)>,
    declared: &mut HashMap<String, (i128, i128)>,
    arrays: &mut HashMap<String, ArrayInfo>,
    sigs: &HashMap<String, FnSig>,
    source: &str,
    indent: &str,
    out: &mut String,
) {
    let _ = source;
    for stmt in stmts {
        match stmt {
            Stmt::Let {
                name,
                min_val,
                max_val,
                value,
            } => {
                if *min_val > *max_val {
                    eprintln!(
                        "\n[VETO]: Некорректный диапазон у {}: минимум ({}) больше максимума ({})!",
                        name, min_val, max_val
                    );
                    std::process::exit(1);
                }
                match eval_expr_range(value, env, arrays, sigs) {
                    Ok((expr_min, expr_max)) => {
                        if expr_min < *min_val as i128 || expr_max > *max_val as i128 {
                            eprintln!(
                                "\n[VETO]: Ошибка диапазона! Переменная {} объявлена как <{}..{}>, но выражение может дать [{}..{}]!",
                                name, min_val, max_val, expr_min, expr_max
                            );
                            std::process::exit(1);
                        }
                    }
                    Err(err) => {
                        if *min_val != i64::MIN || *max_val != i64::MAX {
                            eprintln!(
                                "\n[VETO]: Не могу доказать диапазон для {} (<{}..{}>): {}",
                                name, min_val, max_val, err
                            );
                            std::process::exit(1);
                        }
                    }
                }
                if !(*min_val == i64::MIN && *max_val == i64::MAX) {
                    let bounds = (*min_val as i128, *max_val as i128);
                    env.insert(name.clone(), bounds);
                    declared.insert(name.clone(), bounds);
                }
                out.push_str(&format!(
                    "{}int32_t {} = {};\n",
                    indent,
                    name,
                    expr_to_c(value)
                ));
            }
            Stmt::LetArray {
                name,
                min_val,
                max_val,
                len,
                elements,
            } => {
                if elements.len() != *len {
                    eprintln!(
                        "\n[VETO]: Массив {} объявлен длиной {}, но передано {} элементов!",
                        name,
                        len,
                        elements.len()
                    );
                    std::process::exit(1);
                }
                let mut elems_c = Vec::new();
                for (i, el) in elements.iter().enumerate() {
                    match eval_expr_range(el, env, arrays, sigs) {
                        Ok((emin, emax)) => {
                            if emin < *min_val as i128 || emax > *max_val as i128 {
                                eprintln!(
                                    "\n[VETO]: Элемент {}[{}] выходит за диапазон <{}..{}>!",
                                    name, i, min_val, max_val
                                );
                                std::process::exit(1);
                            }
                        }
                        Err(err) => {
                            eprintln!("\n[VETO]: Ошибка в элементе {}[{}]: {}", name, i, err);
                            std::process::exit(1);
                        }
                    }
                    elems_c.push(expr_to_c(el));
                }
                arrays.insert(
                    name.clone(),
                    ArrayInfo {
                        len: *len,
                        elem_min: *min_val as i128,
                        elem_max: *max_val as i128,
                    },
                );
                out.push_str(&format!(
                    "{}int32_t {}[{}] = {{{}}};\n",
                    indent,
                    name,
                    len,
                    elems_c.join(", ")
                ));
            }
            Stmt::Assign { name, value } => {
                if let Some(&(decl_min, decl_max)) = declared.get(name) {
                    match eval_expr_range(value, env, arrays, sigs) {
                        Ok((expr_min, expr_max)) => {
                            if expr_min < decl_min || expr_max > decl_max {
                                eprintln!(
                                    "\n[VETO]: Присваивание в {} нарушает его контракт <{}..{}> (выражение даёт [{}..{}])!",
                                    name, decl_min, decl_max, expr_min, expr_max
                                );
                                std::process::exit(1);
                            }
                            env.insert(name.clone(), (expr_min, expr_max));
                        }
                        Err(err) => {
                            eprintln!(
                                "\n[VETO]: Не могу доказать присваивание в {} (<{}..{}>): {}",
                                name, decl_min, decl_max, err
                            );
                            std::process::exit(1);
                        }
                    }
                }
                out.push_str(&format!("{}{} = {};\n", indent, name, expr_to_c(value)));
            }
            Stmt::IndexAssign { name, index, value } => {
                if let Some(info) = arrays.get(name) {
                    match eval_expr_range(index, env, arrays, sigs) {
                        Ok((imin, imax)) => {
                            if imin < 0 || imax >= info.len as i128 {
                                eprintln!(
                                    "\n[VETO]: Выход за границы массива {}[0..{}] при записи: индекс [{}..{}]!",
                                    name, info.len - 1, imin, imax
                                );
                                std::process::exit(1);
                            }
                        }
                        Err(err) => {
                            eprintln!(
                                "\n[VETO]: Не могу доказать индекс записи в {}: {}",
                                name, err
                            );
                            std::process::exit(1);
                        }
                    }
                    match eval_expr_range(value, env, arrays, sigs) {
                        Ok((vmin, vmax)) => {
                            if vmin < info.elem_min || vmax > info.elem_max {
                                eprintln!(
                                    "\n[VETO]: Значение [{}..{}] выходит за тип массива {}!",
                                    vmin, vmax, name
                                );
                                std::process::exit(1);
                            }
                        }
                        Err(err) => {
                            eprintln!("\n[VETO]: Не могу доказать значение для {}: {}", name, err);
                            std::process::exit(1);
                        }
                    }
                }
                out.push_str(&format!(
                    "{}{}[{}] = {};\n",
                    indent,
                    name,
                    expr_to_c(index),
                    expr_to_c(value)
                ));
            }
            Stmt::Loop { body } => {
                for (k, v) in declared.iter() {
                    env.insert(k.clone(), *v);
                }
                out.push_str(&format!("{}while (1) {{\n", indent));
                emit_stmts(
                    body,
                    func,
                    env,
                    declared,
                    arrays,
                    sigs,
                    source,
                    &format!("{}    ", indent),
                    out,
                );
                out.push_str(&format!("{}}}\n", indent));
            }
            Stmt::While { cond, body } => {
                out.push_str(&format!("{}while ({}) {{\n", indent, expr_to_c(cond)));
                let (mut body_env, _) = refine_envs(cond, env);
                emit_stmts(
                    body,
                    func,
                    &mut body_env,
                    declared,
                    arrays,
                    sigs,
                    source,
                    &format!("{}    ", indent),
                    out,
                );
                out.push_str(&format!("{}}}\n", indent));
            }
            Stmt::ExprStmt(expr) => {
                if let Err(err) = eval_expr_range(expr, env, arrays, sigs) {
                    eprintln!("\n[VETO]: Ошибка при вызове: {}", err);
                    std::process::exit(1);
                }
                out.push_str(&format!("{}{};\n", indent, expr_to_c(expr)));
            }
            Stmt::If {
                cond,
                then_body,
                else_body,
            } => {
                out.push_str(&format!("{}if ({}) {{\n", indent, expr_to_c(cond)));
                let (mut then_env, mut else_env) = refine_envs(cond, env);
                let mut then_decl = declared.clone();
                let mut else_decl = declared.clone();
                emit_stmts(
                    then_body,
                    func,
                    &mut then_env,
                    &mut then_decl,
                    arrays,
                    sigs,
                    source,
                    &format!("{}    ", indent),
                    out,
                );
                if !else_body.is_empty() {
                    out.push_str(&format!("{}}} else {{\n", indent));
                    emit_stmts(
                        else_body,
                        func,
                        &mut else_env,
                        &mut else_decl,
                        arrays,
                        sigs,
                        source,
                        &format!("{}    ", indent),
                        out,
                    );
                }
                out.push_str(&format!("{}}}\n", indent));
                if block_always_returns(then_body) {
                    *env = else_env;
                } else if !else_body.is_empty() && block_always_returns(else_body) {
                    *env = then_env;
                } else {
                    for (k, &(t_lo, t_hi)) in then_env.iter() {
                        if let Some(&(e_lo, e_hi)) = else_env.get(k) {
                            if let Some(&(orig_lo, orig_hi)) = env.get(k) {
                                let new_lo = t_lo.min(e_lo).max(orig_lo);
                                let new_hi = t_hi.max(e_hi).min(orig_hi);
                                if new_lo <= new_hi {
                                    env.insert(k.clone(), (new_lo, new_hi));
                                }
                            }
                        }
                    }
                }
            }
            Stmt::Prove(cond) => {
                #[cfg(feature = "inquisitor")]
                {
                    use crate::inquisitor::{Inquisitor, Verdict};
                    let mut inq = Inquisitor::new(cond.trim(), source);
                    match inq.auto_prove_with(env) {
                        Verdict::Approve => {}
                        Verdict::Reject => {
                            eprintln!(
                                "[VETO] Инквизитор отклонил доказательство — сборка прервана"
                            );
                            std::process::exit(1);
                        }
                    }
                }
                #[cfg(not(feature = "inquisitor"))]
                {
                    let _ = &env;
                    println!("[GOD-MODE]: Пропуск проверки {} без споров.", cond.trim());
                }
            }
            Stmt::Return(expr) => {
                if func.ret_min != i64::MIN || func.ret_max != i64::MAX {
                    match eval_expr_range(expr, env, arrays, sigs) {
                        Ok((ret_lo, ret_hi)) => {
                            if ret_lo < func.ret_min as i128 || ret_hi > func.ret_max as i128 {
                                eprintln!(
"\n[VETO]: Нарушен контракт функции {}! Ожидался возврат <{}..{}>, но выражение даёт [{}..{}]!",
func.name, func.ret_min, func.ret_max, ret_lo, ret_hi
);
                                std::process::exit(1);
                            }
                        }
                        Err(err) => {
                            eprintln!(
"\n[VETO]: Не могу доказать возвращаемый диапазон <{}..{}> в {}: {}",
func.ret_min, func.ret_max, func.name, err
);
                            std::process::exit(1);
                        }
                    }
                }
                out.push_str(&format!("{}return {};\n", indent, expr_to_c(expr)));
            }
        }
    }
}

pub fn generate_c(program: &Program, source: &str) -> String {
    let mut c_out = String::from(
        r#"#include <stdint.h>
#include <stdlib.h>
#include <time.h>
#include <SDL2/SDL.h>

static SDL_Window *veto_win = NULL;
static SDL_Renderer *veto_ren = NULL;
static int32_t veto_zbuf;

int32_t gfx_open(int32_t w, int32_t h) {
    srand((unsigned int)time(NULL));
    SDL_Init(SDL_INIT_VIDEO);
    veto_win = SDL_CreateWindow("VETO-DOOM // ULTRA HD (400 rays)", SDL_WINDOWPOS_CENTERED, SDL_WINDOWPOS_CENTERED, w, h, 0);
    veto_ren = SDL_CreateRenderer(veto_win, -1, SDL_RENDERER_ACCELERATED | SDL_RENDERER_PRESENTVSYNC);
    return 1;
}

int32_t veto_rand(int32_t max_val) {
    if (max_val <= 0) return 0;
    return rand() % (max_val + 1);
}

int32_t gfx_key_down(int32_t code) {
    SDL_Event e;
    while (SDL_PollEvent(&e)) {
        if (e.type == SDL_QUIT) {
            SDL_Quit();
            exit(0);
        }
    }
    const uint8_t *keys = SDL_GetKeyboardState(NULL);
    if (keys[SDL_SCANCODE_ESCAPE]) {
        SDL_Quit();
        exit(0);
    }
    if (code == 1) return (keys[SDL_SCANCODE_Y] || keys[SDL_SCANCODE_W]) ? 1 : 0;
    if (code == 2) return (keys[SDL_SCANCODE_G] || keys[SDL_SCANCODE_A]) ? 1 : 0;
    if (code == 3) return (keys[SDL_SCANCODE_H] || keys[SDL_SCANCODE_S]) ? 1 : 0;
    if (code == 4) return (keys[SDL_SCANCODE_J] || keys[SDL_SCANCODE_D]) ? 1 : 0;
    if (code == 5) return keys[SDL_SCANCODE_SPACE] ? 1 : 0;
    return 0;
}

int32_t gfx_clear(void) {
    for (int i = 0; i < 400; i++) veto_zbuf[i] = 9999;
    SDL_SetRenderDrawColor(veto_ren, 14, 16, 24, 255);
    SDL_Rect ceil_r = { 0, 0, 800, 300 };
    SDL_RenderFillRect(veto_ren, &ceil_r);
    SDL_SetRenderDrawColor(veto_ren, 38, 35, 32, 255);
    SDL_Rect floor_r = { 0, 300, 800, 300 };
    SDL_RenderFillRect(veto_ren, &floor_r);
    return 0;
}

int32_t gfx_draw_vline(int32_t col, int32_t top, int32_t bot, int32_t shade, int32_t dist) {
    if (col >= 0 && col < 400) {
        veto_zbuf[col] = dist;
    }
    SDL_SetRenderDrawColor(veto_ren, (uint8_t)shade, (uint8_t)(shade / 3), (uint8_t)(shade / 4), 255);
    SDL_Rect r = { col * 2, top, 2, bot - top };
    SDL_RenderFillRect(veto_ren, &r);
    return 0;
}

int32_t gfx_draw_enemy(int32_t screen_x, int32_t dist, int32_t hurt_flash) {
    if (dist <= 1) dist = 1;
    int32_t h = 460 / dist;
    if (h < 16) h = 16;
    if (h > 520) h = 520;
    int32_t w = h / 2;
    int32_t left = screen_x - (w / 2);
    int32_t top = 300 - (h / 2) + (h / 8);

    for (int32_t sx = left; sx < left + w; sx += 2) {
        int32_t col = sx / 2;
        if (col >= 0 && col < 400 && dist < veto_zbuf[col]) {
            if (hurt_flash > 0) {
                SDL_SetRenderDrawColor(veto_ren, 255, 255, 255, 255);
            } else {
                SDL_SetRenderDrawColor(veto_ren, 45, 200, 85, 255);
            }
            SDL_Rect body = { sx, top, 2, h };
            SDL_RenderFillRect(veto_ren, &body);

            if (sx > left + w / 4 && sx < left + (w * 3) / 4) {
                SDL_SetRenderDrawColor(veto_ren, 255, 30, 30, 255);
                SDL_Rect eye = { sx, top + h / 5, 2, h / 12 + 2 };
                SDL_RenderFillRect(veto_ren, &eye);
            }
        }
    }
    return 0;
}

int32_t gfx_draw_gun(int32_t recoil, int32_t kills) {
    SDL_SetRenderDrawColor(veto_ren, 0, 255, 170, 255);
    SDL_Rect ch1 = { 392, 299, 16, 2 };
    SDL_Rect ch2 = { 399, 292, 2, 16 };
    SDL_RenderFillRect(veto_ren, &ch1);
    SDL_RenderFillRect(veto_ren, &ch2);

    for (int32_t i = 0; i < kills && i < 35; i++) {
        SDL_SetRenderDrawColor(veto_ren, 0, 255, 130, 255);
        SDL_Rect badge = { 16 + i * 14, 16, 10, 18 };
        SDL_RenderFillRect(veto_ren, &badge);
    }

    int32_t kick = recoil * 3;
    if (recoil > 4) {
        SDL_SetRenderDrawColor(veto_ren, 255, 220, 60, 255);
        SDL_Rect flash = { 365, 370 + kick, 70, 55 };
        SDL_RenderFillRect(veto_ren, &flash);
    }

    SDL_SetRenderDrawColor(veto_ren, 85, 92, 108, 255);
    SDL_Rect barrel = { 382, 410 + kick, 36, 120 };
    SDL_RenderFillRect(veto_ren, &barrel);

    SDL_SetRenderDrawColor(veto_ren, 50, 55, 65, 255);
    SDL_Rect grip = { 370, 480 + kick, 60, 120 };
    SDL_RenderFillRect(veto_ren, &grip);

    SDL_SetRenderDrawColor(veto_ren, 0, 220, 255, 255);
    SDL_Rect core = { 395, 430 + kick, 10, 45 };
    SDL_RenderFillRect(veto_ren, &core);
    return 0;
}

int32_t gfx_present(void) {
    SDL_RenderPresent(veto_ren);
    SDL_Delay(16);
    return 0;
}
"#,
    );

    let mut sigs: HashMap<String, FnSig> = HashMap::new();

    sigs.insert(
        "gfx_open".to_string(),
        FnSig {
            params: vec![("w".into(), 100, 1920), ("h".into(), 100, 1080)],
            ret_min: 0,
            ret_max: 1,
        },
    );
    sigs.insert(
        "veto_rand".to_string(),
        FnSig {
            params: vec![("max_val".into(), 1, 1000)],
            ret_min: 0,
            ret_max: 1000,
        },
    );
    sigs.insert(
        "gfx_key_down".to_string(),
        FnSig {
            params: vec![("code".into(), 1, 5)],
            ret_min: 0,
            ret_max: 1,
        },
    );
    sigs.insert(
        "gfx_clear".to_string(),
        FnSig {
            params: vec![],
            ret_min: 0,
            ret_max: 0,
        },
    );
    sigs.insert(
        "gfx_draw_vline".to_string(),
        FnSig {
            params: vec![
                ("col".into(), 0, 399),
                ("top".into(), 0, 600),
                ("bot".into(), 0, 600),
                ("shade".into(), 0, 255),
                ("dist".into(), 1, 64),
            ],
            ret_min: 0,
            ret_max: 0,
        },
    );
    sigs.insert(
        "gfx_draw_enemy".to_string(),
        FnSig {
            params: vec![
                ("screen_x".into(), 0, 800),
                ("dist".into(), 1, 64),
                ("hurt_flash".into(), 0, 10),
            ],
            ret_min: 0,
            ret_max: 0,
        },
    );
    sigs.insert(
        "gfx_draw_gun".to_string(),
        FnSig {
            params: vec![("recoil".into(), 0, 10), ("kills".into(), 0, 100)],
            ret_min: 0,
            ret_max: 0,
        },
    );
    sigs.insert(
        "gfx_present".to_string(),
        FnSig {
            params: vec![],
            ret_min: 0,
            ret_max: 0,
        },
    );

    for f in &program.functions {
        sigs.insert(
            f.name.clone(),
            FnSig {
                params: f
                    .params
                    .iter()
                    .map(|p| (p.name.clone(), p.min_val, p.max_val))
                    .collect(),
                ret_min: f.ret_min,
                ret_max: f.ret_max,
            },
        );
    }

    for func in &program.functions {
        let mut env: HashMap<String, (i128, i128)> = HashMap::new();
        let mut declared: HashMap<String, (i128, i128)> = HashMap::new();
        let mut arrays: HashMap<String, ArrayInfo> = HashMap::new();
        for p in &func.params {
            if !(p.min_val == i64::MIN && p.max_val == i64::MAX) {
                let bounds = (p.min_val as i128, p.max_val as i128);
                env.insert(p.name.clone(), bounds);
                declared.insert(p.name.clone(), bounds);
            }
        }

        if func.name == "main" {
            c_out.push_str("int main(void) {\n");
        } else {
            let params_c: Vec<String> = func
                .params
                .iter()
                .map(|p| format!("int32_t {}", p.name))
                .collect();
            let params_str = if params_c.is_empty() {
                "void".to_string()
            } else {
                params_c.join(", ")
            };
            c_out.push_str(&format!("int32_t {}({}) {{\n", func.name, params_str));
        }

        emit_stmts(
            &func.body,
            func,
            &mut env,
            &mut declared,
            &mut arrays,
            &sigs,
            source,
            "    ",
            &mut c_out,
        );
        c_out.push_str("}\n\n");
    }

    c_out
}

fn expr_to_c(expr: &Expr) -> String {
    match expr {
        Expr::Number(n) => n.to_string(),
        Expr::Var(v) => v.clone(),
        Expr::Index(arr, idx) => format!("{}[{}]", arr, expr_to_c(idx)),
        Expr::Call(name, args) => {
            let args_c: Vec<String> = args.iter().map(expr_to_c).collect();
            format!("{}({})", name, args_c.join(", "))
        }
        Expr::BinOp(l, op, r) => format!("({} {} {})", expr_to_c(l), op, expr_to_c(r)),
    }
}

fn token_to_source(token: &Token) -> String {
    match token {
        Token::Ident(name) => name.clone(),
        Token::Number(number) => number.to_string(),
        Token::Comma => ",".to_string(),
        Token::Colon => ":".to_string(),
        Token::ColonColon => "::".to_string(),
        Token::Semicolon => ";".to_string(),
        Token::Assign => "=".to_string(),
        Token::Bang => "!".to_string(),
        Token::Less => "<".to_string(),
        Token::Greater => ">".to_string(),
        Token::Shl => "<<".to_string(),
        Token::Shr => ">>".to_string(),
        Token::Amp => "&".to_string(),
        Token::Pipe => "|".to_string(),
        Token::DotDot => "..".to_string(),
        Token::LParen => "(".to_string(),
        Token::RParen => ")".to_string(),
        Token::LBrace => "{".to_string(),
        Token::RBrace => "}".to_string(),
        Token::LBracket => "[".to_string(),
        Token::RBracket => "]".to_string(),
        Token::Plus => "+".to_string(),
        Token::Minus => "-".to_string(),
        Token::Star => "*".to_string(),
        Token::Slash => "/".to_string(),
        Token::Percent => "%".to_string(),
        Token::Arrow => "->".to_string(),
        Token::Fn => "fn".to_string(),
        Token::Let => "let".to_string(),
        Token::Mut => "mut".to_string(),
        Token::If => "if".to_string(),
        Token::Else => "else".to_string(),
        Token::For => "for".to_string(),
        Token::In => "in".to_string(),
        Token::Loop => "loop".to_string(),
        Token::While => "while".to_string(),
        Token::Use => "use".to_string(),
        Token::Simd => "simd".to_string(),
        Token::Prove => "prove".to_string(),
        Token::Raw => "raw".to_string(),
        Token::Naked => "naked".to_string(),
        Token::Return => "return".to_string(),
        Token::EOF => "".to_string(),
    }
}
