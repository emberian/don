//! The BHS lexer.
//!
//! Written from the 363-file shipped corpus under `ron-data/bhs-corpus/`, not from a
//! designed grammar. Every token form below occurs in that corpus; where a form is
//! *absent* from the corpus but present in the engine's opcode set (`**`, `<<=`,
//! `^=`, …) it is lexed anyway and flagged in `docs/tracks/bhs-grammar.md` as
//! unattested.
//!
//! Encoding: one shipped file (`conquest/Napoleon/leipzigsetup.bhs`) is **not** valid
//! UTF-8. Source is decoded as UTF-8 when possible and Windows-1252/Latin-1 otherwise,
//! so a byte offset in the original maps to a char offset here for ASCII, which is all
//! the grammar uses. Non-ASCII bytes only ever occur inside string literals and
//! comments.

use std::fmt;

/// A source position, 1-based, for diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Pos {
    pub line: u32,
    pub col: u32,
}

impl fmt::Display for Pos {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line, self.col)
    }
}

/// Punctuation and operator tokens.
///
/// Names follow the engine's `OpCodeTypes` enum where an operator maps to one
/// (`Pow` -> `OP_POW_OP`, `LeftAssign` -> `OP_LEFT_ASSIGN`, …).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum P {
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBrack,
    RBrack,
    Semi,
    Comma,
    Dot,
    Colon,
    // assignment
    Assign,
    AddAssign,
    SubAssign,
    MulAssign,
    DivAssign,
    ModAssign,
    PowAssign,
    LeftAssign,
    RightAssign,
    AndAssign,
    XorAssign,
    OrAssign,
    // comparison
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    // arithmetic
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Pow,
    // logical / bitwise
    AndAnd,
    OrOr,
    Not,
    Amp,
    Pipe,
    Caret,
    Tilde,
    Shl,
    Shr,
    // increment / decrement
    Inc,
    Dec,
}

impl P {
    pub fn as_str(self) -> &'static str {
        use P::*;
        match self {
            LParen => "(",
            RParen => ")",
            LBrace => "{",
            RBrace => "}",
            LBrack => "[",
            RBrack => "]",
            Semi => ";",
            Comma => ",",
            Dot => ".",
            Colon => ":",
            Assign => "=",
            AddAssign => "+=",
            SubAssign => "-=",
            MulAssign => "*=",
            DivAssign => "/=",
            ModAssign => "%=",
            PowAssign => "**=",
            LeftAssign => "<<=",
            RightAssign => ">>=",
            AndAssign => "&=",
            XorAssign => "^=",
            OrAssign => "|=",
            Eq => "==",
            Ne => "!=",
            Lt => "<",
            Gt => ">",
            Le => "<=",
            Ge => ">=",
            Plus => "+",
            Minus => "-",
            Star => "*",
            Slash => "/",
            Percent => "%",
            Pow => "**",
            AndAnd => "&&",
            OrOr => "||",
            Not => "!",
            Amp => "&",
            Pipe => "|",
            Caret => "^",
            Tilde => "~",
            Shl => "<<",
            Shr => ">>",
            Inc => "++",
            Dec => "--",
        }
    }
}

/// A lexical token.
#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    /// An identifier or a keyword; the parser decides which, because BHS has no
    /// reserved-word barrier strong enough to do it here (`length` is both a builtin
    /// name and a member name; `scenario` is both a script-type qualifier and a
    /// plausible identifier).
    Ident(String),
    /// A decimal integer literal. Held as `i64` so an out-of-range literal is a
    /// *semantic* diagnostic rather than a lex failure; codegen truncates to `i32`.
    Int(i64),
    /// A real literal (`1.5`, `360.0`). The engine's `real` is binary32.
    Real(f32),
    /// A plain string literal.
    Str(String),
    /// `$S("...")` — a **localised** string literal. Lexed as one token because `$S`
    /// is not a callable name: it does not appear among the engine's 873 registered
    /// builtins. See `docs/tracks/bhs-grammar.md` §"$S".
    LocStr(String),
    Punct(P),
    Eof,
}

impl fmt::Display for Tok {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Tok::Ident(s) => write!(f, "identifier `{s}`"),
            Tok::Int(v) => write!(f, "integer `{v}`"),
            Tok::Real(v) => write!(f, "real `{v}`"),
            Tok::Str(_) => write!(f, "string literal"),
            Tok::LocStr(_) => write!(f, "$S string literal"),
            Tok::Punct(p) => write!(f, "`{}`", p.as_str()),
            Tok::Eof => write!(f, "end of file"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub tok: Tok,
    pub pos: Pos,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexError {
    pub pos: Pos,
    pub msg: String,
}

impl fmt::Display for LexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.pos, self.msg)
    }
}

/// Decode raw source bytes. Shipped `.bhs` files are ASCII apart from a handful of
/// Windows-1252 bytes inside strings and comments; one file is not valid UTF-8 at all.
pub fn decode_source(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        // Latin-1 fallback: every byte becomes exactly one char, so offsets survive.
        Err(_) => bytes.iter().map(|&b| b as char).collect(),
    }
}

pub struct Lexer<'a> {
    src: &'a [char],
    i: usize,
    line: u32,
    col: u32,
}

pub fn lex(src: &str) -> Result<Vec<Token>, LexError> {
    let chars: Vec<char> = src.chars().collect();
    Lexer {
        src: &chars,
        i: 0,
        line: 1,
        col: 1,
    }
    .run()
}

impl<'a> Lexer<'a> {
    fn pos(&self) -> Pos {
        Pos {
            line: self.line,
            col: self.col,
        }
    }

    fn peek(&self) -> Option<char> {
        self.src.get(self.i).copied()
    }

    fn peek_at(&self, n: usize) -> Option<char> {
        self.src.get(self.i + n).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.src.get(self.i).copied()?;
        self.i += 1;
        if c == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(c)
    }

    fn err<T>(&self, pos: Pos, msg: impl Into<String>) -> Result<T, LexError> {
        Err(LexError {
            pos,
            msg: msg.into(),
        })
    }

    fn run(mut self) -> Result<Vec<Token>, LexError> {
        let mut out = Vec::new();
        loop {
            self.skip_trivia()?;
            let pos = self.pos();
            let Some(c) = self.peek() else {
                out.push(Token { tok: Tok::Eof, pos });
                return Ok(out);
            };
            let tok = if c.is_ascii_digit()
                || (c == '.' && matches!(self.peek_at(1), Some(d) if d.is_ascii_digit()))
            {
                self.number(pos)?
            } else if c == '"' {
                Tok::Str(self.string(pos)?)
            } else if c == '$' {
                self.dollar(pos)?
            } else if is_ident_start(c) {
                self.ident()
            } else {
                Tok::Punct(self.punct(pos)?)
            };
            out.push(Token { tok, pos });
        }
    }

    fn skip_trivia(&mut self) -> Result<(), LexError> {
        loop {
            match self.peek() {
                Some(c) if c.is_whitespace() => {
                    self.bump();
                }
                Some('/') if self.peek_at(1) == Some('/') => {
                    while let Some(c) = self.peek() {
                        if c == '\n' {
                            break;
                        }
                        self.bump();
                    }
                }
                Some('/') if self.peek_at(1) == Some('*') => {
                    let start = self.pos();
                    self.bump();
                    self.bump();
                    loop {
                        match self.peek() {
                            None => return self.err(start, "unterminated /* comment"),
                            Some('*') if self.peek_at(1) == Some('/') => {
                                self.bump();
                                self.bump();
                                break;
                            }
                            _ => {
                                self.bump();
                            }
                        }
                    }
                }
                _ => return Ok(()),
            }
        }
    }

    fn ident(&mut self) -> Tok {
        let mut s = String::new();
        while let Some(c) = self.peek() {
            if is_ident_continue(c) {
                s.push(c);
                self.bump();
            } else {
                break;
            }
        }
        Tok::Ident(s)
    }

    /// `$S("...")`. The corpus contains exactly one `$`-form in code position, `$S`,
    /// always immediately followed by `(` and a string literal. `$NUM`, `$STRING`,
    /// `$d`, `$s` occur only *inside* string literals, where they are `parse()`
    /// format specifiers and never reach the lexer as tokens.
    fn dollar(&mut self, pos: Pos) -> Result<Tok, LexError> {
        self.bump(); // '$'
        let mut name = String::new();
        while let Some(c) = self.peek() {
            if is_ident_continue(c) {
                name.push(c);
                self.bump();
            } else {
                break;
            }
        }
        if name != "S" {
            return self.err(pos, format!("unknown `${name}` form"));
        }
        self.skip_trivia()?;
        if self.peek() != Some('(') {
            return self.err(pos, "`$S` must be followed by `(`");
        }
        self.bump();
        self.skip_trivia()?;
        let spos = self.pos();
        if self.peek() != Some('"') {
            return self.err(spos, "`$S(` must be followed by a string literal");
        }
        let s = self.string(spos)?;
        self.skip_trivia()?;
        if self.peek() != Some(')') {
            return self.err(self.pos(), "unterminated `$S(...)`");
        }
        self.bump();
        Ok(Tok::LocStr(s))
    }

    fn string(&mut self, start: Pos) -> Result<String, LexError> {
        self.bump(); // opening quote
        let mut s = String::new();
        loop {
            match self.bump() {
                None => return self.err(start, "unterminated string literal"),
                Some('"') => return Ok(s),
                Some('\\') => match self.bump() {
                    None => return self.err(start, "unterminated string literal"),
                    Some('n') => s.push('\n'),
                    Some('t') => s.push('\t'),
                    Some('r') => s.push('\r'),
                    Some('0') => s.push('\0'),
                    Some('\\') => s.push('\\'),
                    Some('"') => s.push('"'),
                    // The corpus contains `\s`, `\c` and `\a` inside strings. They are
                    // not C escapes; retail's `parse()` markup passes them through. We
                    // keep the backslash so a round-trip is byte-identical.
                    Some(other) => {
                        s.push('\\');
                        s.push(other);
                    }
                },
                Some(c) => s.push(c),
            }
        }
    }

    fn number(&mut self, pos: Pos) -> Result<Tok, LexError> {
        let mut s = String::new();
        let mut is_real = false;
        if self.peek() == Some('0') && matches!(self.peek_at(1), Some('x') | Some('X')) {
            self.bump();
            self.bump();
            let mut hex = String::new();
            while let Some(c) = self.peek() {
                if c.is_ascii_hexdigit() {
                    hex.push(c);
                    self.bump();
                } else {
                    break;
                }
            }
            if hex.is_empty() {
                return self.err(pos, "hex literal has no digits");
            }
            let v = u32::from_str_radix(&hex, 16).map_err(|_| LexError {
                pos,
                msg: format!("hex literal `0x{hex}` out of range"),
            })?;
            return Ok(Tok::Int(v as i32 as i64));
        }
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                s.push(c);
                self.bump();
            } else if c == '.'
                && !is_real
                && matches!(self.peek_at(1), Some(d) if d.is_ascii_digit())
            {
                is_real = true;
                s.push(c);
                self.bump();
            } else if c == '.'
                && !is_real
                && !matches!(self.peek_at(1), Some(d) if is_ident_start(d))
            {
                // Trailing-dot form `1.` — attested nowhere, accepted for symmetry.
                is_real = true;
                s.push(c);
                self.bump();
            } else {
                break;
            }
        }
        if is_real {
            let v: f32 = s.parse().map_err(|_| LexError {
                pos,
                msg: format!("bad real literal `{s}`"),
            })?;
            Ok(Tok::Real(v))
        } else {
            let v: i64 = s.parse().map_err(|_| LexError {
                pos,
                msg: format!("integer literal `{s}` out of range"),
            })?;
            Ok(Tok::Int(v))
        }
    }

    fn punct(&mut self, pos: Pos) -> Result<P, LexError> {
        use P::*;
        let c = self.bump().unwrap();
        let p = match c {
            '(' => LParen,
            ')' => RParen,
            '{' => LBrace,
            '}' => RBrace,
            '[' => LBrack,
            ']' => RBrack,
            ';' => Semi,
            ',' => Comma,
            '.' => Dot,
            ':' => Colon,
            '=' => self.if_next('=', Eq, Assign),
            '!' => self.if_next('=', Ne, Not),
            '+' => match self.peek() {
                Some('+') => {
                    self.bump();
                    Inc
                }
                Some('=') => {
                    self.bump();
                    AddAssign
                }
                _ => Plus,
            },
            '-' => match self.peek() {
                Some('-') => {
                    self.bump();
                    Dec
                }
                Some('=') => {
                    self.bump();
                    SubAssign
                }
                _ => Minus,
            },
            '*' => match self.peek() {
                Some('*') => {
                    self.bump();
                    self.if_next('=', PowAssign, Pow)
                }
                Some('=') => {
                    self.bump();
                    MulAssign
                }
                _ => Star,
            },
            '/' => self.if_next('=', DivAssign, Slash),
            '%' => self.if_next('=', ModAssign, Percent),
            '^' => self.if_next('=', XorAssign, Caret),
            '~' => Tilde,
            '&' => match self.peek() {
                Some('&') => {
                    self.bump();
                    AndAnd
                }
                Some('=') => {
                    self.bump();
                    AndAssign
                }
                _ => Amp,
            },
            '|' => match self.peek() {
                Some('|') => {
                    self.bump();
                    OrOr
                }
                Some('=') => {
                    self.bump();
                    OrAssign
                }
                _ => Pipe,
            },
            '<' => match self.peek() {
                Some('<') => {
                    self.bump();
                    self.if_next('=', LeftAssign, Shl)
                }
                Some('=') => {
                    self.bump();
                    Le
                }
                _ => Lt,
            },
            '>' => match self.peek() {
                Some('>') => {
                    self.bump();
                    self.if_next('=', RightAssign, Shr)
                }
                Some('=') => {
                    self.bump();
                    Ge
                }
                _ => Gt,
            },
            other => {
                return self.err(pos, format!("unexpected character {other:?}"));
            }
        };
        Ok(p)
    }

    fn if_next(&mut self, c: char, yes: P, no: P) -> P {
        if self.peek() == Some(c) {
            self.bump();
            yes
        } else {
            no
        }
    }
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_ident_continue(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(s: &str) -> Vec<Tok> {
        lex(s).unwrap().into_iter().map(|t| t.tok).collect()
    }

    #[test]
    fn comments_and_idents() {
        assert_eq!(
            toks("a // b\n/* c */ d"),
            vec![Tok::Ident("a".into()), Tok::Ident("d".into()), Tok::Eof]
        );
    }

    #[test]
    fn loc_string_is_one_token() {
        assert_eq!(
            toks(r#"$S("hi")"#),
            vec![Tok::LocStr("hi".into()), Tok::Eof]
        );
    }

    #[test]
    fn operators_max_munch() {
        use P::*;
        assert_eq!(
            toks("a **= b <<= c >>= d ++ e"),
            vec![
                Tok::Ident("a".into()),
                Tok::Punct(PowAssign),
                Tok::Ident("b".into()),
                Tok::Punct(LeftAssign),
                Tok::Ident("c".into()),
                Tok::Punct(RightAssign),
                Tok::Ident("d".into()),
                Tok::Punct(Inc),
                Tok::Ident("e".into()),
                Tok::Eof
            ]
        );
    }

    #[test]
    fn member_access_after_int_is_not_a_real() {
        // `x[0].length` must not lex `0.` as a real.
        use P::*;
        assert_eq!(
            toks("1.length"),
            vec![
                Tok::Int(1),
                Tok::Punct(Dot),
                Tok::Ident("length".into()),
                Tok::Eof
            ]
        );
    }

    #[test]
    fn latin1_fallback_never_fails() {
        let s = decode_source(&[b'"', 0xE9, b'"']);
        assert_eq!(toks(&s), vec![Tok::Str("\u{e9}".into()), Tok::Eof]);
    }
}
