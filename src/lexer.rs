#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    Fn,
    Struct,
    Let,
    Mut,
    Const,
    If,
    Else,
    For,
    In,
    Loop,
    While,
    Break,
    Continue,
    Use,
    Simd,
    Prove,
    Raw,
    Naked,
    Return,
    Ident(String),
    Number(i64),
    StrLit(String),
    Comma,      // ,
    Dot,        // .
    Colon,      // :
    ColonColon, // ::
    Semicolon,  // ;
    Assign,     // =
    PlusEq,     // +=
    MinusEq,    // -=
    AmpEq,      // &=
    PipeEq,     // |=
    Bang,       // !
    Less,       // <
    Greater,    // >
    Shl,        // <<
    Shr,        // >>
    Amp,        // &
    Pipe,       // |
    Caret,      // ^
    AndAnd,     // &&
    OrOr,       // ||
    DotDot,     // ..
    LParen,     // (
    RParen,     // )
    LBrace,     // {
    RBrace,     // }
    LBracket,   // [
    RBracket,   // ]
    Plus,       // +
    Minus,      // -
    Star,       // *
    Slash,      // /
    Percent,    // %
    Arrow,      // ->
    EOF,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenKind,
    pub line: usize,
}

pub struct Lexer {
    input: Vec<char>,
    pos: usize,
    line: usize,
}

impl Lexer {
    pub fn new(source: &str) -> Self {
        Self {
            input: source.chars().collect(),
            pos: 0,
            line: 1,
        }
    }

    fn peek(&self) -> Option<char> {
        self.input.get(self.pos).copied()
    }

    fn peek_offset(&self, offset: usize) -> Option<char> {
        self.input.get(self.pos + offset).copied()
    }

    fn advance(&mut self) -> Option<char> {
        let ch = self.peek();
        if ch == Some('\n') {
            self.line += 1;
        }
        self.pos += 1;
        ch
    }

    pub fn tokenize(&mut self) -> Vec<Token> {
        let mut tokens = Vec::new();
        while let Some(ch) = self.peek() {
            let cur_line = self.line;
            match ch {
                c if c.is_whitespace() => {
                    self.advance();
                }
                '/' if self.peek_offset(1) == Some('/') => {
                    while let Some(c) = self.peek() {
                        if c == '\n' {
                            break;
                        }
                        self.advance();
                    }
                }
                '"' => {
                    self.advance();
                    let mut s = String::new();
                    while let Some(c) = self.peek() {
                        if c == '"' {
                            self.advance();
                            break;
                        }
                        if c == '\\' && self.peek_offset(1) == Some('"') {
                            self.advance();
                            s.push(self.advance().unwrap());
                            continue;
                        }
                        s.push(self.advance().unwrap());
                    }
                    tokens.push(Token {
                        kind: TokenKind::StrLit(s),
                        line: cur_line,
                    });
                }
                c if c.is_alphabetic() || c == '_' => {
                    let mut word = String::new();
                    while let Some(c) = self.peek() {
                        if c.is_alphanumeric() || c == '_' {
                            word.push(self.advance().unwrap());
                        } else {
                            break;
                        }
                    }
                    let kind = match word.as_str() {
                        "fn" => TokenKind::Fn,
                        "struct" => TokenKind::Struct,
                        "let" => TokenKind::Let,
                        "mut" => TokenKind::Mut,
                        "const" => TokenKind::Const,
                        "if" => TokenKind::If,
                        "else" => TokenKind::Else,
                        "for" => TokenKind::For,
                        "in" => TokenKind::In,
                        "loop" => TokenKind::Loop,
                        "while" => TokenKind::While,
                        "break" => TokenKind::Break,
                        "continue" => TokenKind::Continue,
                        "use" => TokenKind::Use,
                        "simd" => TokenKind::Simd,
                        "prove" => TokenKind::Prove,
                        "raw" => TokenKind::Raw,
                        "naked" => TokenKind::Naked,
                        "return" => TokenKind::Return,
                        _ => TokenKind::Ident(word),
                    };
                    tokens.push(Token {
                        kind,
                        line: cur_line,
                    });
                }
                '0' if matches!(self.peek_offset(1), Some('x') | Some('X')) => {
                    self.advance();
                    self.advance();
                    let mut hex_str = String::new();
                    while let Some(c) = self.peek() {
                        if c.is_ascii_hexdigit() || c == '_' {
                            let ch = self.advance().unwrap();
                            if ch != '_' {
                                hex_str.push(ch);
                            }
                        } else {
                            break;
                        }
                    }
                    let val = i64::from_str_radix(&hex_str, 16).unwrap_or(0);
                    tokens.push(Token {
                        kind: TokenKind::Number(val),
                        line: cur_line,
                    });
                }
                c if c.is_ascii_digit() => {
                    let mut num_str = String::new();
                    while let Some(c) = self.peek() {
                        if c.is_ascii_digit() || c == '_' {
                            let ch = self.advance().unwrap();
                            if ch != '_' {
                                num_str.push(ch);
                            }
                        } else {
                            break;
                        }
                    }
                    tokens.push(Token {
                        kind: TokenKind::Number(num_str.parse().unwrap_or(0)),
                        line: cur_line,
                    });
                }
                '.' if self.peek_offset(1) == Some('.') => {
                    self.advance();
                    self.advance();
                    tokens.push(Token {
                        kind: TokenKind::DotDot,
                        line: cur_line,
                    });
                }
                '-' if self.peek_offset(1) == Some('>') => {
                    self.advance();
                    self.advance();
                    tokens.push(Token {
                        kind: TokenKind::Arrow,
                        line: cur_line,
                    });
                }
                '-' if self.peek_offset(1) == Some('=') => {
                    self.advance();
                    self.advance();
                    tokens.push(Token {
                        kind: TokenKind::MinusEq,
                        line: cur_line,
                    });
                }
                '+' if self.peek_offset(1) == Some('=') => {
                    self.advance();
                    self.advance();
                    tokens.push(Token {
                        kind: TokenKind::PlusEq,
                        line: cur_line,
                    });
                }
                '&' if self.peek_offset(1) == Some('&') => {
                    self.advance();
                    self.advance();
                    tokens.push(Token {
                        kind: TokenKind::AndAnd,
                        line: cur_line,
                    });
                }
                '&' if self.peek_offset(1) == Some('=') => {
                    self.advance();
                    self.advance();
                    tokens.push(Token {
                        kind: TokenKind::AmpEq,
                        line: cur_line,
                    });
                }
                '|' if self.peek_offset(1) == Some('|') => {
                    self.advance();
                    self.advance();
                    tokens.push(Token {
                        kind: TokenKind::OrOr,
                        line: cur_line,
                    });
                }
                '|' if self.peek_offset(1) == Some('=') => {
                    self.advance();
                    self.advance();
                    tokens.push(Token {
                        kind: TokenKind::PipeEq,
                        line: cur_line,
                    });
                }
                ':' if self.peek_offset(1) == Some(':') => {
                    self.advance();
                    self.advance();
                    tokens.push(Token {
                        kind: TokenKind::ColonColon,
                        line: cur_line,
                    });
                }
                '<' if self.peek_offset(1) == Some('<') => {
                    self.advance();
                    self.advance();
                    tokens.push(Token {
                        kind: TokenKind::Shl,
                        line: cur_line,
                    });
                }
                '>' if self.peek_offset(1) == Some('>') => {
                    self.advance();
                    self.advance();
                    tokens.push(Token {
                        kind: TokenKind::Shr,
                        line: cur_line,
                    });
                }
                '.' => {
                    self.advance();
                    tokens.push(Token {
                        kind: TokenKind::Dot,
                        line: cur_line,
                    });
                }
                ',' => {
                    self.advance();
                    tokens.push(Token {
                        kind: TokenKind::Comma,
                        line: cur_line,
                    });
                }
                ':' => {
                    self.advance();
                    tokens.push(Token {
                        kind: TokenKind::Colon,
                        line: cur_line,
                    });
                }
                ';' => {
                    self.advance();
                    tokens.push(Token {
                        kind: TokenKind::Semicolon,
                        line: cur_line,
                    });
                }
                '=' => {
                    self.advance();
                    tokens.push(Token {
                        kind: TokenKind::Assign,
                        line: cur_line,
                    });
                }
                '!' => {
                    self.advance();
                    tokens.push(Token {
                        kind: TokenKind::Bang,
                        line: cur_line,
                    });
                }
                '<' => {
                    self.advance();
                    tokens.push(Token {
                        kind: TokenKind::Less,
                        line: cur_line,
                    });
                }
                '>' => {
                    self.advance();
                    tokens.push(Token {
                        kind: TokenKind::Greater,
                        line: cur_line,
                    });
                }
                '&' => {
                    self.advance();
                    tokens.push(Token {
                        kind: TokenKind::Amp,
                        line: cur_line,
                    });
                }
                '|' => {
                    self.advance();
                    tokens.push(Token {
                        kind: TokenKind::Pipe,
                        line: cur_line,
                    });
                }
                '^' => {
                    self.advance();
                    tokens.push(Token {
                        kind: TokenKind::Caret,
                        line: cur_line,
                    });
                }
                '(' => {
                    self.advance();
                    tokens.push(Token {
                        kind: TokenKind::LParen,
                        line: cur_line,
                    });
                }
                ')' => {
                    self.advance();
                    tokens.push(Token {
                        kind: TokenKind::RParen,
                        line: cur_line,
                    });
                }
                '{' => {
                    self.advance();
                    tokens.push(Token {
                        kind: TokenKind::LBrace,
                        line: cur_line,
                    });
                }
                '}' => {
                    self.advance();
                    tokens.push(Token {
                        kind: TokenKind::RBrace,
                        line: cur_line,
                    });
                }
                '[' => {
                    self.advance();
                    tokens.push(Token {
                        kind: TokenKind::LBracket,
                        line: cur_line,
                    });
                }
                ']' => {
                    self.advance();
                    tokens.push(Token {
                        kind: TokenKind::RBracket,
                        line: cur_line,
                    });
                }
                '+' => {
                    self.advance();
                    tokens.push(Token {
                        kind: TokenKind::Plus,
                        line: cur_line,
                    });
                }
                '-' => {
                    self.advance();
                    tokens.push(Token {
                        kind: TokenKind::Minus,
                        line: cur_line,
                    });
                }
                '*' => {
                    self.advance();
                    tokens.push(Token {
                        kind: TokenKind::Star,
                        line: cur_line,
                    });
                }
                '/' => {
                    self.advance();
                    tokens.push(Token {
                        kind: TokenKind::Slash,
                        line: cur_line,
                    });
                }
                '%' => {
                    self.advance();
                    tokens.push(Token {
                        kind: TokenKind::Percent,
                        line: cur_line,
                    });
                }
                _ => {
                    self.advance();
                }
            }
        }
        tokens.push(Token {
            kind: TokenKind::EOF,
            line: self.line,
        });
        tokens
    }
}
