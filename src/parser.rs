use crate::ast::*;
use crate::error::{ErrorBag, Stage};
use crate::lexer::{Lexer, Token, TokenKind};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    file: String,
    base_dir: String,
    imported: HashSet<String>,
    no_struct_init: bool,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            pos: 0,
            file: "<input>".into(),
            base_dir: ".".into(),
            imported: HashSet::new(),
            no_struct_init: false,
        }
    }

    pub fn with_context(tokens: Vec<Token>, file: &str, base_dir: &str) -> Self {
        Self {
            tokens,
            pos: 0,
            file: file.into(),
            base_dir: base_dir.into(),
            imported: HashSet::new(),
            no_struct_init: false,
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

    fn expect(&mut self, expected: TokenKind, msg: &str, bag: &mut ErrorBag) -> bool {
        if *self.peek() == expected {
            self.advance();
            true
        } else {
            bag.push(
                Stage::Parser,
                &self.file,
                self.line(),
                format!("{}: found {:?}", msg, self.peek()),
            );
            false
        }
    }

    fn parse_attrs(&mut self, bag: &mut ErrorBag) -> StrList {
        let mut attrs = Vec::new();
        while *self.peek() == TokenKind::Hash {
            self.advance();
            self.expect(TokenKind::LBracket, "expected '[' after '#'", bag);
            if let TokenKind::Ident(a) = self.advance() {
                if *self.peek() == TokenKind::LParen {
                    self.advance();
                    if matches!(a.as_str(), "requires" | "ensures") {
                        let mut states = Vec::new();
                        while *self.peek() != TokenKind::RParen && *self.peek() != TokenKind::EOF {
                            if *self.peek() == TokenKind::Comma {
                                self.advance();
                                continue;
                            }
                            if let TokenKind::Ident(st) = self.advance() {
                                states.push(st);
                            }
                        }
                        self.expect(TokenKind::RParen, "expected ')' after state list", bag);
                        attrs.push(format!("{}:{}", a, states.join(",")));
                        self.expect(
                            TokenKind::RBracket,
                            "expected ']' after attribute name",
                            bag,
                        );
                        continue;
                    }
                    let lo = self.parse_signed().unwrap_or(0);
                    let hi = if *self.peek() == TokenKind::DotDot {
                        self.advance();
                        self.parse_signed().unwrap_or(lo)
                    } else {
                        lo
                    };
                    self.expect(TokenKind::RParen, "expected ')' after attribute range", bag);
                    attrs.push(format!("{}:{}..{}", a, lo, hi));
                } else {
                    attrs.push(a);
                }
            }
            self.expect(
                TokenKind::RBracket,
                "expected ']' after attribute name",
                bag,
            );
        }
        attrs
    }

    pub fn parse_main(&mut self, bag: &mut ErrorBag) -> Program {
        let mut prog = Program::default();
        let mut pending_attrs: StrList = Vec::new();
        while *self.peek() != TokenKind::EOF {
            if *self.peek() == TokenKind::Hash {
                pending_attrs.extend(self.parse_attrs(bag));
                continue;
            }
            match self.peek() {
                TokenKind::Mod => {
                    pending_attrs.clear();
                    self.advance();
                    let mod_name = match self.advance() {
                        TokenKind::Ident(n) => n,
                        _ => "_mod".into(),
                    };
                    self.expect(TokenKind::LBrace, "expected '{' after module name", bag);
                    let mut sub_tokens = Vec::new();
                    let mut depth = 1usize;
                    while depth > 0 && *self.peek() != TokenKind::EOF {
                        let tok = self.tokens[self.pos].clone();
                        self.advance();
                        match tok.kind {
                            TokenKind::LBrace => {
                                depth += 1;
                                sub_tokens.push(tok);
                            }
                            TokenKind::RBrace => {
                                depth -= 1;
                                if depth > 0 {
                                    sub_tokens.push(tok);
                                }
                            }
                            _ => sub_tokens.push(tok),
                        }
                    }
                    let mut mod_parser =
                        Parser::with_context(sub_tokens, &self.file, &self.base_dir);
                    let mod_prog = mod_parser.parse_main(bag);
                    for mut f in mod_prog.functions {
                        f.name = format!("{}__{}", mod_name, f.name);
                        prog.functions.push(f);
                    }
                    prog.structs.extend(mod_prog.structs);
                    prog.enums.extend(mod_prog.enums);
                    prog.consts.extend(mod_prog.consts);
                }
                TokenKind::Use => {
                    pending_attrs.clear();
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
                            match fs::read_to_string(&full_path) {
                                Ok(sub_src) => {
                                    let mut sub_lex = Lexer::new(&sub_src);
                                    let sub_tokens = sub_lex.tokenize();
                                    let sub_dir =
                                        full_path.parent().and_then(|p| p.to_str()).unwrap_or(".");
                                    let mut sub_parser = Parser {
                                        tokens: sub_tokens,
                                        pos: 0,
                                        file: rel_path.clone(),
                                        base_dir: sub_dir.into(),
                                        imported: self.imported.clone(),
                                        no_struct_init: false,
                                    };
                                    let sub_prog = sub_parser.parse_main(bag);
                                    self.imported = sub_parser.imported;
                                    prog.enums.extend(sub_prog.enums);
                                    prog.structs.extend(sub_prog.structs);
                                    prog.consts.extend(sub_prog.consts);
                                    prog.functions.extend(sub_prog.functions);
                                }
                                Err(e) => {
                                    bag.push(
                                        Stage::Parser,
                                        &self.file,
                                        ln,
                                        format!("cannot import '{}': {}", rel_path, e),
                                    );
                                }
                            }
                        }
                    } else {
                        bag.push(
                            Stage::Parser,
                            &self.file,
                            ln,
                            "expected quoted path after 'use'",
                        );
                        while *self.peek() != TokenKind::Semicolon && *self.peek() != TokenKind::EOF
                        {
                            self.advance();
                        }
                        if *self.peek() == TokenKind::Semicolon {
                            self.advance();
                        }
                    }
                }
                TokenKind::Enum => {
                    pending_attrs.clear();
                    if let Some(en) = self.parse_enum(bag) {
                        prog.enums.push(en);
                    }
                }
                TokenKind::Impl => {
                    pending_attrs.clear();
                    let ln = self.line();
                    self.advance();
                    let target_type = match self.advance() {
                        TokenKind::Ident(n) => n,
                        other => {
                            bag.push(
                                Stage::Parser,
                                &self.file,
                                ln,
                                format!("expected type name after impl, found {:?}", other),
                            );
                            "_err".into()
                        }
                    };
                    self.skip_generics();
                    self.expect(
                        TokenKind::LBrace,
                        "expected '{' after type name in impl",
                        bag,
                    );
                    while *self.peek() != TokenKind::RBrace && *self.peek() != TokenKind::EOF {
                        let m_attrs = if *self.peek() == TokenKind::Hash {
                            self.parse_attrs(bag)
                        } else {
                            Vec::new()
                        };
                        if *self.peek() == TokenKind::Fn {
                            if let Some(mut m) = self.parse_method(&target_type, bag) {
                                m.attrs = m_attrs;
                                prog.functions.push(m);
                            }
                        } else {
                            self.advance();
                        }
                    }
                    self.expect(TokenKind::RBrace, "expected '}' at end of impl block", bag);
                }
                TokenKind::BitStruct => {
                    let mut attrs = std::mem::take(&mut pending_attrs);
                    attrs.push("packed".into());
                    if let Some(mut st) = self.parse_bitstruct(bag) {
                        st.attrs = attrs;
                        prog.structs.push(st);
                    }
                }
                TokenKind::Arena => {
                    let ln = self.line();
                    self.advance();
                    let aname = match self.advance() {
                        TokenKind::Ident(n) => n,
                        _ => "_err".into(),
                    };
                    self.expect(TokenKind::Less, "expected '<' after arena name", bag);
                    let etype = match self.advance() {
                        TokenKind::Ident(t) => t,
                        _ => "i32".into(),
                    };
                    self.expect(TokenKind::Comma, "expected ',' in arena definition", bag);
                    let cap = match self.advance() {
                        TokenKind::Number(n) if n > 0 => n as usize,
                        _ => 64,
                    };
                    self.expect(TokenKind::Greater, "expected '>' in arena definition", bag);
                    if *self.peek() == TokenKind::Semicolon {
                        self.advance();
                    }
                    prog.arenas.push(ArenaDef {
                        line: ln,
                        name: aname,
                        elem_type: etype,
                        capacity: cap,
                    });
                }
                TokenKind::Struct => {
                    let attrs = std::mem::take(&mut pending_attrs);
                    if let Some(mut st) = self.parse_struct(bag) {
                        st.attrs = attrs;
                        prog.structs.push(st);
                    }
                }
                TokenKind::Const => {
                    pending_attrs.clear();
                    if let Some(c) = self.parse_const(bag) {
                        prog.consts.push(c);
                    }
                }
                TokenKind::Naked | TokenKind::Comptime => {
                    self.advance();
                }
                TokenKind::Trait => {
                    self.advance();
                    let _ = self.advance();
                    if *self.peek() == TokenKind::LBrace {
                        self.advance();
                        let mut depth = 1usize;
                        while depth > 0 && *self.peek() != TokenKind::EOF {
                            match self.advance() {
                                TokenKind::LBrace => depth += 1,
                                TokenKind::RBrace => depth -= 1,
                                _ => {}
                            }
                        }
                    }
                }
                TokenKind::Fn => {
                    let attrs = std::mem::take(&mut pending_attrs);
                    if let Some(mut f) = self.parse_function(bag) {
                        f.attrs = attrs;
                        prog.functions.push(f);
                    }
                }
                other => {
                    let ln = self.line();
                    bag.push(
                        Stage::Parser,
                        &self.file,
                        ln,
                        format!("unexpected top-level token: {:?}", other),
                    );
                    self.advance();
                }
            }
        }
        prog
    }
    fn parse_enum(&mut self, bag: &mut ErrorBag) -> Option<EnumDef> {
        let line = self.line();
        self.advance();
        let name = match self.advance() {
            TokenKind::Ident(n) => n,
            other => {
                bag.push(
                    Stage::Parser,
                    &self.file,
                    line,
                    format!("expected enum name, found {:?}", other),
                );
                return None;
            }
        };
        self.skip_generics();
        self.expect(TokenKind::LBrace, "expected '{' after enum name", bag);
        let mut variants = Vec::new();
        let mut next_val: i64 = 0;
        while *self.peek() != TokenKind::RBrace && *self.peek() != TokenKind::EOF {
            if *self.peek() == TokenKind::Comma {
                self.advance();
                continue;
            }
            let vname = match self.advance() {
                TokenKind::Ident(n) => n,
                other => {
                    bag.push(
                        Stage::Parser,
                        &self.file,
                        self.line(),
                        format!("expected enum variant name, found {:?}", other),
                    );
                    break;
                }
            };
            let mut payload_range = None;
            if *self.peek() == TokenKind::LParen {
                self.advance();
                let ptype = match self.advance() {
                    TokenKind::Ident(t) => t,
                    _ => "i32".into(),
                };
                let (mut pmin, mut pmax) = self.parse_range();
                if pmin == i64::MIN && pmax == i64::MAX {
                    if let Some((dmin, dmax)) = Self::default_type_bounds(&ptype) {
                        pmin = dmin;
                        pmax = dmax;
                    }
                }
                self.expect(
                    TokenKind::RParen,
                    "expected ')' after enum variant type",
                    bag,
                );
                payload_range = Some((pmin, pmax));
            } else if *self.peek() == TokenKind::Assign {
                self.advance();
                if let Some(v) = self.parse_signed() {
                    next_val = v;
                }
            }
            variants.push((vname, next_val, payload_range));
            next_val += 1;
            if *self.peek() == TokenKind::Comma {
                self.advance();
            }
        }
        self.expect(TokenKind::RBrace, "expected '}' at end of enum", bag);
        Some(EnumDef {
            line,
            name,
            variants,
        })
    }
    fn parse_bitstruct(&mut self, bag: &mut ErrorBag) -> Option<StructDef> {
        let line = self.line();
        self.advance();
        let name = match self.advance() {
            TokenKind::Ident(n) => n,
            _ => return None,
        };
        let mut max_bits: u32 = 32;
        if *self.peek() == TokenKind::Colon {
            self.advance();
            if let TokenKind::Ident(bt) = self.advance() {
                max_bits = match bt.as_str() {
                    "u8" => 8,
                    "u16" => 16,
                    "u32" => 32,
                    _ => 32,
                };
            }
        }
        self.expect(TokenKind::LBrace, "expected '{' in bitstruct", bag);
        let mut fields = Vec::new();
        let mut total_bits: u32 = 0;
        while *self.peek() != TokenKind::RBrace && *self.peek() != TokenKind::EOF {
            if *self.peek() == TokenKind::Comma {
                self.advance();
                continue;
            }
            let fname = match self.advance() {
                TokenKind::Ident(n) => n,
                _ => break,
            };
            self.expect(TokenKind::Colon, "expected ':' after bitfield name", bag);
            let ftype = match self.advance() {
                TokenKind::Ident(t) => t,
                _ => "u1".into(),
            };
            let bits: u8 = ftype
                .strip_prefix('u')
                .and_then(|s| s.parse().ok())
                .unwrap_or(1);
            total_bits += bits as u32;
            let auto_max = if bits >= 31 {
                i32::MAX as i64
            } else {
                (1i64 << bits) - 1
            };
            let (mut min_val, mut max_val) = self.parse_range();
            if min_val == i64::MIN && max_val == i64::MAX {
                min_val = 0;
                max_val = auto_max;
            }
            fields.push(FieldDef {
                name: fname,
                bit_width: Some(bits),
                min_val,
                max_val,
            });
            if *self.peek() == TokenKind::Comma {
                self.advance();
            }
        }
        if total_bits > max_bits {
            bag.push(
                Stage::Parser,
                &self.file,
                line,
                format!(
                    "Total bitfield width ({} bits) exceeds register size ({} bits)!",
                    total_bits, max_bits
                ),
            );
        }
        self.expect(TokenKind::RBrace, "expected '}' at end of bitstruct", bag);
        Some(StructDef {
            line,
            attrs: Vec::new(),
            name,
            is_bitstruct: true,
            fields,
        })
    }
    fn parse_struct(&mut self, bag: &mut ErrorBag) -> Option<StructDef> {
        let line = self.line();
        self.advance();
        let name = match self.advance() {
            TokenKind::Ident(n) => n,
            other => {
                bag.push(
                    Stage::Parser,
                    &self.file,
                    line,
                    format!("expected struct name, found {:?}", other),
                );
                return None;
            }
        };
        self.skip_generics();
        self.expect(TokenKind::LBrace, "expected '{' after struct name", bag);
        let mut fields = Vec::new();
        while *self.peek() != TokenKind::RBrace && *self.peek() != TokenKind::EOF {
            if *self.peek() == TokenKind::Comma {
                self.advance();
                continue;
            }
            let fname = match self.advance() {
                TokenKind::Ident(n) => n,
                other => {
                    bag.push(
                        Stage::Parser,
                        &self.file,
                        self.line(),
                        format!("expected struct field name, found {:?}", other),
                    );
                    break;
                }
            };
            self.expect(TokenKind::Colon, "expected ':' after field name", bag);
            let ftype = match self.advance() {
                TokenKind::Ident(t) => t,
                _ => "i32".into(),
            };
            let (mut min_val, mut max_val) = self.parse_range();
            if min_val == i64::MIN && max_val == i64::MAX {
                if let Some((dmin, dmax)) = Self::default_type_bounds(&ftype) {
                    min_val = dmin;
                    max_val = dmax;
                }
            }
            fields.push(FieldDef {
                name: fname,
                bit_width: None,
                min_val,
                max_val,
            });
            if *self.peek() == TokenKind::Comma {
                self.advance();
            }
        }
        self.expect(TokenKind::RBrace, "expected '}' at end of struct", bag);
        Some(StructDef {
            line,
            attrs: Vec::new(),
            name,
            is_bitstruct: false,
            fields,
        })
    }
    fn skip_generics(&mut self) {
        if *self.peek() == TokenKind::Less {
            let mut depth = 0usize;
            while *self.peek() != TokenKind::EOF {
                match self.peek() {
                    TokenKind::Less => {
                        depth += 1;
                        self.advance();
                    }
                    TokenKind::Greater => {
                        self.advance();
                        depth = depth.saturating_sub(1);
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {
                        self.advance();
                    }
                }
            }
        }
    }
    fn default_type_bounds(type_name: &str) -> Option<(i64, i64)> {
        match type_name {
            "bool" => Some((0, 1)),
            "i8" => Some((i8::MIN as i64, i8::MAX as i64)),
            "u8" => Some((0, u8::MAX as i64)),
            "i16" => Some((i16::MIN as i64, i16::MAX as i64)),
            "u16" => Some((0, u16::MAX as i64)),
            "i32" => Some((i32::MIN as i64, i32::MAX as i64)),
            "u32" => Some((0, u32::MAX as i64)),
            "i64" | "u64" | "usize" => Some((i64::MIN, i64::MAX)),
            _ => None,
        }
    }
    fn parse_range(&mut self) -> (i64, i64) {
        let mut min_val = i64::MIN;
        let mut max_val = i64::MAX;
        if *self.peek() == TokenKind::Less {
            self.advance();
            if matches!(self.peek(), TokenKind::Ident(_)) {
                let inner_t = match self.advance() {
                    TokenKind::Ident(t) => t,
                    _ => "i32".into(),
                };
                let (imin, mut imax) = self.parse_range();
                if imin == i64::MIN && imax == i64::MAX {
                    if let Some((_, dmax)) = Self::default_type_bounds(&inner_t) {
                        imax = dmax;
                    }
                }
                if *self.peek() == TokenKind::Comma {
                    self.advance();
                    let _ = self.advance();
                    let _ = self.parse_range();
                }
                if *self.peek() == TokenKind::Greater {
                    self.advance();
                } else if *self.peek() == TokenKind::Shr {
                    self.tokens[self.pos].kind = TokenKind::Greater;
                }
                return (-1, imax);
            }
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
            } else if *self.peek() == TokenKind::Shr {
                self.tokens[self.pos].kind = TokenKind::Greater;
            }
        }
        (min_val, max_val)
    }
    fn parse_const(&mut self, bag: &mut ErrorBag) -> Option<ConstDef> {
        let line = self.line();
        self.advance();
        let name = match self.advance() {
            TokenKind::Ident(n) => n,
            other => {
                bag.push(
                    Stage::Parser,
                    &self.file,
                    line,
                    format!("expected constant name, found {:?}", other),
                );
                return None;
            }
        };
        self.expect(TokenKind::Colon, "expected ':' after constant name", bag);
        self.advance();
        let (min_val, max_val) = self.parse_range();
        self.expect(
            TokenKind::Assign,
            "expected '=' in constant declaration",
            bag,
        );
        let value = self.parse_logic_or(bag);
        if *self.peek() == TokenKind::Semicolon {
            self.advance();
        }
        Some(ConstDef {
            line,
            name,
            min_val,
            max_val,
            value,
        })
    }
    fn parse_method(&mut self, self_type: &str, bag: &mut ErrorBag) -> Option<Function> {
        let line = self.line();
        self.advance();
        let mname = match self.advance() {
            TokenKind::Ident(n) => n,
            _ => return None,
        };
        self.skip_generics();
        let mangled = format!("{}__{}", self_type, mname);
        self.expect(TokenKind::LParen, "expected '(' after method name", bag);
        let mut params = Vec::new();
        if *self.peek() == TokenKind::Amp {
            self.advance();
            let mut is_mut = false;
            if *self.peek() == TokenKind::Mut {
                self.advance();
                is_mut = true;
            }
            if let TokenKind::Ident(ref s) = self.peek().clone() {
                if s == "self" {
                    self.advance();
                    params.push(Param {
                        name: "self".into(),
                        type_name: self_type.into(),
                        is_mut_ref: is_mut,
                        array_len: None,
                        min_val: i64::MIN,
                        max_val: i64::MAX,
                    });
                }
            }
        } else if let TokenKind::Ident(ref s) = self.peek().clone() {
            if s == "self" {
                self.advance();
                params.push(Param {
                    name: "self".into(),
                    type_name: self_type.into(),
                    is_mut_ref: false,
                    array_len: None,
                    min_val: i64::MIN,
                    max_val: i64::MAX,
                });
            }
        }
        while *self.peek() != TokenKind::RParen && *self.peek() != TokenKind::EOF {
            if *self.peek() == TokenKind::Comma {
                self.advance();
                continue;
            }
            let param_name = match self.advance() {
                TokenKind::Ident(n) => n,
                _ => break,
            };
            self.expect(TokenKind::Colon, "expected ':' after parameter name", bag);
            let mut is_mut_ref = false;
            if *self.peek() == TokenKind::Amp {
                self.advance();
                is_mut_ref = true;
                if *self.peek() == TokenKind::Mut {
                    self.advance();
                }
            }
            let type_name = match self.advance() {
                TokenKind::Ident(t) => t,
                _ => "i32".into(),
            };
            let (mut min_val, mut max_val) = self.parse_range();
            if min_val == i64::MIN && max_val == i64::MAX {
                if let Some((dmin, dmax)) = Self::default_type_bounds(&type_name) {
                    min_val = dmin;
                    max_val = dmax;
                }
            }
            params.push(Param {
                name: param_name,
                type_name,
                is_mut_ref,
                array_len: None,
                min_val,
                max_val,
            });
        }
        self.expect(
            TokenKind::RParen,
            "expected ')' after method parameters",
            bag,
        );
        let mut ret_type = "i32".to_string();
        let mut ret_min = i64::MIN;
        let mut ret_max = i64::MAX;
        if *self.peek() == TokenKind::Arrow {
            self.advance();
            if *self.peek() == TokenKind::LParen {
                self.advance();
                self.expect(TokenKind::RParen, "expected ')' in unit type '()'", bag);
                ret_type = "()".into();
                ret_min = 0;
                ret_max = 0;
            } else {
                if let TokenKind::Ident(rt) = self.advance() {
                    ret_type = rt;
                }
                let (mut r_min, mut r_max) = self.parse_range();
                if r_min == i64::MIN && r_max == i64::MAX {
                    if let Some((dmin, dmax)) = Self::default_type_bounds(&ret_type) {
                        r_min = dmin;
                        r_max = dmax;
                    }
                }
                ret_min = r_min;
                ret_max = r_max;
            }
        }
        let body = self.parse_block(bag);
        Some(Function {
            line,
            attrs: Vec::new(),
            name: mangled,
            params,
            ret_type,
            ret_min,
            ret_max,
            body,
        })
    }
    fn parse_function(&mut self, bag: &mut ErrorBag) -> Option<Function> {
        let line = self.line();
        self.advance();
        let name = match self.advance() {
            TokenKind::Ident(n) => n,
            other => {
                bag.push(
                    Stage::Parser,
                    &self.file,
                    line,
                    format!("expected function name, found {:?}", other),
                );
                return None;
            }
        };
        self.skip_generics();
        self.expect(TokenKind::LParen, "expected '(' after function name", bag);
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
            self.expect(TokenKind::Colon, "expected ':' after parameter name", bag);
            let mut is_mut_ref = false;
            let mut array_len = None;
            if *self.peek() == TokenKind::Amp {
                self.advance();
                is_mut_ref = true;
                if *self.peek() == TokenKind::Mut {
                    self.advance();
                }
            }
            if *self.peek() == TokenKind::Raw {
                self.advance();
            }
            if *self.peek() == TokenKind::LBracket {
                self.advance();
                let el_t = match self.advance() {
                    TokenKind::Ident(t) => t,
                    _ => "i32".into(),
                };
                let (mut min_val, mut max_val) = self.parse_range();
                if min_val == i64::MIN && max_val == i64::MAX {
                    if let Some((dmin, dmax)) = Self::default_type_bounds(&el_t) {
                        min_val = dmin;
                        max_val = dmax;
                    }
                }
                let len = if *self.peek() == TokenKind::Semicolon {
                    self.advance();
                    match self.advance() {
                        TokenKind::Number(n) if n > 0 => n as usize,
                        _ => 1,
                    }
                } else {
                    0
                };
                self.expect(TokenKind::RBracket, "expected ']' in array type", bag);
                array_len = Some(len);
                params.push(Param {
                    name: param_name,
                    type_name: el_t,
                    is_mut_ref,
                    array_len,
                    min_val,
                    max_val,
                });
                continue;
            }
            let type_name = match self.advance() {
                TokenKind::Ident(t) => t,
                _ => "i32".into(),
            };
            let (mut min_val, mut max_val) = self.parse_range();
            if min_val == i64::MIN && max_val == i64::MAX {
                if let Some((dmin, dmax)) = Self::default_type_bounds(&type_name) {
                    min_val = dmin;
                    max_val = dmax;
                }
            }
            params.push(Param {
                name: param_name,
                type_name,
                is_mut_ref,
                array_len,
                min_val,
                max_val,
            });
        }
        self.expect(
            TokenKind::RParen,
            "expected ')' after function parameters",
            bag,
        );
        let mut ret_type = "i32".to_string();
        let mut ret_min = i64::MIN;
        let mut ret_max = i64::MAX;
        if *self.peek() == TokenKind::Arrow {
            self.advance();
            if *self.peek() == TokenKind::LParen {
                self.advance();
                self.expect(TokenKind::RParen, "expected ')' in unit type '()'", bag);
                ret_type = "()".into();
                ret_min = 0;
                ret_max = 0;
            } else {
                if let TokenKind::Ident(rt) = self.advance() {
                    ret_type = rt;
                }
                let (mut r_min, mut r_max) = self.parse_range();
                if r_min == i64::MIN && r_max == i64::MAX {
                    if let Some((dmin, dmax)) = Self::default_type_bounds(&ret_type) {
                        r_min = dmin;
                        r_max = dmax;
                    }
                }
                ret_min = r_min;
                ret_max = r_max;
            }
        }
        let body = self.parse_block(bag);
        Some(Function {
            line,
            attrs: Vec::new(),
            name,
            params,
            ret_type,
            ret_min,
            ret_max,
            body,
        })
    }
    fn parse_block(&mut self, bag: &mut ErrorBag) -> StmtList {
        self.expect(TokenKind::LBrace, "expected '{'", bag);
        let mut body = Vec::new();
        while *self.peek() != TokenKind::RBrace && *self.peek() != TokenKind::EOF {
            let ln = self.line();
            match self.peek() {
                TokenKind::Let => {
                    if let Some(st) = self.parse_let(bag) {
                        body.push(st);
                    }
                }
                TokenKind::If => body.push(self.parse_if(bag)),
                TokenKind::Match => body.push(self.parse_match(bag)),
                TokenKind::Loop => {
                    self.advance();
                    body.push(Stmt::Loop {
                        line: ln,
                        body: self.parse_block(bag),
                    });
                }
                TokenKind::While => {
                    self.advance();
                    self.no_struct_init = true;
                    let cond = self.parse_logic_or(bag);
                    self.no_struct_init = false;
                    body.push(Stmt::While {
                        line: ln,
                        cond,
                        body: self.parse_block(bag),
                    });
                }
                TokenKind::For => {
                    self.advance();
                    let var = match self.advance() {
                        TokenKind::Ident(n) => n,
                        other => {
                            bag.push(
                                Stage::Parser,
                                &self.file,
                                ln,
                                format!("expected loop variable name in for, found {:?}", other),
                            );
                            "_err".into()
                        }
                    };
                    self.expect(TokenKind::In, "expected 'in' in for loop", bag);
                    let start = self.parse_expr(bag);
                    self.expect(TokenKind::DotDot, "expected '..' in for range", bag);
                    let end = self.parse_expr(bag);
                    body.push(Stmt::For {
                        line: ln,
                        var,
                        start,
                        end,
                        body: self.parse_block(bag),
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
                TokenKind::Ident(ref id) if id == "asm" && *self.peek_next() == TokenKind::Bang => {
                    self.advance();
                    self.advance();
                    self.expect(TokenKind::LParen, "expected '(' after asm!", bag);
                    let template = match self.advance() {
                        TokenKind::StrLit(s) => s,
                        _ => "".into(),
                    };
                    let mut clobbers = Vec::new();
                    while *self.peek() != TokenKind::RParen && *self.peek() != TokenKind::EOF {
                        if *self.peek() == TokenKind::Comma {
                            self.advance();
                            continue;
                        }
                        if let TokenKind::Ident(key) = self.advance() {
                            if *self.peek() == TokenKind::Colon {
                                self.advance();
                            }
                            if let TokenKind::StrLit(val) = self.advance() {
                                if key == "clobbers" {
                                    for c in val
                                        .split(',')
                                        .map(|s| s.trim().to_string())
                                        .filter(|s| !s.is_empty())
                                    {
                                        clobbers.push(c);
                                    }
                                }
                            }
                        }
                    }
                    self.expect(TokenKind::RParen, "expected ')' at end of asm!", bag);
                    if *self.peek() == TokenKind::Semicolon {
                        self.advance();
                    }
                    body.push(Stmt::InlineAsm {
                        line: ln,
                        template,
                        clobbers,
                    });
                }
                TokenKind::Prove => body.push(self.parse_prove()),
                TokenKind::Return => body.push(self.parse_return(bag)),
                TokenKind::Ident(..) if *self.peek_next() == TokenKind::Dot => {
                    let obj = match self.advance() {
                        TokenKind::Ident(n) => n,
                        _ => unreachable!(),
                    };
                    self.advance();
                    let field = match self.advance() {
                        TokenKind::Ident(f) => f,
                        _ => "_err".into(),
                    };
                    if *self.peek() == TokenKind::LParen {
                        self.advance();
                        let mut args = Vec::new();
                        while *self.peek() != TokenKind::RParen && *self.peek() != TokenKind::EOF {
                            if *self.peek() == TokenKind::Comma {
                                self.advance();
                                continue;
                            }
                            args.push(self.parse_logic_or(bag));
                        }
                        self.expect(
                            TokenKind::RParen,
                            "expected ')' after method arguments",
                            bag,
                        );
                        if *self.peek() == TokenKind::Semicolon {
                            self.advance();
                        }
                        body.push(Stmt::ExprStmt {
                            line: ln,
                            expr: Expr::MethodCall(obj, field, args),
                        });
                        continue;
                    }
                    let op = self.advance();
                    let rhs = self.parse_logic_or(bag);
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
                TokenKind::Ident(..)
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
                    let rhs = self.parse_logic_or(bag);
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
                    let index = self.parse_expr(bag);
                    self.expect(TokenKind::RBracket, "expected ']'", bag);
                    let op = self.advance();
                    let rhs = self.parse_logic_or(bag);
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
                    let expr = self.parse_expr(bag);
                    if *self.peek() == TokenKind::Semicolon {
                        self.advance();
                    }
                    body.push(Stmt::ExprStmt { line: ln, expr });
                }
                other => {
                    bag.push(
                        Stage::Parser,
                        &self.file,
                        ln,
                        format!("unexpected statement in block: {:?}", other),
                    );
                    self.advance();
                }
            }
        }
        self.expect(TokenKind::RBrace, "expected '}'", bag);
        body
    }
    fn parse_match(&mut self, bag: &mut ErrorBag) -> Stmt {
        let line = self.line();
        self.advance();
        self.no_struct_init = true;
        let expr = self.parse_logic_or(bag);
        self.no_struct_init = false;
        self.expect(
            TokenKind::LBrace,
            "expected '{' after match expression",
            bag,
        );
        let mut arms = Vec::new();
        while *self.peek() != TokenKind::RBrace && *self.peek() != TokenKind::EOF {
            if *self.peek() == TokenKind::Comma {
                self.advance();
                continue;
            }
            let arm_line = self.line();
            let pat = match self.peek().clone() {
                TokenKind::Ident(ref id) if id == "_" => {
                    self.advance();
                    MatchPat::Wildcard
                }
                TokenKind::Ident(en) if *self.peek_next() == TokenKind::ColonColon => {
                    self.advance();
                    self.advance();
                    let var = match self.advance() {
                        TokenKind::Ident(v) => v,
                        _ => "err".into(),
                    };
                    let bind = if *self.peek() == TokenKind::LParen {
                        self.advance();
                        let b = match self.advance() {
                            TokenKind::Ident(id) => Some(id),
                            _ => None,
                        };
                        self.expect(TokenKind::RParen, "expected ')' in match pattern", bag);
                        b
                    } else {
                        None
                    };
                    MatchPat::EnumVal(en, var, bind)
                }
                TokenKind::Number(_) | TokenKind::Minus => {
                    let lo = self.parse_signed().unwrap_or(0);
                    if *self.peek() == TokenKind::DotDot {
                        self.advance();
                        let hi = self.parse_signed().unwrap_or(lo);
                        MatchPat::Range(lo, hi)
                    } else {
                        MatchPat::Num(lo)
                    }
                }
                other => {
                    bag.push(
                        Stage::Parser,
                        &self.file,
                        arm_line,
                        format!("expected match pattern, found {:?}", other),
                    );
                    self.advance();
                    MatchPat::Wildcard
                }
            };
            self.expect(
                TokenKind::FatArrow,
                "expected '=>' after match pattern",
                bag,
            );
            let body = if *self.peek() == TokenKind::LBrace {
                self.parse_block(bag)
            } else {
                let ln = self.line();
                let st = if *self.peek() == TokenKind::Return {
                    vec![self.parse_return(bag)]
                } else {
                    vec![Stmt::ExprStmt {
                        line: ln,
                        expr: self.parse_logic_or(bag),
                    }]
                };
                if *self.peek() == TokenKind::Comma {
                    self.advance();
                }
                st
            };
            arms.push(MatchArm {
                line: arm_line,
                pat,
                body,
            });
        }
        self.expect(TokenKind::RBrace, "expected '}' at end of match", bag);
        Stmt::Match { line, expr, arms }
    }
    fn parse_if(&mut self, bag: &mut ErrorBag) -> Stmt {
        let line = self.line();
        self.advance();
        self.no_struct_init = true;
        let cond = self.parse_logic_or(bag);
        self.no_struct_init = false;
        let then_body = self.parse_block(bag);
        let else_body = if *self.peek() == TokenKind::Else {
            self.advance();
            if *self.peek() == TokenKind::If {
                vec![self.parse_if(bag)]
            } else {
                self.parse_block(bag)
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
    fn parse_let(&mut self, bag: &mut ErrorBag) -> Option<Stmt> {
        let line = self.line();
        self.advance();
        if *self.peek() == TokenKind::Mut {
            self.advance();
        }
        let name = match self.advance() {
            TokenKind::Ident(n) => n,
            other => {
                bag.push(
                    Stage::Parser,
                    &self.file,
                    line,
                    format!("expected variable name after let, found {:?}", other),
                );
                return None;
            }
        };
        self.expect(TokenKind::Colon, "expected ':' after variable name", bag);
        if *self.peek() == TokenKind::LBracket {
            self.advance();
            let el_t = match self.advance() {
                TokenKind::Ident(t) => t,
                _ => "i32".into(),
            };
            let (mut min_val, mut max_val) = self.parse_range();
            if min_val == i64::MIN && max_val == i64::MAX {
                if let Some((dmin, dmax)) = Self::default_type_bounds(&el_t) {
                    min_val = dmin;
                    max_val = dmax;
                }
            }
            if *self.peek() == TokenKind::Semicolon {
                self.advance();
            }
            let len = match self.advance() {
                TokenKind::Number(n) if n > 0 => n as usize,
                other => {
                    bag.push(
                        Stage::Parser,
                        &self.file,
                        line,
                        format!("expected array length > 0, found {:?}", other),
                    );
                    1
                }
            };
            self.expect(TokenKind::RBracket, "expected ']' in array type", bag);
            self.expect(
                TokenKind::Assign,
                "expected '=' in array initialization",
                bag,
            );
            self.expect(TokenKind::LBracket, "expected '[' in array literal", bag);
            let mut elements = Vec::new();
            if *self.peek() != TokenKind::RBracket {
                let first = self.parse_logic_or(bag);
                if *self.peek() == TokenKind::Semicolon {
                    self.advance();
                    let count = match self.advance() {
                        TokenKind::Number(n) if n > 0 => n as usize,
                        _ => 1,
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
                        elements.push(self.parse_logic_or(bag));
                    }
                }
            }
            self.expect(TokenKind::RBracket, "expected ']' at end of array", bag);
            if *self.peek() == TokenKind::Semicolon {
                self.advance();
            }
            return Some(Stmt::LetArray {
                line,
                name,
                min_val,
                max_val,
                len,
                elements,
            });
        }
        let type_name = match self.advance() {
            TokenKind::Ident(t) => t,
            _ => "i32".into(),
        };
        let (mut min_val, mut max_val) = self.parse_range();
        if min_val == i64::MIN && max_val == i64::MAX {
            if let Some((dmin, dmax)) = Self::default_type_bounds(&type_name) {
                min_val = dmin;
                max_val = dmax;
            }
        }
        self.expect(
            TokenKind::Assign,
            "expected '=' in variable declaration",
            bag,
        );
        let value = self.parse_logic_or(bag);
        if *self.peek() == TokenKind::Semicolon {
            self.advance();
        }
        Some(Stmt::Let {
            line,
            name,
            type_name,
            min_val,
            max_val,
            value,
        })
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
    fn parse_return(&mut self, bag: &mut ErrorBag) -> Stmt {
        let line = self.line();
        self.advance();
        if *self.peek() == TokenKind::Semicolon {
            self.advance();
            return Stmt::Return {
                line,
                expr: Expr::Number(0),
            };
        }
        let expr = self.parse_logic_or(bag);
        if *self.peek() == TokenKind::Semicolon {
            self.advance();
        }
        Stmt::Return { line, expr }
    }
    fn parse_logic_or(&mut self, bag: &mut ErrorBag) -> Expr {
        let mut left = self.parse_logic_and(bag);
        while *self.peek() == TokenKind::OrOr {
            self.advance();
            left = Expr::BinOp(
                Box::new(left),
                "||".into(),
                Box::new(self.parse_logic_and(bag)),
            );
        }
        left
    }
    fn parse_logic_and(&mut self, bag: &mut ErrorBag) -> Expr {
        let mut left = self.parse_comparison(bag);
        while *self.peek() == TokenKind::AndAnd {
            self.advance();
            left = Expr::BinOp(
                Box::new(left),
                "&&".into(),
                Box::new(self.parse_comparison(bag)),
            );
        }
        left
    }
    fn parse_comparison(&mut self, bag: &mut ErrorBag) -> Expr {
        let left = self.parse_expr(bag);
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
        Expr::BinOp(Box::new(left), op.into(), Box::new(self.parse_expr(bag)))
    }
    fn parse_expr(&mut self, bag: &mut ErrorBag) -> Expr {
        let mut left = self.parse_shift(bag);
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
            left = Expr::BinOp(Box::new(left), op.into(), Box::new(self.parse_shift(bag)));
        }
        left
    }
    fn parse_shift(&mut self, bag: &mut ErrorBag) -> Expr {
        let mut left = self.parse_term(bag);
        loop {
            let op = match self.peek() {
                TokenKind::Shl => "<<",
                TokenKind::Shr => ">>",
                _ => break,
            };
            self.advance();
            left = Expr::BinOp(Box::new(left), op.into(), Box::new(self.parse_term(bag)));
        }
        left
    }
    fn parse_term(&mut self, bag: &mut ErrorBag) -> Expr {
        let mut left = self.parse_atom(bag);
        loop {
            let op = match self.peek() {
                TokenKind::Star => "*",
                TokenKind::Slash => "/",
                TokenKind::Percent => "%",
                _ => break,
            };
            self.advance();
            left = Expr::BinOp(Box::new(left), op.into(), Box::new(self.parse_atom(bag)));
        }
        left
    }
    fn parse_atom(&mut self, bag: &mut ErrorBag) -> Expr {
        let ln = self.line();
        match self.advance() {
            TokenKind::Number(n) => Expr::Number(n),
            TokenKind::StrLit(s) => Expr::Str(s),
            TokenKind::Comptime => self.parse_expr(bag),
            TokenKind::Bang => Expr::UnaryNot(Box::new(self.parse_atom(bag))),
            TokenKind::Amp => {
                if let TokenKind::Mut = self.peek() {
                    self.advance();
                }
                self.parse_atom(bag)
            }
            TokenKind::Ident(name) => {
                if *self.peek() == TokenKind::ColonColon {
                    self.advance();
                    let vname = match self.advance() {
                        TokenKind::Ident(v) => v,
                        _ => "_err".into(),
                    };
                    let payload = if *self.peek() == TokenKind::LParen {
                        self.advance();
                        let e = self.parse_logic_or(bag);
                        self.expect(TokenKind::RParen, "expected ')' after enum argument", bag);
                        Some(Box::new(e))
                    } else {
                        None
                    };
                    return Expr::EnumVal(name, vname, payload);
                }
                if *self.peek() == TokenKind::LParen {
                    self.advance();
                    let mut args = Vec::new();
                    while *self.peek() != TokenKind::RParen && *self.peek() != TokenKind::EOF {
                        if *self.peek() == TokenKind::Comma {
                            self.advance();
                            continue;
                        }
                        args.push(self.parse_logic_or(bag));
                    }
                    self.expect(TokenKind::RParen, "expected ')' after arguments", bag);
                    Expr::Call(name, args)
                } else if *self.peek() == TokenKind::LBrace && !self.no_struct_init {
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
                        self.expect(
                            TokenKind::Colon,
                            "expected ':' in field initialization",
                            bag,
                        );
                        let val = self.parse_logic_or(bag);
                        inits.push((fname, val));
                        if *self.peek() == TokenKind::Comma {
                            self.advance();
                        }
                    }
                    self.expect(TokenKind::RBrace, "expected '}' at end of struct", bag);
                    Expr::StructInit(name, inits)
                } else if *self.peek() == TokenKind::Dot {
                    self.advance();
                    let field = match self.advance() {
                        TokenKind::Ident(f) => f,
                        other => {
                            bag.push(
                                Stage::Parser,
                                &self.file,
                                ln,
                                format!("expected field name after '.', found {:?}", other),
                            );
                            "_err".into()
                        }
                    };
                    if *self.peek() == TokenKind::LParen {
                        self.advance();
                        let mut args = Vec::new();
                        while *self.peek() != TokenKind::RParen && *self.peek() != TokenKind::EOF {
                            if *self.peek() == TokenKind::Comma {
                                self.advance();
                                continue;
                            }
                            args.push(self.parse_logic_or(bag));
                        }
                        self.expect(
                            TokenKind::RParen,
                            "expected ')' after method arguments",
                            bag,
                        );
                        Expr::MethodCall(name, field, args)
                    } else {
                        Expr::Field(name, field)
                    }
                } else if *self.peek() == TokenKind::LBracket {
                    self.advance();
                    let idx = self.parse_expr(bag);
                    self.expect(TokenKind::RBracket, "expected ']' in array indexing", bag);
                    Expr::Index(name, Box::new(idx))
                } else if name == "None" {
                    Expr::Number(-1)
                } else {
                    Expr::Var(name)
                }
            }
            TokenKind::LParen => {
                let e = self.parse_logic_or(bag);
                self.expect(TokenKind::RParen, "expected ')'", bag);
                e
            }
            TokenKind::Minus => match self.parse_atom(bag) {
                Expr::Number(n) => Expr::Number(-n),
                e => Expr::BinOp(Box::new(Expr::Number(0)), "-".into(), Box::new(e)),
            },
            TokenKind::BoolLit(b) => Expr::Number(if b { 1 } else { 0 }),
            other => {
                bag.push(
                    Stage::Parser,
                    &self.file,
                    ln,
                    format!("expected expression, found {:?}", other),
                );
                Expr::Number(0)
            }
        }
    }
}
pub fn token_to_source(token: &TokenKind) -> String {
    match token {
        TokenKind::Ident(name) => name.clone(),
        TokenKind::Number(n) => n.to_string(),
        TokenKind::StrLit(s) => format!("\"{}\"", s),
        TokenKind::Hash => "#".into(),
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
        TokenKind::FatArrow => "=>".into(),
        TokenKind::BoolLit(b) => b.to_string(),
        TokenKind::Unknown(c) => c.to_string(),
        TokenKind::Fn => "fn".into(),
        TokenKind::Enum => "enum".into(),
        TokenKind::Struct => "struct".into(),
        TokenKind::BitStruct => "bitstruct".into(),
        TokenKind::Impl => "impl".into(),
        TokenKind::Let => "let".into(),
        TokenKind::Mut => "mut".into(),
        TokenKind::Const => "const".into(),
        TokenKind::If => "if".into(),
        TokenKind::Match => "match".into(),
        TokenKind::Else => "else".into(),
        TokenKind::For => "for".into(),
        TokenKind::In => "in".into(),
        TokenKind::Loop => "loop".into(),
        TokenKind::While => "while".into(),
        TokenKind::Break => "break".into(),
        TokenKind::Continue => "continue".into(),
        TokenKind::Use => "use".into(),
        TokenKind::Mod => "mod".into(),
        TokenKind::Simd => "simd".into(),
        TokenKind::Prove => "prove".into(),
        TokenKind::Raw => "raw".into(),
        TokenKind::Naked => "naked".into(),
        TokenKind::Comptime => "comptime".into(),
        TokenKind::Trait => "trait".into(),
        TokenKind::Return => "return".into(),
        TokenKind::Arena => "arena".into(),
        TokenKind::EOF => "".into(),
    }
}
