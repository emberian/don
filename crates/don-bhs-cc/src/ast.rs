//! The BHS abstract syntax tree.
//!
//! Shaped by what the 363-file shipped corpus actually contains. Notably:
//!
//! * A script *signature* carries an optional return type **and** an optional
//!   *script-type qualifier* (`ai`, `scenario`, `conquest`) — both are optional and
//!   both are plain identifiers at the token level, so the parser classifies by
//!   position, not by a reserved-word list.
//! * A file may contain one **anonymous** script (`scenario { … }`), which is the
//!   per-frame entry point `Game::do_frame` calls.
//! * `labels { A = 1, B, }` is an enum-like constant block that occurs both at file
//!   scope and as a statement inside a script body.
//! * Parameters and locals may be **untyped** (`scenario all_gen_entrench (Player)`),
//!   and variables may be used with no declaration at all.

use crate::lex::{Pos, P};

/// A written type. `name` is whatever identifier the author used; BHS type names are
/// case-insensitive in the corpus (`String` and `string` both occur, as do `int`/`Int`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeRef {
    pub name: String,
    /// Number of `[]` suffixes. Only 0 and 1 occur in the corpus.
    pub array_depth: u8,
    pub pos: Pos,
}

/// One `struct` declaration.
#[derive(Debug, Clone)]
pub struct StructDef {
    pub name: String,
    pub fields: Vec<Field>,
    pub pos: Pos,
}

#[derive(Debug, Clone)]
pub struct Field {
    pub ty: TypeRef,
    pub name: String,
    /// `int grid[8];` — a fixed-size field. Absent for `int[] units;`.
    pub fixed_len: Option<Expr>,
    pub pos: Pos,
}

/// One entry of a `labels { … }` block.
#[derive(Debug, Clone)]
pub struct LabelDef {
    pub name: String,
    /// `A = 1`. When absent the value is the previous entry's value plus one,
    /// starting from 0 — see `docs/tracks/bhs-grammar.md` §"labels".
    pub value: Option<Expr>,
    pub pos: Pos,
}

/// A formal parameter.
#[derive(Debug, Clone)]
pub struct Param {
    /// `ref int step` — by-reference. 75 occurrences in the corpus.
    pub by_ref: bool,
    /// `None` for the untyped form `civil_war_by_percent (Empire, Rebels, percent)`.
    pub ty: Option<TypeRef>,
    pub name: String,
    pub pos: Pos,
}

/// A script signature: the part before `{` or `;`.
#[derive(Debug, Clone)]
pub struct ScriptSig {
    /// `None` for `scenario all_gen_entrench (Player)`, which declares no return type.
    pub ret: Option<TypeRef>,
    /// `ai` / `scenario` / `conquest`. `None` when the author wrote none.
    pub script_type: Option<String>,
    pub name: String,
    pub params: Vec<Param>,
    pub pos: Pos,
}

/// A script definition or a forward declaration.
#[derive(Debug, Clone)]
pub struct ScriptDef {
    pub sig: ScriptSig,
    /// `None` for a forward declaration ending in `;`.
    pub body: Option<Block>,
}

/// The anonymous per-frame entry script: `scenario { … }`. Attested with a return type
/// too (`void scenario { … }`, `int scenario { … }`), which the engine ignores because
/// `Game::do_frame` discards the value.
#[derive(Debug, Clone)]
pub struct MainScript {
    pub script_type: String,
    pub ret: Option<TypeRef>,
    pub body: Block,
    pub pos: Pos,
}

/// A file-scope item.
#[derive(Debug, Clone)]
pub enum Item {
    Include {
        path: String,
        pos: Pos,
    },
    Labels {
        defs: Vec<LabelDef>,
        pos: Pos,
    },
    Struct(StructDef),
    Script(ScriptDef),
    Main(MainScript),
    /// A file-scope variable declaration.
    Var(VarDecl),
}

#[derive(Debug, Clone, Default)]
pub struct SourceFile {
    pub path: String,
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, Default)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub pos: Pos,
}

/// One declarator inside a declaration statement: `static int a = 1, b[] = [2,3];`
#[derive(Debug, Clone)]
pub struct Declarator {
    pub name: String,
    /// `[]` present (dynamic array) or `[n]` (sized array).
    pub array: Option<ArraySuffix>,
    pub init: Option<Expr>,
    pub pos: Pos,
}

#[derive(Debug, Clone)]
pub enum ArraySuffix {
    /// `int a[];`
    Dynamic,
    /// `int a[8];`
    Sized(Expr),
}

#[derive(Debug, Clone)]
pub struct VarDecl {
    /// `static int x` — persists across frames in `Script::static_vars`.
    pub is_static: bool,
    /// `None` for the untyped form `static have_objective = false;`.
    pub ty: Option<TypeRef>,
    pub decls: Vec<Declarator>,
    pub pos: Pos,
}

#[derive(Debug, Clone)]
pub struct SwitchCase {
    /// `None` for `default:`.
    pub value: Option<Expr>,
    pub pos: Pos,
}

#[derive(Debug, Clone)]
pub struct SwitchArm {
    /// One or more `case`/`default` labels sharing a body (fallthrough grouping).
    pub labels: Vec<SwitchCase>,
    pub body: Vec<Stmt>,
}

#[derive(Debug, Clone)]
pub enum Stmt {
    Empty(Pos),
    Expr(Expr),
    Decl(VarDecl),
    Block(Block),
    If {
        cond: Expr,
        then: Box<Stmt>,
        els: Option<Box<Stmt>>,
        pos: Pos,
    },
    While {
        cond: Expr,
        body: Box<Stmt>,
        pos: Pos,
    },
    DoWhile {
        body: Box<Stmt>,
        cond: Expr,
        pos: Pos,
    },
    For {
        init: Option<Box<Stmt>>,
        cond: Option<Expr>,
        step: Option<Expr>,
        body: Box<Stmt>,
        pos: Pos,
    },
    Switch {
        subject: Expr,
        arms: Vec<SwitchArm>,
        pos: Pos,
    },
    Break(Pos),
    Continue(Pos),
    Return {
        value: Option<Expr>,
        pos: Pos,
    },
    /// `labels { … }` written inside a body. Scope is the enclosing script.
    Labels {
        defs: Vec<LabelDef>,
        pos: Pos,
    },
    /// `trigger Name(cond) { … }` — a guarded block whose enable bit lives in
    /// `Script::trigger_bits` and is toggled by the `enable_trigger` /
    /// `disable_trigger` builtins. The name is optional in the corpus
    /// (`trigger (num_cities(1) > 0) { … }`).
    Trigger {
        name: Option<String>,
        cond: Option<Expr>,
        /// Usually a block, but a bare statement is attested:
        /// `trigger (num_units(a) < 1) defeat(a);`
        body: Box<Stmt>,
        pos: Pos,
    },
    /// `run_once { … }` — runs on the first frame only. Modelled as an anonymous
    /// trigger that disables itself; see `docs/tracks/bhs-grammar.md`.
    RunOnce {
        body: Block,
        pos: Pos,
    },
    /// A nested `struct` declaration inside a body (occurs in the corpus).
    Struct(StructDef),
}

#[derive(Debug, Clone)]
pub enum Expr {
    Int(i64, Pos),
    Real(f32, Pos),
    /// A plain string literal.
    Str(String, Pos),
    /// `$S("…")`. Kept distinct from `Str` because it is a different *source* form;
    /// whether it is a different *runtime* value is recorded in the grammar doc.
    LocStr(String, Pos),
    /// A bare name: a local, a static, a parameter, a `labels` constant, or an
    /// implicitly-declared variable.
    Name(String, Pos),
    /// `[1, 2, 3]` — an array literal (`OP_CREATE_ARRAY_INITER`).
    ArrayLit(Vec<Expr>, Pos),
    /// A C-style cast: `(int)x`, `(float)y`, `(void)f()`. 144 occurrences in the corpus,
    /// all to a scalar type. Compiles to `OP_CAST` with the target's `SymType` tag.
    Cast {
        ty: TypeRef,
        expr: Box<Expr>,
        pos: Pos,
    },
    Unary {
        op: P,
        expr: Box<Expr>,
        pos: Pos,
    },
    /// `++x` / `--x`
    PreIncDec {
        inc: bool,
        expr: Box<Expr>,
        pos: Pos,
    },
    /// `x++` / `x--`
    PostIncDec {
        inc: bool,
        expr: Box<Expr>,
        pos: Pos,
    },
    Binary {
        op: P,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
        pos: Pos,
    },
    /// Assignment, including the compound forms.
    Assign {
        op: P,
        target: Box<Expr>,
        value: Box<Expr>,
        pos: Pos,
    },
    /// `f(a, b)` — resolved later to a script call or an engine builtin.
    Call {
        name: String,
        args: Vec<Expr>,
        pos: Pos,
    },
    /// `g.add_to_group(u)` — receiver-first sugar for `add_to_group(g, u)`.
    MethodCall {
        recv: Box<Expr>,
        name: String,
        args: Vec<Expr>,
        pos: Pos,
    },
    /// `a[i]`
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
        pos: Pos,
    },
    /// `s.field`, and the built-in pseudo-field `a.length`.
    Member {
        base: Box<Expr>,
        name: String,
        pos: Pos,
    },
}

impl Expr {
    pub fn pos(&self) -> Pos {
        use Expr::*;
        match self {
            Int(_, p)
            | Real(_, p)
            | Str(_, p)
            | LocStr(_, p)
            | Name(_, p)
            | ArrayLit(_, p)
            | Cast { pos: p, .. }
            | Unary { pos: p, .. }
            | PreIncDec { pos: p, .. }
            | PostIncDec { pos: p, .. }
            | Binary { pos: p, .. }
            | Assign { pos: p, .. }
            | Call { pos: p, .. }
            | MethodCall { pos: p, .. }
            | Index { pos: p, .. }
            | Member { pos: p, .. } => *p,
        }
    }
}
