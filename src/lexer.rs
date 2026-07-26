use crate::diagnostic::{Diagnostic, Span};

#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TokenKind {
    Identifier(String),
    String(String),
    Integer(i64),
    Float(f32),
    Game,
    Let,
    Start,
    Update,
    Draw,
    Fn,
    Return,
    If,
    Else,
    While,
    True,
    False,
    I32,
    F32,
    Bool,
    LeftParen,
    RightParen,
    LeftBrace,
    RightBrace,
    Colon,
    Comma,
    Semicolon,
    Plus,
    Minus,
    Star,
    Slash,
    Bang,
    Equal,
    EqualEqual,
    BangEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Arrow,
    Eof,
}

pub fn lex(source: &str) -> Result<Vec<Token>, Vec<Diagnostic>> {
    Lexer::new(source).run()
}

struct Lexer<'a> {
    source: &'a str,
    bytes: &'a [u8],
    cursor: usize,
    tokens: Vec<Token>,
    diagnostics: Vec<Diagnostic>,
}

impl<'a> Lexer<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            bytes: source.as_bytes(),
            cursor: 0,
            tokens: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    fn run(mut self) -> Result<Vec<Token>, Vec<Diagnostic>> {
        while self.cursor < self.bytes.len() {
            let start = self.cursor;
            let byte = self.advance();
            match byte {
                b' ' | b'\t' | b'\r' | b'\n' => {}
                b'(' => self.simple(TokenKind::LeftParen, start),
                b')' => self.simple(TokenKind::RightParen, start),
                b'{' => self.simple(TokenKind::LeftBrace, start),
                b'}' => self.simple(TokenKind::RightBrace, start),
                b':' => self.simple(TokenKind::Colon, start),
                b',' => self.simple(TokenKind::Comma, start),
                b';' => self.simple(TokenKind::Semicolon, start),
                b'+' => self.simple(TokenKind::Plus, start),
                b'*' => self.simple(TokenKind::Star, start),
                b'-' if self.take(b'>') => self.push(TokenKind::Arrow, start),
                b'-' => self.simple(TokenKind::Minus, start),
                b'/' if self.take(b'/') => self.line_comment(),
                b'/' => self.simple(TokenKind::Slash, start),
                b'!' if self.take(b'=') => self.push(TokenKind::BangEqual, start),
                b'!' => self.simple(TokenKind::Bang, start),
                b'=' if self.take(b'=') => self.push(TokenKind::EqualEqual, start),
                b'=' => self.simple(TokenKind::Equal, start),
                b'<' if self.take(b'=') => self.push(TokenKind::LessEqual, start),
                b'<' => self.simple(TokenKind::Less, start),
                b'>' if self.take(b'=') => self.push(TokenKind::GreaterEqual, start),
                b'>' => self.simple(TokenKind::Greater, start),
                b'"' => self.string(start),
                b'0'..=b'9' => self.number(start),
                b'a'..=b'z' | b'A'..=b'Z' | b'_' => self.identifier(start),
                _ => {
                    let character = self.source[start..]
                        .chars()
                        .next()
                        .expect("cursor always points at a source character");
                    self.cursor = start + character.len_utf8();
                    self.diagnostics.push(Diagnostic::new(
                        format!("unexpected character `{character}`"),
                        Span::new(start, self.cursor),
                    ));
                }
            }
        }

        self.tokens.push(Token {
            kind: TokenKind::Eof,
            span: Span::new(self.cursor, self.cursor),
        });
        if self.diagnostics.is_empty() {
            Ok(self.tokens)
        } else {
            Err(self.diagnostics)
        }
    }

    fn advance(&mut self) -> u8 {
        let byte = self.bytes[self.cursor];
        self.cursor += 1;
        byte
    }

    fn take(&mut self, expected: u8) -> bool {
        if self.bytes.get(self.cursor) == Some(&expected) {
            self.cursor += 1;
            true
        } else {
            false
        }
    }

    fn simple(&mut self, kind: TokenKind, start: usize) {
        self.tokens.push(Token {
            kind,
            span: Span::new(start, start + 1),
        });
    }

    fn push(&mut self, kind: TokenKind, start: usize) {
        self.tokens.push(Token {
            kind,
            span: Span::new(start, self.cursor),
        });
    }

    fn line_comment(&mut self) {
        while self.cursor < self.bytes.len() && self.bytes[self.cursor] != b'\n' {
            self.cursor += 1;
        }
    }

    fn string(&mut self, start: usize) {
        let value_start = self.cursor;
        while self.cursor < self.bytes.len() && self.bytes[self.cursor] != b'"' {
            if self.bytes[self.cursor] == b'\n' {
                self.diagnostics.push(Diagnostic::new(
                    "unterminated game title",
                    Span::new(start, self.cursor),
                ));
                return;
            }
            self.cursor += 1;
        }
        if self.cursor == self.bytes.len() {
            self.diagnostics.push(Diagnostic::new(
                "unterminated game title",
                Span::new(start, self.cursor),
            ));
            return;
        }
        let value = self.source[value_start..self.cursor].to_owned();
        self.cursor += 1;
        self.push(TokenKind::String(value), start);
    }

    fn number(&mut self, start: usize) {
        while self.bytes.get(self.cursor).is_some_and(u8::is_ascii_digit) {
            self.cursor += 1;
        }
        let is_float = self.bytes.get(self.cursor) == Some(&b'.')
            && self
                .bytes
                .get(self.cursor + 1)
                .is_some_and(u8::is_ascii_digit);
        if is_float {
            self.cursor += 1;
            while self.bytes.get(self.cursor).is_some_and(u8::is_ascii_digit) {
                self.cursor += 1;
            }
        }
        let text = &self.source[start..self.cursor];
        let parsed = if is_float {
            text.parse::<f32>().map(TokenKind::Float).map_err(|_| ())
        } else {
            text.parse::<i64>().map(TokenKind::Integer).map_err(|_| ())
        };
        match parsed {
            Ok(kind) => self.push(kind, start),
            Err(()) => self.diagnostics.push(Diagnostic::new(
                "numeric literal is out of range",
                Span::new(start, self.cursor),
            )),
        }
    }

    fn identifier(&mut self, start: usize) {
        while self
            .bytes
            .get(self.cursor)
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
        {
            self.cursor += 1;
        }
        let text = &self.source[start..self.cursor];
        let kind = match text {
            "game" => TokenKind::Game,
            "let" => TokenKind::Let,
            "start" => TokenKind::Start,
            "update" => TokenKind::Update,
            "draw" => TokenKind::Draw,
            "fn" => TokenKind::Fn,
            "return" => TokenKind::Return,
            "if" => TokenKind::If,
            "else" => TokenKind::Else,
            "while" => TokenKind::While,
            "true" => TokenKind::True,
            "false" => TokenKind::False,
            "i32" => TokenKind::I32,
            "f32" => TokenKind::F32,
            "bool" => TokenKind::Bool,
            _ => TokenKind::Identifier(text.to_owned()),
        };
        self.push(kind, start);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexes_game_shaped_source_and_locations() {
        let tokens = lex("game \"Dot\"\nlet x: f32 = 1.5\nupdate(dt: f32) { x = x + dt }")
            .expect("source should lex");
        assert_eq!(tokens[0].kind, TokenKind::Game);
        assert_eq!(tokens[1].kind, TokenKind::String("Dot".into()));
        assert_eq!(tokens[2].kind, TokenKind::Let);
        assert!(
            tokens
                .iter()
                .any(|token| token.kind == TokenKind::Float(1.5))
        );
        assert_eq!(tokens[2].span, Span::new(11, 14));
    }

    #[test]
    fn reports_unknown_character() {
        let errors = lex("game \"Dot\"\n@").expect_err("source should fail");
        assert_eq!(errors[0].message, "unexpected character `@`");
        assert_eq!(errors[0].span, Span::new(11, 12));
    }

    #[test]
    fn keeps_unicode_error_spans_on_utf8_boundaries() {
        let source = "game \"Dot\"\n🐸";
        let errors = lex(source).expect_err("source should fail");
        assert_eq!(errors[0].message, "unexpected character `🐸`");
        assert_eq!(errors[0].span, Span::new(11, 15));
        assert!(
            errors[0]
                .render(std::path::Path::new("frog.spk"), source)
                .contains("🐸")
        );
    }
}
