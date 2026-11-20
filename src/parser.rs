use crate::ast::*;
use crate::lexer::{Lexer, Token, TokenKind};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    base_dir: String,
    imported: HashSet<String>,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            pos: 0,
            base_dir: ".".into(),
            imported: HashSet::new(),
        }
    }

    pub fn with_base_dir(tokens: Vec<Token>, base_dir: &str) -> Self {
        Self {
            tokens,
            pos: 0,
            base_dir: base_dir.into(),
            imported: HashSet::new(),
        }
    }

    fn peek(&self) -> &TokenKind {
        self.tokens
            .get(self.pos)
            .map(|t| &t.kind)
            .unwrap_or(&TokenKind::EOF)
    }
    fn peek_next(&self) -> &TokenKind {
        self.tokens
            .get(self.pos + 1)
            .map(|t| &t.kind)
            .unwrap_or(&TokenKind::EOF)
    }
    fn line(&self) -> usize {
        self.tokens.get(self.pos).map(|t| t.line).unwrap_or(0)
    }
    fn advance(&mut self) -> TokenKind {
        let t = self.peek().clone();
        self.pos += 1;
        t
    }

    pub fn parse_main(&mut self) -> Program {
        let mut prog = Program::default();
        while *self.peek() != TokenKind::EOF {
            match self.peek() {
                TokenKind::Use => {
                    let ln = self.line();
                    self.advance();
                    if let TokenKind::StrLit(rel_path) = self.advance() {
                        if *self.peek() == TokenKind::Semicolon {
                            self.advance();
                        }
                        let full_path = Path::new(&self.base_dir).join(&rel_path);
                        let canon = full_path.to_string_lossy().to_string();
                        if !self.imported.contains(&canon) {
                            self.imported.insert(canon);
                            prog.imports.push(rel_path.clone());
                            let sub_src = fs::read_to_string(&full_path).unwrap_or_else(|e| {
                                eprintln!(
                                    "[VETO | строка {}]: не могу импортировать '{}': {}",
                                    ln, rel_path, e
                                );
                                std::process::exit(1);
                            });
                            let mut sub_lex = Lexer::new(&sub_src);
                            let sub_tokens = sub_lex.tokenize();
                            let sub_dir =
                                full_path.parent().and_then(|p| p.to_str()).unwrap_or(".");
                            let mut sub_parser = Parser {
                                tokens: sub_tokens,
                                pos: 0,
                                base_dir: sub_dir.into(),
                                imported: self.imported.clone(),
                            };
                            let sub_prog = sub_parser.parse_main();
                            self.imported = sub_parser.imported;
                            prog.structs.extend(sub_prog.structs);
                            prog.consts.extend(sub_prog.consts);
                            prog.functions.extend(sub_prog.functions);
                        }
                    } else {
                        while *self.peek() != TokenKind::Semicolon && *self.peek() != TokenKind::EOF
                        {
                            self.advance();
                        }
                        if *self.peek() == TokenKind::Semicolon {
                            self.advance();
                        }
                    }
                }
                TokenKind::Struct => prog.structs.push(self.parse_struct()),
                TokenKind::Const => prog.consts.push(self.parse_const()),
                TokenKind::Naked => {
                    self.advance();
                }
                TokenKind::Fn => prog.functions.push(self.parse_function()),
                _ => {
                    self.advance();
                }
            }
        }
        prog
    }

    fn parse_struct(&mut self) -> StructDef {
        let line = self.line();
        self.advance();
        let name = match self.advance() {
            TokenKind::Ident(n) => n,
            other => panic!(
                "[VETO | строка {}]: ожидалось имя struct: {:?}",
                line, other
            ),
        };
        if *self.peek() == TokenKind::LBrace {
            self.advance();
        }
        let mut fields = Vec::new();
        while *self.peek() != TokenKind::RBrace && *self.peek() != TokenKind::EOF {
            if *self.peek() == TokenKind::Comma {
                self.advance();
                continue;
            }
            let fname = match self.advance() {
                TokenKind::Ident(n) => n,
                _ => break,
            };
            if *self.peek() == TokenKind::Colon {
                self.advance();
            }
            self.advance();
            let (min_val, max_val) = self.parse_range();
            fields.push(FieldDef {
                name: fname,
                min_val,
                max_val,
            });
            if *self.peek() == TokenKind::Comma {
                self.advance();
            }
        }
        if *self.peek() == TokenKind::RBrace {
            self.advance();
        }
        StructDef { line, name, fields }
    }

    fn parse_range(&mut self) -> (i64, i64) {
        let mut min_val = i64::MIN;
        let mut max_val = i64::MAX;
        if *self.peek() == TokenKind::Less {
            self.advance();
            if let Some(min) = self.parse_signed() {
                min_val = min;
            }
            if *self.peek() == TokenKind::DotDot {
                self.advance();
            }
            if let Some(max) = self.parse_signed() {
                max_val = max;
            }
            if *self.peek() == TokenKind::Greater {
                self.advance();
            }
        }
        (min_val, max_val)
    }

    fn parse_const(&mut self) -> ConstDef {
        let line = self.line();
        self.advance();
        let name = match self.advance() {
            TokenKind::Ident(n) => n,
            other => panic!("ожидалось имя const: {:?}", other),
        };
        if *self.peek() == TokenKind::Colon {
            self.advance();
        }
        self.advance();
        let (min_val, max_val) = self.parse_range();
        if *self.peek() == TokenKind::Assign {
            self.advance();
        }
        let value = self.parse_logic_or();
        if *self.peek() == TokenKind::Semicolon {
            self.advance();
        }
        ConstDef {
            line,
            name,
            min_val,
            max_val,
            value,
        }
    }

    fn parse_function(&mut self) -> Function {
        let line = self.line();
        self.advance();
        let name = match self.advance() {
            TokenKind::Ident(n) => n,
            other => panic!("ожидалось имя fn: {:?}", other),
        };
        if *self.peek() == TokenKind::LParen {
            self.advance();
        }
        let mut params = Vec::new();
        while *self.peek() != TokenKind::RParen && *self.peek() != TokenKind::EOF {
            if *self.peek() == TokenKind::Comma {
                self.advance();
                continue;
            }
            let param_name = match self.advance() {
                TokenKind::Ident(n) => n,
                _ => break,
            };
            if *self.peek() == TokenKind::Colon {
                self.advance();
            }
            if *self.peek() == TokenKind::Raw {
                self.advance();
            }
            let type_name = match self.advance() {
                TokenKind::Ident(t) => t,
                _ => "i32".into(),
            };
            let (min_val, max_val) = self.parse_range();
            params.push(Param {
                name: param_name,
                type_name,
                min_val,
                max_val,
            });
        }
        if *self.peek() == TokenKind::RParen {
            self.advance();
        }
        let mut ret_type = "i32".to_string();
        let mut ret_min = i64::MIN;
        let mut ret_max = i64::MAX;
        if *self.peek() == TokenKind::Arrow {
            self.advance();
            if let TokenKind::Ident(rt) = self.advance() {
                ret_type = rt;
            }
            let (r_min, r_max) = self.parse_range();
            ret_min = r_min;
            ret_max = r_max;
        }
        let body = self.parse_block();
        Function {
            line,
            name,
            params,
            ret_type,
            ret_min,
            ret_max,
            body,
        }
    }

    fn parse_block(&mut self) -> StmtList {
        if *self.peek() == TokenKind::LBrace {
            self.advance();
        }
        let mut body = Vec::new();
        while *self.peek() != TokenKind::RBrace && *self.peek() != TokenKind::EOF {
            let ln = self.line();
            match self.peek() {
                TokenKind::Let => body.push(self.parse_let()),
                TokenKind::If => body.push(self.parse_if()),
                TokenKind::Loop => {
                    self.advance();
                    body.push(Stmt::Loop {
                        line: ln,
                        body: self.parse_block(),
                    });
                }
                TokenKind::While => {
                    self.advance();
                    let cond = self.parse_logic_or();
                    body.push(Stmt::While {
                        line: ln,
                        cond,
                        body: self.parse_block(),
                    });
                }
                TokenKind::Break => {
                    self.advance();
                    if *self.peek() == TokenKind::Semicolon {
                        self.advance();
                    }
                    body.push(Stmt::Break { line: ln });
                }
                TokenKind::Continue => {
                    self.advance();
                    if *self.peek() == TokenKind::Semicolon {
                        self.advance();
                    }
                    body.push(Stmt::Continue { line: ln });
                }
                TokenKind::Prove => body.push(self.parse_prove()),
                TokenKind::Return => body.push(self.parse_return()),
                TokenKind::Ident(_) if *self.peek_next() == TokenKind::Dot => {
                    let obj = match self.advance() {
                        TokenKind::Ident(n) => n,
                        _ => unreachable!(),
                    };
                    self.advance();
                    let field = match self.advance() {
                        TokenKind::Ident(f) => f,
                        _ => unreachable!(),
                    };
                    let op = self.advance();
                    let rhs = self.parse_logic_or();
                    if *self.peek() == TokenKind::Semicolon {
                        self.advance();
                    }
                    let value = match op {
                        TokenKind::Assign => rhs,
                        TokenKind::PlusEq => Expr::BinOp(
                            Box::new(Expr::Field(obj.clone(), field.clone())),
                            "+".into(),
                            Box::new(rhs),
                        ),
                        TokenKind::MinusEq => Expr::BinOp(
                            Box::new(Expr::Field(obj.clone(), field.clone())),
                            "-".into(),
                            Box::new(rhs),
                        ),
                        _ => rhs,
                    };
                    body.push(Stmt::FieldAssign {
                        line: ln,
                        obj,
                        field,
                        value,
                    });
                }
                TokenKind::Ident(_)
                    if matches!(
                        self.peek_next(),
                        TokenKind::Assign
                            | TokenKind::PlusEq
                            | TokenKind::MinusEq
                            | TokenKind::AmpEq
                            | TokenKind::PipeEq
                    ) =>
                {
                    let name = match self.advance() {
                        TokenKind::Ident(n) => n,
                        _ => unreachable!(),
                    };
                    let op = self.advance();
                    let rhs = self.parse_logic_or();
                    if *self.peek() == TokenKind::Semicolon {
                        self.advance();
                    }
                    let value = match op {
                        TokenKind::Assign => rhs,
                        TokenKind::PlusEq => Expr::BinOp(
                            Box::new(Expr::Var(name.clone())),
                            "+".into(),
                            Box::new(rhs),
                        ),
                        TokenKind::MinusEq => Expr::BinOp(
                            Box::new(Expr::Var(name.clone())),
                            "-".into(),
                            Box::new(rhs),
                        ),
                        TokenKind::AmpEq => Expr::BinOp(
                            Box::new(Expr::Var(name.clone())),
                            "&".into(),
                            Box::new(rhs),
                        ),
                        TokenKind::PipeEq => Expr::BinOp(
                            Box::new(Expr::Var(name.clone())),
                            "|".into(),
                            Box::new(rhs),
                        ),
                        _ => unreachable!(),
                    };
                    body.push(Stmt::Assign {
                        line: ln,
                        name,
                        value,
                    });
                }
                TokenKind::Ident(..) if *self.peek_next() == TokenKind::LBracket => {
                    let name = match self.advance() {
                        TokenKind::Ident(n) => n,
                        _ => unreachable!(),
                    };
                    self.advance();
                    let index = self.parse_expr();
                    if *self.peek() == TokenKind::RBracket {
                        self.advance();
                    }
                    let op = self.advance();
                    let rhs = self.parse_logic_or();
                    if *self.peek() == TokenKind::Semicolon {
                        self.advance();
                    }
                    let value = match op {
                        TokenKind::Assign => rhs,
                        TokenKind::PlusEq => Expr::BinOp(
                            Box::new(Expr::Index(name.clone(), Box::new(index.clone()))),
                            "+".into(),
                            Box::new(rhs),
                        ),
                        TokenKind::MinusEq => Expr::BinOp(
                            Box::new(Expr::Index(name.clone(), Box::new(index.clone()))),
                            "-".into(),
                            Box::new(rhs),
                        ),
                        _ => rhs,
                    };
                    body.push(Stmt::IndexAssign {
                        line: ln,
                        name,
                        index,
                        value,
                    });
                }
                TokenKind::Ident(..) if *self.peek_next() == TokenKind::LParen => {
                    let expr = self.parse_expr();
                    if *self.peek() == TokenKind::Semicolon {
                        self.advance();
                    }
                    body.push(Stmt::ExprStmt { line: ln, expr });
                }
                _ => {
                    self.advance();
                }
            }
        }
        if *self.peek() == TokenKind::RBrace {
            self.advance();
        }
        body
    }
    fn parse_if(&mut self) -> Stmt {
        let line = self.line();
        self.advance();
        let cond = self.parse_logic_or();
        let then_body = self.parse_block();
        let else_body = if *self.peek() == TokenKind::Else {
            self.advance();
            if *self.peek() == TokenKind::If {
                vec![self.parse_if()]
            } else {
                self.parse_block()
            }
        } else {
            Vec::new()
        };
        Stmt::If {
            line,
            cond,
            then_body,
            else_body,
        }
    }
    fn parse_signed(&mut self) -> Option<i64> {
        let neg = if *self.peek() == TokenKind::Minus {
            self.advance();
            true
        } else {
            false
        };
        match self.advance() {
            TokenKind::Number(n) => Some(if neg { -n } else { n }),
            _ => None,
        }
    }
    fn parse_let(&mut self) -> Stmt {
        let line = self.line();
        self.advance();
        if *self.peek() == TokenKind::Mut {
            self.advance();
        }
        let name = match self.advance() {
            TokenKind::Ident(n) => n,
            _ => panic!("ожидалось имя после let"),
        };
        self.advance();
        if *self.peek() == TokenKind::LBracket {
            self.advance();
            self.advance();
            let (min_val, max_val) = self.parse_range();
            if *self.peek() == TokenKind::Semicolon {
                self.advance();
            }
            let len = match self.advance() {
                TokenKind::Number(n) if n > 0 => n as usize,
                other => panic!("длина массива > 0: {:?}", other),
            };
            if *self.peek() == TokenKind::RBracket {
                self.advance();
            }
            if *self.peek() == TokenKind::Assign {
                self.advance();
            }
            if *self.peek() == TokenKind::LBracket {
                self.advance();
            }
            let mut elements = Vec::new();
            if *self.peek() != TokenKind::RBracket {
                let first = self.parse_logic_or();
                if *self.peek() == TokenKind::Semicolon {
                    self.advance();
                    let count = match self.advance() {
                        TokenKind::Number(n) if n > 0 => n as usize,
                        other => panic!("повторы: {:?}", other),
                    };
                    for _ in 0..count {
                        elements.push(first.clone());
                    }
                } else {
                    elements.push(first);
                    while *self.peek() != TokenKind::RBracket && *self.peek() != TokenKind::EOF {
                        if *self.peek() == TokenKind::Comma {
                            self.advance();
                            continue;
                        }
                        elements.push(self.parse_logic_or());
                    }
                }
            }
            if *self.peek() == TokenKind::RBracket {
                self.advance();
            }
            if *self.peek() == TokenKind::Semicolon {
                self.advance();
            }
            return Stmt::LetArray {
                line,
                name,
                min_val,
                max_val,
                len,
                elements,
            };
        }
        let type_name = match self.advance() {
            TokenKind::Ident(t) => t,
            _ => "i32".into(),
        };
        let (min_val, max_val) = self.parse_range();
        self.advance();
        let value = self.parse_logic_or();
        if *self.peek() == TokenKind::Semicolon {
            self.advance();
        }
        Stmt::Let {
            line,
            name,
            type_name,
            min_val,
            max_val,
            value,
        }
    }
    fn parse_prove(&mut self) -> Stmt {
        let line = self.line();
        self.advance();
        self.advance();
        let mut parts: StrList = Vec::new();
        let mut depth = 0usize;
        while *self.peek() != TokenKind::EOF {
            match self.peek() {
                TokenKind::RParen if depth == 0 => break,
                TokenKind::LParen => depth += 1,
                TokenKind::RParen => depth -= 1,
                _ => {}
            }
            let token = self.advance();
            let mut s = token_to_source(&token);
            if matches!(
                token,
                TokenKind::Less | TokenKind::Greater | TokenKind::Assign | TokenKind::Bang
            ) && *self.peek() == TokenKind::Assign
            {
                self.advance();
                s.push('=');
            }
            parts.push(s);
        }
        self.advance();
        if *self.peek() == TokenKind::Semicolon {
            self.advance();
        }
        Stmt::Prove {
            line,
            cond: parts.join(" "),
        }
    }
    fn parse_return(&mut self) -> Stmt {
        let line = self.line();
        self.advance();
        let expr = self.parse_logic_or();
        if *self.peek() == TokenKind::Semicolon {
            self.advance();
        }
        Stmt::Return { line, expr }
    }
    fn parse_logic_or(&mut self) -> Expr {
        let mut left = self.parse_logic_and();
        while *self.peek() == TokenKind::OrOr {
            self.advance();
            left = Expr::BinOp(
                Box::new(left),
                "||".into(),
                Box::new(self.parse_logic_and()),
            );
        }
        left
    }
    fn parse_logic_and(&mut self) -> Expr {
        let mut left = self.parse_comparison();
        while *self.peek() == TokenKind::AndAnd {
            self.advance();
            left = Expr::BinOp(
                Box::new(left),
                "&&".into(),
                Box::new(self.parse_comparison()),
            );
        }
        left
    }
    fn parse_comparison(&mut self) -> Expr {
        let left = self.parse_expr();
        let op = match self.peek() {
            TokenKind::Less => {
                self.advance();
                if *self.peek() == TokenKind::Assign {
                    self.advance();
                    "<="
                } else {
                    "<"
                }
            }
            TokenKind::Greater => {
                self.advance();
                if *self.peek() == TokenKind::Assign {
                    self.advance();
                    ">="
                } else {
                    ">"
                }
            }
            TokenKind::Assign => {
                self.advance();
                if *self.peek() == TokenKind::Assign {
                    self.advance();
                }
                "=="
            }
            TokenKind::Bang => {
                self.advance();
                if *self.peek() == TokenKind::Assign {
                    self.advance();
                }
                "!="
            }
            _ => return left,
        };
        Expr::BinOp(Box::new(left), op.into(), Box::new(self.parse_expr()))
    }
    fn parse_expr(&mut self) -> Expr {
        let mut left = self.parse_shift();
        loop {
            let op = match self.peek() {
                TokenKind::Plus => "+",
                TokenKind::Minus => "-",
                TokenKind::Amp => "&",
                TokenKind::Pipe => "|",
                TokenKind::Caret => "^",
                _ => break,
            };
            self.advance();
            left = Expr::BinOp(Box::new(left), op.into(), Box::new(self.parse_shift()));
        }
        left
    }
    fn parse_shift(&mut self) -> Expr {
        let mut left = self.parse_term();
        loop {
            let op = match self.peek() {
                TokenKind::Shl => "<<",
                TokenKind::Shr => ">>",
                _ => break,
            };
            self.advance();
            left = Expr::BinOp(Box::new(left), op.into(), Box::new(self.parse_term()));
        }
        left
    }
    fn parse_term(&mut self) -> Expr {
        let mut left = self.parse_atom();
        loop {
            let op = match self.peek() {
                TokenKind::Star => "*",
                TokenKind::Slash => "/",
                TokenKind::Percent => "%",
                _ => break,
            };
            self.advance();
            left = Expr::BinOp(Box::new(left), op.into(), Box::new(self.parse_atom()));
        }
        left
    }
    fn parse_atom(&mut self) -> Expr {
        let ln = self.line();
        match self.advance() {
            TokenKind::Number(n) => Expr::Number(n),
            TokenKind::StrLit(s) => Expr::Str(s),
            TokenKind::Bang => Expr::UnaryNot(Box::new(self.parse_atom())),
            TokenKind::Ident(name) => {
                if *self.peek() == TokenKind::LParen {
                    self.advance();
                    let mut args = Vec::new();
                    while *self.peek() != TokenKind::RParen && *self.peek() != TokenKind::EOF {
                        if *self.peek() == TokenKind::Comma {
                            self.advance();
                            continue;
                        }
                        args.push(self.parse_logic_or());
                    }
                    if *self.peek() == TokenKind::RParen {
                        self.advance();
                    }
                    Expr::Call(name, args)
                } else if *self.peek() == TokenKind::LBrace {
                    self.advance();
                    let mut inits = Vec::new();
                    while *self.peek() != TokenKind::RBrace && *self.peek() != TokenKind::EOF {
                        if *self.peek() == TokenKind::Comma {
                            self.advance();
                            continue;
                        }
                        let fname = match self.advance() {
                            TokenKind::Ident(f) => f,
                            _ => break,
                        };
                        if *self.peek() == TokenKind::Colon {
                            self.advance();
                        }
                        let val = self.parse_logic_or();
                        inits.push((fname, val));
                        if *self.peek() == TokenKind::Comma {
                            self.advance();
                        }
                    }
                    if *self.peek() == TokenKind::RBrace {
                        self.advance();
                    }
                    Expr::StructInit(name, inits)
                } else if *self.peek() == TokenKind::Dot {
                    self.advance();
                    let field = match self.advance() {
                        TokenKind::Ident(f) => f,
                        _ => panic!("ожидалось имя поля после ."),
                    };
                    Expr::Field(name, field)
                } else if *self.peek() == TokenKind::LBracket {
                    self.advance();
                    let idx = self.parse_expr();
                    if *self.peek() == TokenKind::RBracket {
                        self.advance();
                    }
                    Expr::Index(name, Box::new(idx))
                } else {
                    Expr::Var(name)
                }
            }
            TokenKind::LParen => {
                let e = self.parse_logic_or();
                if *self.peek() == TokenKind::RParen {
                    self.advance();
                }
                e
            }
            TokenKind::Minus => match self.parse_atom() {
                Expr::Number(n) => Expr::Number(-n),
                e => Expr::BinOp(Box::new(Expr::Number(0)), "-".into(), Box::new(e)),
            },
            other => panic!(
                "[VETO | строка {}]: ожидалось выражение, найдено {:?}",
                ln, other
            ),
        }
    }
}
pub fn token_to_source(token: &TokenKind) -> String {
    match token {
        TokenKind::Ident(name) => name.clone(),
        TokenKind::Number(n) => n.to_string(),
        TokenKind::StrLit(s) => format!("\"{}\"", s),
        TokenKind::Comma => ",".into(),
        TokenKind::Dot => ".".into(),
        TokenKind::Colon => ":".into(),
        TokenKind::ColonColon => "::".into(),
        TokenKind::Semicolon => ";".into(),
        TokenKind::Assign => "=".into(),
        TokenKind::PlusEq => "+=".into(),
        TokenKind::MinusEq => "-=".into(),
        TokenKind::AmpEq => "&=".into(),
        TokenKind::PipeEq => "|=".into(),
        TokenKind::Bang => "!".into(),
        TokenKind::Less => "<".into(),
        TokenKind::Greater => ">".into(),
        TokenKind::Shl => "<<".into(),
        TokenKind::Shr => ">>".into(),
        TokenKind::Amp => "&".into(),
        TokenKind::Pipe => "|".into(),
        TokenKind::Caret => "^".into(),
        TokenKind::AndAnd => "&&".into(),
        TokenKind::OrOr => "||".into(),
        TokenKind::DotDot => "..".into(),
        TokenKind::LParen => "(".into(),
        TokenKind::RParen => ")".into(),
        TokenKind::LBrace => "{".into(),
        TokenKind::RBrace => "}".into(),
        TokenKind::LBracket => "[".into(),
        TokenKind::RBracket => "]".into(),
        TokenKind::Plus => "+".into(),
        TokenKind::Minus => "-".into(),
        TokenKind::Star => "*".into(),
        TokenKind::Slash => "/".into(),
        TokenKind::Percent => "%".into(),
        TokenKind::Arrow => "->".into(),
        TokenKind::Fn => "fn".into(),
        TokenKind::Struct => "struct".into(),
        TokenKind::Let => "let".into(),
        TokenKind::Mut => "mut".into(),
        TokenKind::Const => "const".into(),
        TokenKind::If => "if".into(),
        TokenKind::Else => "else".into(),
        TokenKind::For => "for".into(),
        TokenKind::In => "in".into(),
        TokenKind::Loop => "loop".into(),
        TokenKind::While => "while".into(),
        TokenKind::Break => "break".into(),
        TokenKind::Continue => "continue".into(),
        TokenKind::Use => "use".into(),
        TokenKind::Simd => "simd".into(),
        TokenKind::Prove => "prove".into(),
        TokenKind::Raw => "raw".into(),
        TokenKind::Naked => "naked".into(),
        TokenKind::Return => "return".into(),
        TokenKind::EOF => "".into(),
    }
}
