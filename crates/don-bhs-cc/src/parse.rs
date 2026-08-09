//! The BHS parser.
//!
//! Recursive descent with precedence climbing. Every disambiguation rule here was
//! forced by a construct that occurs in `ron-data/bhs-corpus/`; the reasoning is in
//! `docs/tracks/bhs-grammar.md`. The two rules that matter most:
//!
//! * **Declaration vs expression** at statement level is decided by *shape*, not by a
//!   type table: `Ident Ident` starts a declaration, `Ident (` / `Ident .` / `Ident =`
//!   starts an expression. This is what lets user `struct` types be used as local types
//!   without the parser knowing them, and it is exactly what a one-pass 2003 compiler
//!   could do.
//! * **Script heads** are a run of identifiers followed by `(` or `{`. The last is the
//!   name; a leading run of one or two words is the return type and/or the script-type
//!   qualifier. A head of exactly one word followed by `{` is the file's anonymous
//!   per-frame entry script.

use crate::ast::*;
use crate::lex::{lex, LexError, Pos, Tok, Token, P};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub pos: Pos,
    pub msg: String,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.pos, self.msg)
    }
}

impl From<LexError> for ParseError {
    fn from(e: LexError) -> Self {
        ParseError {
            pos: e.pos,
            msg: e.msg,
        }
    }
}

/// The script-type qualifiers the corpus uses. Attested counts across 363 files:
/// `conquest` 262, `scenario` 63, `ai` 11 in the three-word declaration form, plus the
/// anonymous entry form `scenario { … }`.
pub const SCRIPT_TYPES: [&str; 3] = ["ai", "scenario", "conquest"];

pub fn is_script_type(s: &str) -> bool {
    SCRIPT_TYPES.iter().any(|k| k.eq_ignore_ascii_case(s))
}

/// Words that may never be an identifier in expression position.
fn is_keyword(s: &str) -> bool {
    matches!(
        s.to_ascii_lowercase().as_str(),
        "if" | "else"
            | "while"
            | "do"
            | "for"
            | "switch"
            | "case"
            | "default"
            | "break"
            | "continue"
            | "return"
            | "static"
            | "struct"
            | "labels"
            | "trigger"
            | "run_once"
            | "include"
            | "ref"
    )
}

pub fn parse_file(path: &str, src: &str) -> Result<SourceFile, ParseError> {
    let toks = lex(src)?;
    let mut p = Parser { t: toks, i: 0 };
    let items = p.items()?;
    Ok(SourceFile {
        path: path.to_string(),
        items,
    })
}

struct Parser {
    t: Vec<Token>,
    i: usize,
}

impl Parser {
    // ---- token helpers -------------------------------------------------------

    fn peek(&self) -> &Tok {
        &self.t[self.i.min(self.t.len() - 1)].tok
    }

    fn peek_at(&self, n: usize) -> &Tok {
        &self.t[(self.i + n).min(self.t.len() - 1)].tok
    }

    fn pos(&self) -> Pos {
        self.t[self.i.min(self.t.len() - 1)].pos
    }

    fn bump(&mut self) -> Tok {
        let t = self.t[self.i.min(self.t.len() - 1)].tok.clone();
        if self.i < self.t.len() - 1 {
            self.i += 1;
        }
        t
    }

    fn at_eof(&self) -> bool {
        matches!(self.peek(), Tok::Eof)
    }

    fn at(&self, p: P) -> bool {
        matches!(self.peek(), Tok::Punct(q) if *q == p)
    }

    fn at_n(&self, n: usize, p: P) -> bool {
        matches!(self.peek_at(n), Tok::Punct(q) if *q == p)
    }

    fn eat(&mut self, p: P) -> bool {
        if self.at(p) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, p: P) -> Result<(), ParseError> {
        if self.eat(p) {
            Ok(())
        } else {
            Err(self.err(format!("expected `{}`, found {}", p.as_str(), self.peek())))
        }
    }

    /// Is the current token the given keyword (case-insensitively)?
    fn at_kw(&self, kw: &str) -> bool {
        matches!(self.peek(), Tok::Ident(s) if s.eq_ignore_ascii_case(kw))
    }

    fn eat_kw(&mut self, kw: &str) -> bool {
        if self.at_kw(kw) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn ident(&mut self) -> Result<String, ParseError> {
        match self.bump() {
            Tok::Ident(s) => Ok(s),
            other => Err(ParseError {
                pos: self.pos(),
                msg: format!("expected identifier, found {other}"),
            }),
        }
    }

    fn err(&self, msg: impl Into<String>) -> ParseError {
        ParseError {
            pos: self.pos(),
            msg: msg.into(),
        }
    }

    fn is_ident_tok(&self, n: usize) -> bool {
        matches!(self.peek_at(n), Tok::Ident(_))
    }

    // ---- file scope ----------------------------------------------------------

    fn items(&mut self) -> Result<Vec<Item>, ParseError> {
        let mut out = Vec::new();
        while !self.at_eof() {
            if self.eat(P::Semi) {
                continue;
            }
            out.push(self.item()?);
        }
        Ok(out)
    }

    fn item(&mut self) -> Result<Item, ParseError> {
        let pos = self.pos();
        if self.at_kw("include") {
            self.bump();
            let path = match self.bump() {
                Tok::Str(s) => s,
                other => {
                    return Err(ParseError {
                        pos,
                        msg: format!("`include` expects a string, found {other}"),
                    })
                }
            };
            self.eat(P::Semi);
            return Ok(Item::Include { path, pos });
        }
        if self.at_kw("labels") {
            self.bump();
            let defs = self.labels_block()?;
            return Ok(Item::Labels { defs, pos });
        }
        if self.at_kw("struct") {
            return Ok(Item::Struct(self.struct_def()?));
        }
        if self.at_kw("static") || self.at_kw("const") {
            let vd = self.var_decl_stmt()?;
            return Ok(Item::Var(vd));
        }
        self.script_or_var_item()
    }

    /// `labels { A = 1, B, }` — trailing comma allowed, empty block allowed.
    fn labels_block(&mut self) -> Result<Vec<LabelDef>, ParseError> {
        self.expect(P::LBrace)?;
        let mut defs = Vec::new();
        loop {
            if self.eat(P::RBrace) {
                break;
            }
            if self.eat(P::Comma) {
                continue;
            }
            let pos = self.pos();
            let name = self.ident()?;
            let value = if self.eat(P::Assign) {
                Some(self.expr()?)
            } else {
                None
            };
            defs.push(LabelDef { name, value, pos });
            if !self.eat(P::Comma) {
                // The corpus always uses commas, but a `;`-separated or newline-separated
                // block would parse here too. `}` ends the block.
                self.eat(P::Semi);
            }
        }
        Ok(defs)
    }

    fn struct_def(&mut self) -> Result<StructDef, ParseError> {
        let pos = self.pos();
        self.bump(); // `struct`
        let name = self.ident()?;
        self.expect(P::LBrace)?;
        let mut fields = Vec::new();
        while !self.at(P::RBrace) {
            if self.eat(P::Semi) {
                continue;
            }
            let fpos = self.pos();
            let ty = self.type_ref()?;
            loop {
                let name = self.ident()?;
                let fixed_len = if self.eat(P::LBrack) {
                    let n = if self.at(P::RBrack) {
                        None
                    } else {
                        Some(self.expr()?)
                    };
                    self.expect(P::RBrack)?;
                    n
                } else {
                    None
                };
                fields.push(Field {
                    ty: ty.clone(),
                    name,
                    fixed_len,
                    pos: fpos,
                });
                if !self.eat(P::Comma) {
                    break;
                }
            }
            self.expect(P::Semi)?;
        }
        self.expect(P::RBrace)?;
        self.eat(P::Semi);
        Ok(StructDef { name, fields, pos })
    }

    /// A type: one identifier plus any number of `[]` suffixes.
    fn type_ref(&mut self) -> Result<TypeRef, ParseError> {
        let pos = self.pos();
        let name = self.ident()?;
        let mut array_depth = 0u8;
        while self.at(P::LBrack) && self.at_n(1, P::RBrack) {
            self.bump();
            self.bump();
            array_depth += 1;
        }
        Ok(TypeRef {
            name,
            array_depth,
            pos,
        })
    }

    /// A script definition/declaration, the anonymous entry script, or a file-scope
    /// variable. All three begin with a run of identifiers.
    fn script_or_var_item(&mut self) -> Result<Item, ParseError> {
        let pos = self.pos();
        if !self.is_ident_tok(0) {
            return Err(self.err(format!("expected a declaration, found {}", self.peek())));
        }
        let mut head: Vec<TypeRef> = Vec::new();
        loop {
            head.push(self.type_ref()?);
            if !self.is_ident_tok(0) {
                break;
            }
        }

        if self.at(P::LParen) {
            let sig = self.script_sig(head, pos)?;
            let body = if self.eat(P::Semi) {
                None
            } else if self.at(P::LBrace) {
                Some(self.block()?)
            } else {
                return Err(self.err(format!(
                    "expected `;` or `{{` after script `{}`, found {}",
                    sig.name,
                    self.peek()
                )));
            };
            return Ok(Item::Script(ScriptDef { sig, body }));
        }

        if self.at(P::LBrace) {
            // The anonymous per-frame entry script. Attested with and without a return
            // type: `scenario { … }`, `void scenario { … }`, `int scenario { … }`.
            let script_type = match head.len() {
                1 => head[0].name.clone(),
                2 => head[1].name.clone(),
                _ => {
                    return Err(ParseError {
                        pos,
                        msg: "anonymous entry script must be written as \
                              `[return-type] <script-type> { … }`"
                            .to_string(),
                    })
                }
            };
            let ret = if head.len() == 2 {
                Some(head[0].clone())
            } else {
                None
            };
            let body = self.block()?;
            return Ok(Item::Main(MainScript {
                script_type,
                ret,
                body,
                pos,
            }));
        }

        // Otherwise: a file-scope variable declaration whose head we already consumed.
        self.finish_var_decl(false, head, pos).map(Item::Var)
    }

    /// Split an already-parsed head run into return type, script-type qualifier and name.
    fn script_sig(&mut self, head: Vec<TypeRef>, pos: Pos) -> Result<ScriptSig, ParseError> {
        let mut head = head;
        let name_ty = head.pop().expect("head is never empty");
        if name_ty.array_depth != 0 {
            return Err(ParseError {
                pos,
                msg: "script name cannot carry `[]`".into(),
            });
        }
        let name = name_ty.name;
        let (ret, script_type) = match head.len() {
            0 => (None, None),
            1 => {
                let w = head.pop().unwrap();
                if w.array_depth == 0 && is_script_type(&w.name) {
                    (None, Some(w.name))
                } else {
                    (Some(w), None)
                }
            }
            2 => {
                let st = head.pop().unwrap();
                let rt = head.pop().unwrap();
                (Some(rt), Some(st.name))
            }
            _ => {
                return Err(ParseError {
                    pos,
                    msg: format!(
                        "script head has {} qualifiers; at most 2 are attested",
                        head.len()
                    ),
                })
            }
        };
        let params = self.params()?;
        Ok(ScriptSig {
            ret,
            script_type,
            name,
            params,
            pos,
        })
    }

    fn params(&mut self) -> Result<Vec<Param>, ParseError> {
        self.expect(P::LParen)?;
        let mut out = Vec::new();
        if self.eat(P::RParen) {
            return Ok(out);
        }
        // `(void)` is the empty parameter list.
        if self.at_kw("void") && self.at_n(1, P::RParen) {
            self.bump();
            self.bump();
            return Ok(out);
        }
        loop {
            let pos = self.pos();
            let by_ref = self.eat_kw("ref");
            // `const` appears only inside comments in the shipped corpus, but the
            // tokenizer would reach here if it ever appeared in code.
            self.eat_kw("const");
            let first = self.type_ref()?;
            let (ty, name) = if self.is_ident_tok(0) {
                let n = self.ident()?;
                (Some(first), n)
            } else {
                if first.array_depth != 0 {
                    return Err(ParseError {
                        pos,
                        msg: "parameter name cannot carry `[]`".into(),
                    });
                }
                (None, first.name)
            };
            // A trailing `[]` on the name: `int arr[]`.
            let mut ty = ty;
            if self.at(P::LBrack) && self.at_n(1, P::RBrack) {
                self.bump();
                self.bump();
                if let Some(t) = ty.as_mut() {
                    t.array_depth += 1;
                }
            }
            out.push(Param {
                by_ref,
                ty,
                name,
                pos,
            });
            if !self.eat(P::Comma) {
                break;
            }
        }
        self.expect(P::RParen)?;
        Ok(out)
    }

    // ---- statements ----------------------------------------------------------

    fn block(&mut self) -> Result<Block, ParseError> {
        let pos = self.pos();
        self.expect(P::LBrace)?;
        let mut stmts = Vec::new();
        while !self.at(P::RBrace) {
            if self.at_eof() {
                return Err(ParseError {
                    pos,
                    msg: "unterminated `{` block".into(),
                });
            }
            stmts.push(self.stmt()?);
        }
        self.expect(P::RBrace)?;
        Ok(Block { stmts, pos })
    }

    /// Does a declaration start here?
    ///
    /// Shapes, all attested:
    /// * `static …`
    /// * `Ident Ident` — `int i`, `String s`, `ConquestDiploOffer offer`
    /// * `Ident [ ] Ident` — `int[] units`
    fn at_decl(&self) -> bool {
        if self.at_kw("static") || self.at_kw("const") {
            return true;
        }
        if !self.is_ident_tok(0) || is_keyword(self.kw_at(0)) {
            return false;
        }
        if self.is_ident_tok(1) && !is_keyword(self.kw_at(1)) {
            return true;
        }
        if self.at_n(1, P::LBrack) && self.at_n(2, P::RBrack) && self.is_ident_tok(3) {
            return true;
        }
        false
    }

    fn kw_at(&self, n: usize) -> &str {
        match self.peek_at(n) {
            Tok::Ident(s) => s,
            _ => "",
        }
    }

    fn stmt(&mut self) -> Result<Stmt, ParseError> {
        let pos = self.pos();
        if self.eat(P::Semi) {
            return Ok(Stmt::Empty(pos));
        }
        if self.at(P::LBrace) {
            return Ok(Stmt::Block(self.block()?));
        }
        if self.at_kw("struct") {
            return Ok(Stmt::Struct(self.struct_def()?));
        }
        if self.at_kw("labels") {
            self.bump();
            let defs = self.labels_block()?;
            return Ok(Stmt::Labels { defs, pos });
        }
        if self.at_kw("if") {
            self.bump();
            self.expect(P::LParen)?;
            let cond = self.expr()?;
            self.expect(P::RParen)?;
            let then = Box::new(self.stmt()?);
            let els = if self.eat_kw("else") {
                Some(Box::new(self.stmt()?))
            } else {
                None
            };
            return Ok(Stmt::If {
                cond,
                then,
                els,
                pos,
            });
        }
        if self.at_kw("while") {
            self.bump();
            self.expect(P::LParen)?;
            let cond = self.expr()?;
            self.expect(P::RParen)?;
            let body = Box::new(self.stmt()?);
            return Ok(Stmt::While { cond, body, pos });
        }
        if self.at_kw("do") {
            self.bump();
            let body = Box::new(self.stmt()?);
            if !self.eat_kw("while") {
                return Err(self.err("expected `while` after `do` body"));
            }
            self.expect(P::LParen)?;
            let cond = self.expr()?;
            self.expect(P::RParen)?;
            self.expect(P::Semi)?;
            return Ok(Stmt::DoWhile { body, cond, pos });
        }
        if self.at_kw("for") {
            self.bump();
            self.expect(P::LParen)?;
            let init = if self.at(P::Semi) {
                self.bump();
                None
            } else if self.at_decl() {
                Some(Box::new(Stmt::Decl(self.var_decl_stmt()?)))
            } else {
                let e = self.expr()?;
                self.expect(P::Semi)?;
                Some(Box::new(Stmt::Expr(e)))
            };
            let cond = if self.at(P::Semi) {
                None
            } else {
                Some(self.expr()?)
            };
            self.expect(P::Semi)?;
            let step = if self.at(P::RParen) {
                None
            } else {
                Some(self.expr()?)
            };
            self.expect(P::RParen)?;
            let body = Box::new(self.stmt()?);
            return Ok(Stmt::For {
                init,
                cond,
                step,
                body,
                pos,
            });
        }
        if self.at_kw("switch") {
            return self.switch_stmt();
        }
        if self.at_kw("break") {
            self.bump();
            self.expect(P::Semi)?;
            return Ok(Stmt::Break(pos));
        }
        if self.at_kw("continue") {
            self.bump();
            self.expect(P::Semi)?;
            return Ok(Stmt::Continue(pos));
        }
        if self.at_kw("return") {
            self.bump();
            let value = if self.at(P::Semi) {
                None
            } else {
                Some(self.expr()?)
            };
            self.expect(P::Semi)?;
            return Ok(Stmt::Return { value, pos });
        }
        if self.at_kw("run_once") {
            self.bump();
            let body = self.block()?;
            return Ok(Stmt::RunOnce { body, pos });
        }
        if self.at_kw("trigger") {
            self.bump();
            // `trigger name(cond) { … }`, `trigger (cond) { … }`, `trigger name() { … }`
            let name = if self.is_ident_tok(0) {
                Some(self.ident()?)
            } else {
                None
            };
            let cond = if self.eat(P::LParen) {
                let c = if self.at(P::RParen) {
                    None
                } else {
                    Some(self.expr()?)
                };
                self.expect(P::RParen)?;
                c
            } else {
                None
            };
            // The body is any statement, not necessarily a block:
            // `trigger (num_units(attacker) < 1) defeat(attacker);` is attested.
            let body = Box::new(self.stmt()?);
            return Ok(Stmt::Trigger {
                name,
                cond,
                body,
                pos,
            });
        }
        if self.at_decl() {
            return Ok(Stmt::Decl(self.var_decl_stmt()?));
        }
        let e = self.expr()?;
        // The terminating `;` is **optional** on an expression statement. Not a design
        // choice: `conquest/Alexander/thrace.bhs:269` and `thrace2.bhs:189` both ship a
        // bare `enable_trigger("add_merchant")` with no semicolon, followed by another
        // statement. `include "x.bhs"` likewise carries none. Every other statement form
        // requires its terminator, and the corpus sustains that.
        self.eat(P::Semi);
        Ok(Stmt::Expr(e))
    }

    fn switch_stmt(&mut self) -> Result<Stmt, ParseError> {
        let pos = self.pos();
        self.bump(); // `switch`
        self.expect(P::LParen)?;
        let subject = self.expr()?;
        self.expect(P::RParen)?;
        self.expect(P::LBrace)?;
        let mut arms: Vec<SwitchArm> = Vec::new();
        while !self.at(P::RBrace) {
            if self.at_eof() {
                return Err(ParseError {
                    pos,
                    msg: "unterminated `switch` block".into(),
                });
            }
            if self.at_kw("case") || self.at_kw("default") {
                let lpos = self.pos();
                let is_default = self.at_kw("default");
                self.bump();
                let value = if is_default { None } else { Some(self.expr()?) };
                self.expect(P::Colon)?;
                let label = SwitchCase { value, pos: lpos };
                match arms.last_mut() {
                    // Consecutive labels with no statements between them share a body.
                    Some(a) if a.body.is_empty() => a.labels.push(label),
                    _ => arms.push(SwitchArm {
                        labels: vec![label],
                        body: Vec::new(),
                    }),
                }
            } else {
                let s = self.stmt()?;
                match arms.last_mut() {
                    Some(a) => a.body.push(s),
                    None => {
                        // Statements before the first `case` are unreachable but legal.
                        arms.push(SwitchArm {
                            labels: Vec::new(),
                            body: vec![s],
                        })
                    }
                }
            }
        }
        self.expect(P::RBrace)?;
        Ok(Stmt::Switch { subject, arms, pos })
    }

    fn var_decl_stmt(&mut self) -> Result<VarDecl, ParseError> {
        let pos = self.pos();
        let mut is_static = false;
        loop {
            if self.at_kw("static") {
                self.bump();
                is_static = true;
                continue;
            }
            if self.at_kw("const") {
                self.bump();
                continue;
            }
            break;
        }
        // `static have_objective = false;` — a static with no written type.
        let head = if self.is_ident_tok(0)
            && (self.is_ident_tok(1) || (self.at_n(1, P::LBrack) && self.at_n(2, P::RBrack)))
        {
            vec![self.type_ref()?]
        } else {
            Vec::new()
        };
        self.finish_var_decl(is_static, head, pos)
    }

    /// Finish a declaration whose type head (0 or 1 `TypeRef`) has been consumed.
    fn finish_var_decl(
        &mut self,
        is_static: bool,
        head: Vec<TypeRef>,
        pos: Pos,
    ) -> Result<VarDecl, ParseError> {
        let ty = match head.len() {
            0 => None,
            1 => Some(head.into_iter().next().unwrap()),
            _ => {
                return Err(ParseError {
                    pos,
                    msg: "declaration has more than one type word".into(),
                })
            }
        };
        let mut decls = Vec::new();
        loop {
            let dpos = self.pos();
            let name = self.ident()?;
            let array = if self.eat(P::LBrack) {
                let a = if self.at(P::RBrack) {
                    ArraySuffix::Dynamic
                } else {
                    ArraySuffix::Sized(self.expr()?)
                };
                self.expect(P::RBrack)?;
                Some(a)
            } else {
                None
            };
            let init = if self.eat(P::Assign) {
                Some(self.expr()?)
            } else {
                None
            };
            decls.push(Declarator {
                name,
                array,
                init,
                pos: dpos,
            });
            if !self.eat(P::Comma) {
                break;
            }
        }
        self.expect(P::Semi)?;
        Ok(VarDecl {
            is_static,
            ty,
            decls,
            pos,
        })
    }

    // ---- expressions ---------------------------------------------------------

    pub fn expr(&mut self) -> Result<Expr, ParseError> {
        self.assign_expr()
    }

    fn assign_expr(&mut self) -> Result<Expr, ParseError> {
        let lhs = self.binary_expr(0)?;
        let op = match self.peek() {
            Tok::Punct(p)
                if matches!(
                    p,
                    P::Assign
                        | P::AddAssign
                        | P::SubAssign
                        | P::MulAssign
                        | P::DivAssign
                        | P::ModAssign
                        | P::PowAssign
                        | P::LeftAssign
                        | P::RightAssign
                        | P::AndAssign
                        | P::XorAssign
                        | P::OrAssign
                ) =>
            {
                *p
            }
            _ => return Ok(lhs),
        };
        let pos = self.pos();
        self.bump();
        let value = self.assign_expr()?; // right-associative
        Ok(Expr::Assign {
            op,
            target: Box::new(lhs),
            value: Box::new(value),
            pos,
        })
    }

    /// Binding powers, lowest first. C's table, with `**` inserted above `* / %`.
    ///
    /// **There are no word operators.** An earlier survey counted `and` 845, `or` 138
    /// and `not` 394 across the corpus; every one of those is inside a comment or a
    /// string literal. Blanking comments and string literals leaves **zero** occurrences
    /// of `and`, `or` or `not` in code, and the retail flex DFA agrees: none of the three
    /// is a rule, so all three lex as ordinary identifiers.
    ///
    /// The corpus contains no `**` in code either, so its precedence is unattested; see
    /// `docs/tracks/bhs-grammar.md` §"Unattested".
    fn bin_prec(&self) -> Option<(P, u8)> {
        let p = match self.peek() {
            Tok::Punct(p) => *p,
            _ => return None,
        };
        let prec = match p {
            P::OrOr => 1,
            P::AndAnd => 2,
            P::Pipe => 3,
            P::Caret => 4,
            P::Amp => 5,
            P::Eq | P::Ne => 6,
            P::Lt | P::Gt | P::Le | P::Ge => 7,
            P::Shl | P::Shr => 8,
            P::Plus | P::Minus => 9,
            P::Star | P::Slash | P::Percent => 10,
            P::Pow => 11,
            _ => return None,
        };
        Some((p, prec))
    }

    fn binary_expr(&mut self, min_prec: u8) -> Result<Expr, ParseError> {
        let mut lhs = self.unary_expr()?;
        while let Some((op, prec)) = self.bin_prec() {
            if prec < min_prec {
                break;
            }
            let pos = self.pos();
            self.bump();
            // `**` is right-associative; everything else is left-associative.
            let next_min = if op == P::Pow { prec } else { prec + 1 };
            let rhs = self.binary_expr(next_min)?;
            lhs = Expr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
                pos,
            };
        }
        Ok(lhs)
    }

    /// Is a C-style cast starting here?
    ///
    /// `( <scalar-type> )`. Restricting the target to the scalar type names keeps this
    /// unambiguous against a parenthesised expression: `(x)` where `x` is a variable
    /// stays an expression. The corpus casts only to `int` (137), `float` (4) and
    /// `void` (3); no cast to a struct or array type occurs.
    fn at_cast(&self) -> bool {
        if !self.at(P::LParen) || !self.is_ident_tok(1) || !self.at_n(2, P::RParen) {
            return false;
        }
        matches!(
            self.kw_at(1).to_ascii_lowercase().as_str(),
            "int" | "float" | "real" | "string" | "void"
        )
    }

    fn unary_expr(&mut self) -> Result<Expr, ParseError> {
        let pos = self.pos();
        if self.at_cast() {
            self.bump(); // `(`
            let ty = self.type_ref()?;
            self.expect(P::RParen)?;
            let expr = self.unary_expr()?;
            return Ok(Expr::Cast {
                ty,
                expr: Box::new(expr),
                pos,
            });
        }
        match self.peek() {
            Tok::Punct(P::Not)
            | Tok::Punct(P::Minus)
            | Tok::Punct(P::Tilde)
            | Tok::Punct(P::Plus) => {
                let op = match self.bump() {
                    Tok::Punct(p) => p,
                    _ => unreachable!(),
                };
                let e = self.unary_expr()?;
                if op == P::Plus {
                    // Unary `+` has no opcode; it is the identity.
                    return Ok(e);
                }
                Ok(Expr::Unary {
                    op,
                    expr: Box::new(e),
                    pos,
                })
            }
            Tok::Punct(P::Inc) | Tok::Punct(P::Dec) => {
                let inc = self.at(P::Inc);
                self.bump();
                let e = self.unary_expr()?;
                Ok(Expr::PreIncDec {
                    inc,
                    expr: Box::new(e),
                    pos,
                })
            }
            _ => self.postfix_expr(),
        }
    }

    fn postfix_expr(&mut self) -> Result<Expr, ParseError> {
        let mut e = self.primary_expr()?;
        loop {
            let pos = self.pos();
            if self.at(P::LBrack) {
                self.bump();
                let index = self.expr()?;
                self.expect(P::RBrack)?;
                e = Expr::Index {
                    base: Box::new(e),
                    index: Box::new(index),
                    pos,
                };
            } else if self.at(P::Dot) {
                self.bump();
                let name = self.ident()?;
                if self.at(P::LParen) {
                    let args = self.call_args()?;
                    e = Expr::MethodCall {
                        recv: Box::new(e),
                        name,
                        args,
                        pos,
                    };
                } else {
                    e = Expr::Member {
                        base: Box::new(e),
                        name,
                        pos,
                    };
                }
            } else if self.at(P::Inc) || self.at(P::Dec) {
                let inc = self.at(P::Inc);
                self.bump();
                e = Expr::PostIncDec {
                    inc,
                    expr: Box::new(e),
                    pos,
                };
            } else {
                return Ok(e);
            }
        }
    }

    fn call_args(&mut self) -> Result<Vec<Expr>, ParseError> {
        self.expect(P::LParen)?;
        let mut args = Vec::new();
        if self.eat(P::RParen) {
            return Ok(args);
        }
        loop {
            args.push(self.expr()?);
            if !self.eat(P::Comma) {
                break;
            }
        }
        self.expect(P::RParen)?;
        Ok(args)
    }

    fn primary_expr(&mut self) -> Result<Expr, ParseError> {
        let pos = self.pos();
        match self.peek().clone() {
            Tok::Int(v) => {
                self.bump();
                Ok(Expr::Int(v, pos))
            }
            Tok::Real(v) => {
                self.bump();
                Ok(Expr::Real(v, pos))
            }
            Tok::Str(s) => {
                // **Adjacent string literals concatenate**, exactly as in C. Attested
                // beyond doubt at `scenario/Custom/italy_mp/italy_mp.bhs:334-336`, where
                // one assignment spans three quoted fragments on three lines with no
                // operator between them.
                //
                // This also settles `scenario/scriptlibrary/ctw_lib.bhs:326`, where an
                // array literal reads `["House D" "House D1", "House D2", …]`: the
                // missing comma is a *shipped bug*, and the correct compilation makes
                // element 0 the string `"House DHouse D1"` and the array one shorter
                // than the author intended. A faithful compiler reproduces that.
                self.bump();
                let mut s = s;
                while let Tok::Str(next) = self.peek().clone() {
                    self.bump();
                    s.push_str(&next);
                }
                Ok(Expr::Str(s, pos))
            }
            Tok::LocStr(s) => {
                self.bump();
                Ok(Expr::LocStr(s, pos))
            }
            Tok::Punct(P::LParen) => {
                self.bump();
                let e = self.expr()?;
                self.expect(P::RParen)?;
                Ok(e)
            }
            Tok::Punct(P::LBrack) => {
                self.bump();
                let mut items = Vec::new();
                if !self.at(P::RBrack) {
                    loop {
                        items.push(self.expr()?);
                        if !self.eat(P::Comma) {
                            break;
                        }
                        if self.at(P::RBrack) {
                            break; // trailing comma
                        }
                    }
                }
                self.expect(P::RBrack)?;
                Ok(Expr::ArrayLit(items, pos))
            }
            Tok::Ident(name) => {
                if name.eq_ignore_ascii_case("true") {
                    self.bump();
                    return Ok(Expr::Int(1, pos));
                }
                if name.eq_ignore_ascii_case("false") {
                    self.bump();
                    return Ok(Expr::Int(0, pos));
                }
                if is_keyword(&name) {
                    return Err(ParseError {
                        pos,
                        msg: format!("`{name}` is a keyword and cannot start an expression"),
                    });
                }
                self.bump();
                if self.at(P::LParen) {
                    let args = self.call_args()?;
                    Ok(Expr::Call { name, args, pos })
                } else {
                    Ok(Expr::Name(name, pos))
                }
            }
            other => Err(ParseError {
                pos,
                msg: format!("unexpected {other} in expression"),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(src: &str) -> SourceFile {
        parse_file("<test>", src).unwrap_or_else(|e| panic!("parse failed: {e}"))
    }

    #[test]
    fn anonymous_entry_script() {
        let f = p("scenario\n{\n  int i = 1;\n}\n");
        assert!(matches!(f.items[0], Item::Main(_)));
    }

    #[test]
    fn forward_decl_then_definition() {
        let f = p("void conquest foo(int who);\nvoid conquest foo(int who) { return; }\n");
        assert_eq!(f.items.len(), 2);
        match (&f.items[0], &f.items[1]) {
            (Item::Script(a), Item::Script(b)) => {
                assert!(a.body.is_none());
                assert!(b.body.is_some());
                assert_eq!(a.sig.script_type.as_deref(), Some("conquest"));
                assert_eq!(a.sig.ret.as_ref().unwrap().name, "void");
            }
            _ => panic!("expected two scripts"),
        }
    }

    #[test]
    fn untyped_params_and_no_return_type() {
        let f = p("scenario all_gen_entrench (Player) { }");
        match &f.items[0] {
            Item::Script(s) => {
                assert_eq!(s.sig.script_type.as_deref(), Some("scenario"));
                assert_eq!(s.sig.name, "all_gen_entrench");
                assert!(s.sig.params[0].ty.is_none());
            }
            _ => panic!(),
        }
    }

    #[test]
    fn labels_auto_numbering_shape() {
        let f = p("labels {\n A = 1,\n B,\n C,\n}\n");
        match &f.items[0] {
            Item::Labels { defs, .. } => {
                assert_eq!(defs.len(), 3);
                assert!(defs[0].value.is_some());
                assert!(defs[1].value.is_none());
            }
            _ => panic!(),
        }
    }

    #[test]
    fn method_call_is_receiver_sugar() {
        let f = p("scenario { g.add_to_group(u); }");
        match &f.items[0] {
            Item::Main(m) => match &m.body.stmts[0] {
                Stmt::Expr(Expr::MethodCall { name, args, .. }) => {
                    assert_eq!(name, "add_to_group");
                    assert_eq!(args.len(), 1);
                }
                s => panic!("{s:?}"),
            },
            _ => panic!(),
        }
    }

    #[test]
    fn switch_groups_fallthrough_labels() {
        let f = p("scenario { switch (x) { case 0: case 1: y(); break; default: z(); } }");
        match &f.items[0] {
            Item::Main(m) => match &m.body.stmts[0] {
                Stmt::Switch { arms, .. } => {
                    assert_eq!(arms.len(), 2);
                    assert_eq!(arms[0].labels.len(), 2);
                    assert!(arms[1].labels[0].value.is_none());
                }
                s => panic!("{s:?}"),
            },
            _ => panic!(),
        }
    }

    #[test]
    fn there_are_no_word_operators() {
        // `and` / `or` / `not` are ordinary identifiers: the retail lexer has no rule for
        // them and the corpus contains none in code. `not(x)` is therefore a CALL.
        let f = p("scenario { y = not(x); }");
        match &f.items[0] {
            Item::Main(m) => match &m.body.stmts[0] {
                Stmt::Expr(Expr::Assign { value, .. }) => {
                    assert!(matches!(**value, Expr::Call { .. }), "{value:?}");
                }
                s => panic!("{s:?}"),
            },
            _ => panic!(),
        }
    }

    #[test]
    fn struct_with_dynamic_arrays() {
        let f = p("struct UnitGroup { int nation; int[] units; };");
        match &f.items[0] {
            Item::Struct(s) => {
                assert_eq!(s.fields.len(), 2);
                assert_eq!(s.fields[1].ty.array_depth, 1);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn do_while_and_trailing_semicolon() {
        let f = p("scenario { do { x(); } while (y > -1); }");
        match &f.items[0] {
            Item::Main(m) => assert!(matches!(m.body.stmts[0], Stmt::DoWhile { .. })),
            _ => panic!(),
        }
    }
}
