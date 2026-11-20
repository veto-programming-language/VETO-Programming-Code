#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Fn,
    Let,
    Mut,
    If,
    Else,
    For,
    In,
    Loop,
    While,
    Use,
    Simd,
    Prove,
    Raw,
    Naked,
    Return,
    Ident(String),
    Number(i64),
    Comma,      // ,
    Colon,      // :
    ColonColon, // ::
    Semicolon,  // ;
    Assign,     // =
    Bang,       // !
    Less,       // <
    Greater,    // >
    Shl,        // <<
    Shr,        // >>
    Amp,        // &
    Pipe,       // |
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

pub struct Lexer {
    input: Vec<char>,
    pos: usize,
}

impl Lexer {
    pub fn new(source: &str) -> Self {
        Self {
            input: source.chars().collect(),
            pos: 0,
        }
    }

    fn peek(&self) -> Option<char> {
        self.input.get(self.pos).copied()
    }

    fn advance(&mut self) -> Option<char> {
        let ch = self.peek();
        self.pos += 1;
        ch
    }

    pub fn tokenize(&mut self) -> Vec<Token> {
        let mut tokens = Vec::new();
        while let Some(ch) = self.peek() {
            match ch {
                c if c.is_whitespace() => {
                    self.advance();
                }
                '/' if self.input.get(self.pos + 1) == Some(&'/') => {
                    while let Some(c) = self.peek() {
                        if c == '\n' {
                            break;
                        }
                        self.advance();
                    }
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
                    let token = match word.as_str() {
                        "fn" => Token::Fn,
                        "let" => Token::Let,
                        "mut" => Token::Mut,
                        "if" => Token::If,
                        "else" => Token::Else,
                        "for" => Token::For,
                        "in" => Token::In,
                        "loop" => Token::Loop,
                        "while" => Token::While,
                        "use" => Token::Use,
                        "simd" => Token::Simd,
                        "prove" => Token::Prove,
                        "raw" => Token::Raw,
                        "naked" => Token::Naked,
                        "return" => Token::Return,
                        _ => Token::Ident(word),
                    };
                    tokens.push(token);
                }
                c if c.is_ascii_digit() => {
                    let mut num_str = String::new();
                    while let Some(c) = self.peek() {
                        if c.is_ascii_digit() {
                            num_str.push(self.advance().unwrap());
                        } else {
                            break;
                        }
                    }
                    tokens.push(Token::Number(num_str.parse().unwrap_or(0)));
                }
                '.' if self.input.get(self.pos + 1) == Some(&'.') => {
                    self.advance();
                    self.advance();
                    tokens.push(Token::DotDot);
                }
                '-' if self.input.get(self.pos + 1) == Some(&'>') => {
                    self.advance();
                    self.advance();
                    tokens.push(Token::Arrow);
                }
                ':' if self.input.get(self.pos + 1) == Some(&':') => {
                    self.advance();
                    self.advance();
                    tokens.push(Token::ColonColon);
                }
                '<' if self.input.get(self.pos + 1) == Some(&'<') => {
                    self.advance();
                    self.advance();
                    tokens.push(Token::Shl);
                }
                '>' if self.input.get(self.pos + 1) == Some(&'>') => {
                    self.advance();
                    self.advance();
                    tokens.push(Token::Shr);
                }
                ',' => {
                    self.advance();
                    tokens.push(Token::Comma);
                }
                ':' => {
                    self.advance();
                    tokens.push(Token::Colon);
                }
                ';' => {
                    self.advance();
                    tokens.push(Token::Semicolon);
                }
                '=' => {
                    self.advance();
                    tokens.push(Token::Assign);
                }
                '!' => {
                    self.advance();
                    tokens.push(Token::Bang);
                }
                '<' => {
                    self.advance();
                    tokens.push(Token::Less);
                }
                '>' => {
                    self.advance();
                    tokens.push(Token::Greater);
                }
                '&' => {
                    self.advance();
                    tokens.push(Token::Amp);
                }
                '|' => {
                    self.advance();
                    tokens.push(Token::Pipe);
                }
                '(' => {
                    self.advance();
                    tokens.push(Token::LParen);
                }
                ')' => {
                    self.advance();
                    tokens.push(Token::RParen);
                }
                '{' => {
                    self.advance();
                    tokens.push(Token::LBrace);
                }
                '}' => {
                    self.advance();
                    tokens.push(Token::RBrace);
                }
                '[' => {
                    self.advance();
                    tokens.push(Token::LBracket);
                }
                ']' => {
                    self.advance();
                    tokens.push(Token::RBracket);
                }
                '+' => {
                    self.advance();
                    tokens.push(Token::Plus);
                }
                '-' => {
                    self.advance();
                    tokens.push(Token::Minus);
                }
                '*' => {
                    self.advance();
                    tokens.push(Token::Star);
                }
                '/' => {
                    self.advance();
                    tokens.push(Token::Slash);
                }
                '%' => {
                    self.advance();
                    tokens.push(Token::Percent);
                }
                _ => {
                    self.advance();
                }
            }
        }
        tokens.push(Token::EOF);
        tokens
    }
}
