//! pdb-extract — turn a shipped MSVC PDB into machine-readable ground truth.
//!
//! Emits two artifacts:
//!   * symbols.json — every function: VA/RVA, PDB name, mangled name, demangled
//!     signature, size, owning .obj module, source file + line span.
//!   * types.json   — the type catalogue: classes/structs/unions with field
//!     names, types, offsets, sizes; base classes; virtual method slots; enums.
//!
//! Usage:
//!   pdb-extract <rise.pdb> <image_base_hex> <out_symbols.json> <out_types.json>

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::File;

use pdb::FallibleIterator;
use serde::Serialize;

type Ti = pdb::TypeIndex;

// ---------------------------------------------------------------------------
// symbols.json
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct FunctionRec {
    /// Virtual address as a 0x-prefixed 8-hex-digit string (image base applied).
    va: String,
    /// Relative virtual address (from the PE image base).
    rva: u32,
    /// PDB procedure name: fully qualified, no argument types (`Class::method`).
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    mangled: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    demangled: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    size: Option<u32>,
    /// "global" (S_GPROC32), "local" (S_LPROC32, i.e. static/internal linkage),
    /// "thunk" (S_THUNK32), or "public" (S_PUB32 only — no procedure record).
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    module: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    line_end: Option<u32>,
    /// Type index of the procedure's function type, when the PDB records one.
    #[serde(skip_serializing_if = "Option::is_none")]
    type_index: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    signature: Option<String>,
}

#[derive(Serialize)]
struct GlobalRec {
    va: String,
    rva: u32,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    mangled: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    demangled: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    ty: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    type_index: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    size: Option<u32>,
    /// "global" (S_GDATA32), "static" (S_LDATA32, internal linkage),
    /// "public" (S_PUB32 data with no typed record).
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    module: Option<String>,
}

// ---------------------------------------------------------------------------
// types.json
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct FieldRec {
    name: String,
    #[serde(rename = "type")]
    ty: String,
    type_index: u32,
    offset: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    size: Option<u32>,
    /// Present only for bitfields.
    #[serde(skip_serializing_if = "Option::is_none")]
    bit_offset: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bit_length: Option<u8>,
}

#[derive(Serialize)]
struct BaseRec {
    name: String,
    offset: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    size: Option<u32>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    virtual_base: bool,
}

#[derive(Serialize)]
struct StaticRec {
    name: String,
    #[serde(rename = "type")]
    ty: String,
}

#[derive(Serialize)]
struct MethodRec {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    signature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    vtable_offset: Option<u32>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    is_virtual: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    is_static: bool,
}

#[derive(Serialize)]
struct TypeRec {
    kind: &'static str, // "class" | "struct" | "interface" | "union"
    #[serde(skip_serializing_if = "Option::is_none")]
    size: Option<u32>,
    unique_name: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    bases: Vec<BaseRec>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    fields: Vec<FieldRec>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    statics: Vec<StaticRec>,
    /// Virtual methods only — each carries its introducing vtable slot where the
    /// PDB records one. Non-virtual methods are deliberately omitted: every
    /// *emitted* method already appears in symbols.json with its full signature
    /// and address, and keeping them here tripled the file for no new facts.
    /// `methods_declared` is the total the TPI declared, so nothing is hidden.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    methods: Vec<MethodRec>,
    #[serde(skip_serializing_if = "is_zero")]
    methods_declared: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    nested: Vec<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    has_vftable: bool,
}

#[derive(Serialize)]
struct EnumRec {
    underlying: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    size: Option<u32>,
    values: Vec<(String, i64)>,
}

// ---------------------------------------------------------------------------

struct TypeCtx<'a> {
    finder: pdb::TypeFinder<'a>,
    /// name -> index of the *definition* (non-forward-reference) record.
    defs: HashMap<String, Ti>,
    /// unique_name -> index of the definition.
    udefs: HashMap<String, Ti>,
    size_cache: HashMap<u32, Option<u32>>,
    name_cache: HashMap<u32, String>,
    ptr_size: u32,
}

impl<'a> TypeCtx<'a> {
    fn find(&self, ti: Ti) -> Option<pdb::TypeData<'a>> {
        self.finder.find(ti).ok().and_then(|i| i.parse().ok())
    }

    /// Resolve a forward reference to its definition where one exists.
    fn resolve(&self, ti: Ti) -> Ti {
        match self.find(ti) {
            Some(pdb::TypeData::Class(c)) if c.properties.forward_reference() => c
                .unique_name
                .and_then(|u| self.udefs.get(&u.to_string().into_owned()).copied())
                .or_else(|| self.defs.get(&c.name.to_string().into_owned()).copied())
                .unwrap_or(ti),
            Some(pdb::TypeData::Union(u)) if u.properties.forward_reference() => u
                .unique_name
                .and_then(|n| self.udefs.get(&n.to_string().into_owned()).copied())
                .or_else(|| self.defs.get(&u.name.to_string().into_owned()).copied())
                .unwrap_or(ti),
            Some(pdb::TypeData::Enumeration(e)) if e.properties.forward_reference() => e
                .unique_name
                .and_then(|n| self.udefs.get(&n.to_string().into_owned()).copied())
                .or_else(|| self.defs.get(&e.name.to_string().into_owned()).copied())
                .unwrap_or(ti),
            _ => ti,
        }
    }

    fn size_of(&mut self, ti: Ti) -> Option<u32> {
        self.size_inner(ti, 0)
    }

    fn size_inner(&mut self, ti: Ti, depth: u32) -> Option<u32> {
        if depth > 24 {
            return None;
        }
        let key: u32 = ti.into();
        if let Some(v) = self.size_cache.get(&key) {
            return *v;
        }
        // Insert a placeholder so cycles terminate.
        self.size_cache.insert(key, None);
        let ti = self.resolve(ti);
        let out = match self.find(ti) {
            Some(pdb::TypeData::Primitive(p)) => primitive_size(&p, self.ptr_size),
            Some(pdb::TypeData::Class(c)) if !c.properties.forward_reference() => Some(c.size as u32),
            Some(pdb::TypeData::Union(u)) if !u.properties.forward_reference() => Some(u.size as u32),
            Some(pdb::TypeData::Enumeration(e)) => self.size_inner(e.underlying_type, depth + 1),
            Some(pdb::TypeData::Pointer(_)) => Some(self.ptr_size),
            Some(pdb::TypeData::Array(a)) => a.dimensions.last().copied(),
            Some(pdb::TypeData::Modifier(m)) => self.size_inner(m.underlying_type, depth + 1),
            Some(pdb::TypeData::Bitfield(b)) => self.size_inner(b.underlying_type, depth + 1),
            Some(pdb::TypeData::Procedure(_)) | Some(pdb::TypeData::MemberFunction(_)) => None,
            _ => None,
        };
        self.size_cache.insert(key, out);
        out
    }

    fn name_of(&mut self, ti: Ti) -> String {
        self.name_inner(ti, 0)
    }

    fn name_inner(&mut self, ti: Ti, depth: u32) -> String {
        let key: u32 = ti.into();
        if let Some(n) = self.name_cache.get(&key) {
            return n.clone();
        }
        if depth > 16 {
            return format!("<t{key}>");
        }
        let out = match self.find(ti) {
            Some(pdb::TypeData::Primitive(p)) => primitive_name(&p),
            Some(pdb::TypeData::Class(c)) => c.name.to_string().into_owned(),
            Some(pdb::TypeData::Union(u)) => u.name.to_string().into_owned(),
            Some(pdb::TypeData::Enumeration(e)) => e.name.to_string().into_owned(),
            Some(pdb::TypeData::Modifier(m)) => {
                let inner = self.name_inner(m.underlying_type, depth + 1);
                let mut s = String::new();
                if m.constant {
                    s.push_str("const ");
                }
                if m.volatile {
                    s.push_str("volatile ");
                }
                s.push_str(&inner);
                s
            }
            Some(pdb::TypeData::Pointer(p)) => {
                let inner = self.name_inner(p.underlying_type, depth + 1);
                let suffix = if p.attributes.is_reference() { "&" } else { "*" };
                format!("{inner}{suffix}")
            }
            Some(pdb::TypeData::Array(a)) => {
                let inner = self.name_inner(a.element_type, depth + 1);
                let elem = self.size_inner(a.element_type, depth + 1).unwrap_or(0);
                let total = a.dimensions.last().copied().unwrap_or(0);
                if elem > 0 && total % elem == 0 {
                    format!("{inner}[{}]", total / elem)
                } else {
                    format!("{inner}[/*{total} bytes*/]")
                }
            }
            Some(pdb::TypeData::Bitfield(b)) => {
                let inner = self.name_inner(b.underlying_type, depth + 1);
                format!("{inner}:{}", b.length)
            }
            Some(pdb::TypeData::Procedure(p)) => {
                let ret = self.name_inner(p.return_type.unwrap_or(pdb::TypeIndex(3)), depth + 1);
                let args = self.arg_list(p.argument_list, depth + 1);
                format!("{ret} ({args})")
            }
            Some(pdb::TypeData::MemberFunction(f)) => {
                let ret = self.name_inner(f.return_type, depth + 1);
                let cls = self.name_inner(f.class_type, depth + 1);
                let args = self.arg_list(f.argument_list, depth + 1);
                format!("{ret} {cls}::({args})")
            }
            _ => format!("<t{key}>"),
        };
        self.name_cache.insert(key, out.clone());
        out
    }

    fn arg_list(&mut self, ti: Ti, depth: u32) -> String {
        match self.find(ti) {
            Some(pdb::TypeData::ArgumentList(a)) => a
                .arguments
                .iter()
                .map(|t| self.name_inner(*t, depth))
                .collect::<Vec<_>>()
                .join(", "),
            _ => String::new(),
        }
    }

    /// Full signature for a member-function type index (used for methods and
    /// for procedure symbols whose type index is a MemberFunction/Procedure).
    fn signature(&mut self, ti: Ti, name: &str) -> Option<String> {
        match self.find(ti) {
            Some(pdb::TypeData::MemberFunction(f)) => {
                let ret = self.name_of(f.return_type);
                let args = self.arg_list(f.argument_list, 1);
                let cnst = matches!(self.find(f.this_pointer_type.unwrap_or(pdb::TypeIndex(0))),
                    Some(pdb::TypeData::Pointer(p))
                        if matches!(self.find(p.underlying_type), Some(pdb::TypeData::Modifier(m)) if m.constant));
                Some(format!(
                    "{ret} {name}({args}){}",
                    if cnst { " const" } else { "" }
                ))
            }
            Some(pdb::TypeData::Procedure(p)) => {
                let ret = self.name_of(p.return_type.unwrap_or(pdb::TypeIndex(3)));
                let args = self.arg_list(p.argument_list, 1);
                Some(format!("{ret} {name}({args})"))
            }
            _ => None,
        }
    }
}

fn primitive_size(p: &pdb::PrimitiveType, ptr: u32) -> Option<u32> {
    use pdb::PrimitiveKind::*;
    if p.indirection.is_some() {
        return Some(ptr);
    }
    Some(match p.kind {
        NoType | Void => 0,
        Char | UChar | RChar | I8 | U8 | Bool8 => 1,
        WChar | RChar16 | Short | UShort | I16 | U16 | Bool16 | F16 => 2,
        RChar32 | Long | ULong | I32 | U32 | Bool32 | F32 | F32PP | HRESULT => 4,
        F48 => 6,
        Quad | UQuad | I64 | U64 | Bool64 | F64 | Complex32 => 8,
        F80 => 10,
        Octa | UOcta | I128 | U128 | F128 | Complex64 => 16,
        Complex80 => 20,
        Complex128 => 32,
        _ => return None,
    })
}

fn primitive_name(p: &pdb::PrimitiveType) -> String {
    use pdb::PrimitiveKind::*;
    let base = match p.kind {
        NoType => "<notype>",
        Void => "void",
        Char => "char",
        UChar => "unsigned char",
        RChar => "char",
        WChar => "wchar_t",
        RChar16 => "char16_t",
        RChar32 => "char32_t",
        I8 => "int8_t",
        U8 => "uint8_t",
        Short => "short",
        UShort => "unsigned short",
        I16 => "int16_t",
        U16 => "uint16_t",
        Long => "long",
        ULong => "unsigned long",
        I32 => "int",
        U32 => "unsigned int",
        Quad => "long long",
        UQuad => "unsigned long long",
        I64 => "int64_t",
        U64 => "uint64_t",
        Octa => "__int128",
        UOcta => "unsigned __int128",
        I128 => "int128_t",
        U128 => "uint128_t",
        F16 => "half",
        F32 => "float",
        F32PP => "float",
        F48 => "float48",
        F64 => "double",
        F80 => "long double",
        F128 => "float128",
        Complex32 => "_Complex float",
        Complex64 => "_Complex double",
        Complex80 => "_Complex long double",
        Complex128 => "_Complex float128",
        Bool8 => "bool",
        Bool16 => "bool16",
        Bool32 => "bool32",
        Bool64 => "bool64",
        HRESULT => "HRESULT",
        _ => "<primitive>",
    };
    match p.indirection {
        Some(_) => format!("{base}*"),
        None => base.to_string(),
    }
}

// ---------------------------------------------------------------------------

/// MSVC mangles `A::B::c` as `?c@B@A@@...`. Build that prefix so a procedure can
/// be matched to *its own* public symbol at an address several symbols share.
/// Returns a prefix that matches nothing for names MSVC does not mangle this way
/// (templates, operators, C linkage), which makes the caller fall back.
fn msvc_prefix(name: &str) -> String {
    if name.contains('<') || name.contains('`') || name.contains(' ') {
        return "\u{0}".to_string();
    }
    let mut parts: Vec<&str> = name.split("::").collect();
    parts.reverse();
    format!("?{}@@", parts.join("@"))
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

fn demangle(m: &str) -> Option<String> {
    let flags = msvc_demangler::DemangleFlags::llvm();
    msvc_demangler::demangle(m, flags).ok()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 5 {
        eprintln!("usage: pdb-extract <pdb> <image_base_hex> <symbols.json> <types.json>");
        std::process::exit(2);
    }
    let pdb_path = &args[1];
    let image_base = u64::from_str_radix(args[2].trim_start_matches("0x"), 16)?;
    let sym_out = &args[3];
    let ty_out = &args[4];

    let file = File::open(pdb_path)?;
    let mut pdb = pdb::PDB::open(file)?;

    let pdb_info = pdb.pdb_information()?;
    let guid = pdb_info.guid;
    let age = pdb_info.age;
    let signature = pdb_info.signature;

    let address_map = pdb.address_map()?;
    let string_table = pdb.string_table().ok();

    // Executable RVA ranges, from the PE section headers embedded in the PDB.
    // The S_PUB32 "function" bit is NOT reliable on this PDB (`__alloca_probe_8`
    // is code but not flagged), so code/data is decided by section instead.
    const IMAGE_SCN_MEM_EXECUTE: u32 = 0x2000_0000;
    let exec_ranges: Vec<(u32, u32)> = pdb
        .sections()?
        .unwrap_or_default()
        .iter()
        .filter(|s| s.characteristics.0 & IMAGE_SCN_MEM_EXECUTE != 0)
        .map(|s| (s.virtual_address, s.virtual_address + s.virtual_size))
        .collect();
    let is_exec = |rva: u32| exec_ranges.iter().any(|(a, b)| rva >= *a && rva < *b);

    // ---- public symbols: rva -> decorated name ---------------------------
    let mut publics: HashMap<u32, Vec<String>> = HashMap::new();
    let mut public_is_func: HashMap<u32, bool> = HashMap::new();
    let mut public_count = 0usize;
    let mut public_func_count = 0usize;
    // (rva, name, type_index, is_global) collected from S_GDATA32 in the globals stream.
    let mut raw_globals: Vec<(u32, String, Ti, bool)> = Vec::new();
    {
        let gs = pdb.global_symbols()?;
        let mut it = gs.iter();
        while let Some(sym) = it.next()? {
            match sym.parse() {
                Ok(pdb::SymbolData::Public(p)) => {
                    public_count += 1;
                    if let Some(rva) = p.offset.to_rva(&address_map) {
                        if p.function {
                            public_func_count += 1;
                        }
                        *public_is_func.entry(rva.0).or_insert(false) |= p.function;
                        publics
                            .entry(rva.0)
                            .or_default()
                            .push(p.name.to_string().into_owned());
                    }
                }
                Ok(pdb::SymbolData::Data(d)) => {
                    if let Some(rva) = d.offset.to_rva(&address_map) {
                        raw_globals.push((
                            rva.0,
                            d.name.to_string().into_owned(),
                            d.type_index,
                            d.global,
                        ));
                    }
                }
                _ => {}
            }
        }
    }

    // ---- type information -------------------------------------------------
    let type_info = pdb.type_information()?;
    let mut finder = type_info.finder();
    let mut defs: HashMap<String, Ti> = HashMap::new();
    let mut udefs: HashMap<String, Ti> = HashMap::new();
    let mut all_type_records = 0usize;
    {
        let mut it = type_info.iter();
        while let Some(t) = it.next()? {
            finder.update(&it);
            all_type_records += 1;
            let idx = t.index();
            match t.parse() {
                Ok(pdb::TypeData::Class(c)) if !c.properties.forward_reference() => {
                    defs.entry(c.name.to_string().into_owned()).or_insert(idx);
                    if let Some(u) = c.unique_name {
                        udefs.entry(u.to_string().into_owned()).or_insert(idx);
                    }
                }
                Ok(pdb::TypeData::Union(u)) if !u.properties.forward_reference() => {
                    defs.entry(u.name.to_string().into_owned()).or_insert(idx);
                    if let Some(n) = u.unique_name {
                        udefs.entry(n.to_string().into_owned()).or_insert(idx);
                    }
                }
                Ok(pdb::TypeData::Enumeration(e)) if !e.properties.forward_reference() => {
                    defs.entry(e.name.to_string().into_owned()).or_insert(idx);
                    if let Some(n) = e.unique_name {
                        udefs.entry(n.to_string().into_owned()).or_insert(idx);
                    }
                }
                _ => {}
            }
        }
    }
    let mut ctx = TypeCtx {
        finder,
        defs,
        udefs,
        size_cache: HashMap::new(),
        name_cache: HashMap::new(),
        ptr_size: 4,
    };

    // ---- modules: procedures + line info ---------------------------------
    let dbi = pdb.debug_information()?;
    let mut funcs: Vec<FunctionRec> = Vec::new();
    let mut seen_rva: HashSet<u32> = HashSet::new();
    let mut seen_proc: HashSet<(u32, String)> = HashSet::new();
    let mut source_files: HashSet<String> = HashSet::new();
    let mut module_names: Vec<String> = Vec::new();
    let mut per_module: BTreeMap<String, usize> = BTreeMap::new();
    let mut with_lines = 0usize;
    let mut module_statics: Vec<(u32, String, Ti, bool, String)> = Vec::new();

    let mut modules = dbi.modules()?;
    while let Some(module) = modules.next()? {
        let raw_mod = module.module_name().into_owned();
        module_names.push(raw_mod.clone());
        let info = match pdb.module_info(&module)? {
            Some(i) => i,
            None => continue,
        };
        let line_program = info.line_program().ok();
        let mut syms = info.symbols()?;
        while let Some(sym) = syms.next()? {
            if let Ok(pdb::SymbolData::Data(d)) = sym.parse() {
                if let Some(rva) = d.offset.to_rva(&address_map) {
                    module_statics.push((
                        rva.0,
                        d.name.to_string().into_owned(),
                        d.type_index,
                        d.global,
                        raw_mod.clone(),
                    ));
                }
                continue;
            }
            let (name, offset, len, kind, tyidx) = match sym.parse() {
                Ok(pdb::SymbolData::Procedure(p)) => (
                    p.name.to_string().into_owned(),
                    p.offset,
                    Some(p.len),
                    if p.global { "global" } else { "local" },
                    Some(p.type_index),
                ),
                Ok(pdb::SymbolData::Thunk(t)) => (
                    t.name.to_string().into_owned(),
                    t.offset,
                    Some(t.len as u32),
                    "thunk",
                    None,
                ),
                _ => continue,
            };
            let rva = match offset.to_rva(&address_map) {
                Some(r) => r.0,
                None => continue,
            };
            // Do NOT collapse by address. MSVC identical-COMDAT-folding gives one
            // address several procedure records with different names; keeping only
            // the first is how a 3-byte `CheckSum::walk_test` gets reported as
            // somebody's `OnPaint`. Emit every record; consumers that want a
            // single name per address must decide which, knowingly.
            seen_rva.insert(rva);
            if !seen_proc.insert((rva, name.clone())) {
                continue;
            }
            *per_module.entry(raw_mod.clone()).or_insert(0) += 1;

            // line info
            let mut file: Option<String> = None;
            let mut line: Option<u32> = None;
            let mut line_end: Option<u32> = None;
            if let (Some(lp), Some(st)) = (line_program.as_ref(), string_table.as_ref()) {
                // `lines_for_symbol` can return records outside the procedure's
                // extent (MASM). Keep only records inside [offset, offset+len).
                let end = offset.offset + len.unwrap_or(0);
                let mut lines = lp.lines_for_symbol(offset);
                let mut min = u32::MAX;
                let mut max = 0u32;
                let mut fidx = None;
                let mut first_off = u32::MAX;
                while let Some(li) = lines.next()? {
                    if li.offset.section != offset.section
                        || li.offset.offset < offset.offset
                        || (len.is_some() && li.offset.offset >= end)
                    {
                        continue;
                    }
                    if li.offset.offset < first_off {
                        first_off = li.offset.offset;
                        fidx = Some(li.file_index);
                    }
                    if li.line_start < min {
                        min = li.line_start;
                    }
                    if li.line_end > max {
                        max = li.line_end;
                    }
                }
                if fidx.is_none() {
                    // Fall back to any record the line program associates with
                    // this symbol (MASM / tail-merged code lands here).
                    let mut lines = lp.lines_for_symbol(offset);
                    if let Some(li) = lines.next()? {
                        fidx = Some(li.file_index);
                        min = li.line_start;
                        max = li.line_end.max(li.line_start);
                    }
                }
                if let Some(fi) = fidx {
                    if let Ok(finfo) = lp.get_file_info(fi) {
                        if let Ok(n) = finfo.name.to_string_lossy(st) {
                            let s = n.into_owned();
                            source_files.insert(s.clone());
                            file = Some(s);
                        }
                    }
                    if min != u32::MAX {
                        line = Some(min);
                        line_end = Some(max.max(min));
                        with_lines += 1;
                    }
                }
            }

            // Pick the public symbol that belongs to *this* procedure, not just
            // any public at this address: after ICF one address carries several.
            // MSVC mangles `A::b` as `?b@A@@...`, so build that prefix and match.
            let want = msvc_prefix(&name);
            let mut ambiguous = false;
            let mangled = publics.get(&rva).and_then(|v| {
                if let Some(exact) = v.iter().find(|s| s.starts_with(&want)) {
                    return Some(exact.clone());
                }
                if v.iter().filter(|s| s.starts_with('?')).count() > 1 {
                    ambiguous = true;
                }
                v.iter()
                    .find(|s| s.starts_with('?'))
                    .or_else(|| v.first())
                    .cloned()
            });
            let demangled = if ambiguous {
                None
            } else {
                mangled.as_deref().and_then(demangle)
            };
            let mangled = if ambiguous { None } else { mangled };
            let sig = tyidx.and_then(|t| ctx.signature(t, &name));

            funcs.push(FunctionRec {
                va: format!("0x{:08x}", image_base as u32 as u64 + rva as u64),
                rva,
                name,
                mangled,
                demangled,
                size: len,
                kind,
                module: Some(raw_mod.clone()),
                file,
                line,
                line_end,
                type_index: tyidx.map(|t| t.into()),
                signature: sig,
            });
        }
    }
    let proc_count = funcs.len();

    // ---- publics with no procedure record (thunks, imports, asm) ---------
    let mut public_only = 0usize;
    let mut public_data = 0usize;
    let mut pubs: Vec<(u32, String, bool)> = publics
        .iter()
        .filter(|(rva, _)| !seen_rva.contains(rva))
        .map(|(rva, v)| {
            let n = v
                .iter()
                .find(|s| s.starts_with('?'))
                .or_else(|| v.first())
                .cloned()
                .unwrap_or_default();
            let code = is_exec(*rva) || public_is_func.get(rva).copied().unwrap_or(false);
            (*rva, n, code)
        })
        .collect();
    pubs.sort();

    // ---- global / static data --------------------------------------------
    let mut globals: Vec<GlobalRec> = Vec::new();
    let mut seen_global: HashSet<(u32, String)> = HashSet::new();
    for (rva, name, ti, is_global, module) in module_statics
        .iter()
        .map(|(a, b, c, d, e)| (*a, b.clone(), *c, *d, Some(e.clone())))
        .chain(
            raw_globals
                .iter()
                .map(|(a, b, c, d)| (*a, b.clone(), *c, *d, None)),
        )
    {
        // Unnamed S_LDATA32 records are compiler-emitted jump tables; skip them.
        if name.is_empty() || !seen_global.insert((rva, name.clone())) {
            continue;
        }
        let ty = ctx.name_of(ti);
        let size = ctx.size_of(ti);
        let mangled = publics.get(&rva).and_then(|v| {
            v.iter()
                .find(|s| s.starts_with('?') && s.contains(&name))
                .or_else(|| v.iter().find(|s| s.starts_with('?')))
                .cloned()
        });
        let demangled = mangled.as_deref().and_then(demangle);
        globals.push(GlobalRec {
            va: format!("0x{:08x}", image_base as u32 as u64 + rva as u64),
            rva,
            name,
            mangled,
            demangled,
            ty: Some(ty),
            type_index: Some(ti.into()),
            size,
            kind: if is_global { "global" } else { "static" },
            module,
        });
    }
    for (rva, mangled, is_func) in &pubs {
        if *is_func {
            continue;
        }
        let demangled = demangle(mangled);
        let name = demangled.clone().unwrap_or_else(|| mangled.clone());
        if !seen_global.insert((*rva, name.clone())) {
            continue;
        }
        public_data += 1;
        globals.push(GlobalRec {
            va: format!("0x{:08x}", image_base as u32 as u64 + *rva as u64),
            rva: *rva,
            name,
            mangled: Some(mangled.clone()),
            demangled,
            ty: None,
            type_index: None,
            size: None,
            kind: "public",
            module: None,
        });
    }
    globals.sort_by(|a, b| a.rva.cmp(&b.rva).then(a.name.cmp(&b.name)));

    for (rva, mangled, is_func) in pubs {
        if !is_func {
            continue;
        }
        let demangled = demangle(&mangled);
        public_only += 1;
        funcs.push(FunctionRec {
            va: format!("0x{:08x}", image_base as u32 as u64 + rva as u64),
            rva,
            name: demangled.clone().unwrap_or_else(|| mangled.clone()),
            mangled: Some(mangled),
            demangled,
            size: None,
            kind: "public",
            module: None,
            file: None,
            line: None,
            line_end: None,
            type_index: None,
            signature: None,
        });
    }
    funcs.sort_by_key(|f| f.rva);

    let mut sf: Vec<String> = source_files.iter().cloned().collect();
    sf.sort();

    let mut name_counts: HashMap<u32, usize> = HashMap::new();
    for f in &funcs {
        if f.kind != "public" {
            *name_counts.entry(f.rva).or_insert(0) += 1;
        }
    }
    let multi_name = name_counts.values().filter(|v| **v > 1).count();

    let sym_json = serde_json::json!({
        "_meta": {
            "pdb": pdb_path,
            "guid": format!("{{{guid}}}"),
            "age": age,
            "signature": signature,
            "image_base": format!("0x{:08x}", image_base),
            "generator": "tools/pdb-extract (Rust `pdb` 0.8 + msvc-demangler)",
            "counts": {
                "functions_total": funcs.len(),
                "procedures": proc_count,
                "distinct_function_addresses": seen_rva.len(),
                "addresses_with_multiple_names": multi_name,
                "public_only": public_only,
                "public_symbols": public_count,
                "public_functions": public_func_count,
                "with_source_line": with_lines,
                "source_files": sf.len(),
                "modules": module_names.len(),
                "type_records": all_type_records,
                "globals": globals.len(),
                "globals_public_only": public_data,
            },
            "note": "functions[].kind: global/local = S_GPROC32/S_LPROC32 with real code extents; public = an S_PUB32 function address with no procedure record (import thunk, hand-written asm) and therefore no size. globals[].kind: global/static = S_GDATA32/S_LDATA32 with a type index; public = S_PUB32 data with no typed record.",
        },
        "source_files": sf,
        "modules": module_names,
        "functions": funcs,
        "globals": globals,
    });
    serde_json::to_writer(std::io::BufWriter::new(File::create(sym_out)?), &sym_json)?;

    // ---- types.json -------------------------------------------------------
    let mut classes: BTreeMap<String, TypeRec> = BTreeMap::new();
    let mut enums: BTreeMap<String, EnumRec> = BTreeMap::new();

    let def_list: Vec<(String, Ti)> = ctx.defs.iter().map(|(k, v)| (k.clone(), *v)).collect();
    for (name, ti) in def_list {
        match ctx.find(ti) {
            Some(pdb::TypeData::Class(c)) => {
                let kind = match c.kind {
                    pdb::ClassKind::Class => "class",
                    pdb::ClassKind::Struct => "struct",
                    pdb::ClassKind::Interface => "interface",
                };
                let mut rec = TypeRec {
                    kind,
                    size: Some(c.size as u32),
                    unique_name: c.unique_name.map(|u| u.to_string().into_owned()),
                    bases: vec![],
                    fields: vec![],
                    statics: vec![],
                    methods: vec![],
                    methods_declared: 0,
                    nested: vec![],
                    has_vftable: c.vtable_shape.is_some(),
                };
                if let Some(f) = c.fields {
                    walk_fields(&mut ctx, f, &mut rec, 0);
                }
                classes.insert(name, rec);
            }
            Some(pdb::TypeData::Union(u)) => {
                let mut rec = TypeRec {
                    kind: "union",
                    size: Some(u.size as u32),
                    unique_name: u.unique_name.map(|n| n.to_string().into_owned()),
                    bases: vec![],
                    fields: vec![],
                    statics: vec![],
                    methods: vec![],
                    methods_declared: 0,
                    nested: vec![],
                    has_vftable: false,
                };
                walk_fields(&mut ctx, u.fields, &mut rec, 0);
                classes.insert(name, rec);
            }
            Some(pdb::TypeData::Enumeration(e)) => {
                let underlying = ctx.name_of(e.underlying_type);
                let size = ctx.size_of(e.underlying_type);
                let mut values = Vec::new();
                collect_enum(&ctx, e.fields, &mut values, 0);
                enums.insert(name, EnumRec { underlying, size, values });
            }
            _ => {}
        }
    }

    let ty_json = serde_json::json!({
        "_meta": {
            "pdb": pdb_path,
            "guid": format!("{{{guid}}}"),
            "generator": "tools/pdb-extract (Rust `pdb` 0.8)",
            "pointer_size": 4,
            "counts": {
                "type_records": all_type_records,
                "classes": classes.len(),
                "enums": enums.len(),
            },
            "note": "offsets and sizes are bytes; `size` on a field is the size of that field's type, resolved through typedef/modifier/array/enum. Forward references are resolved to their definitions. `methods` lists VIRTUAL methods only (with their introducing vtable slot, which exists nowhere else); `methods_declared` is the full declared count. Non-virtual methods are omitted because every emitted one is already in symbols.json with an address and a full signature.",
        },
        "classes": classes,
        "enums": enums,
    });
    serde_json::to_writer(std::io::BufWriter::new(File::create(ty_out)?), &ty_json)?;

    eprintln!(
        "functions={} (proc={} public_only={}) with_line={} files={} modules={} classes={} enums={} type_records={}",
        funcs.len(),
        proc_count,
        public_only,
        with_lines,
        sf.len(),
        module_names.len(),
        classes.len(),
        enums.len(),
        all_type_records
    );
    Ok(())
}

fn walk_fields(ctx: &mut TypeCtx, fields: Ti, rec: &mut TypeRec, depth: u32) {
    if depth > 64 {
        return;
    }
    let fl = match ctx.find(fields) {
        Some(pdb::TypeData::FieldList(f)) => f,
        _ => return,
    };
    for f in &fl.fields {
        match f {
            pdb::TypeData::Member(m) => {
                let ty = ctx.name_of(m.field_type);
                let size = ctx.size_of(m.field_type);
                let (bit_offset, bit_length) = match ctx.find(m.field_type) {
                    Some(pdb::TypeData::Bitfield(b)) => (Some(b.position), Some(b.length)),
                    _ => (None, None),
                };
                rec.fields.push(FieldRec {
                    name: m.name.to_string().into_owned(),
                    ty,
                    type_index: m.field_type.into(),
                    offset: m.offset as u32,
                    size,
                    bit_offset,
                    bit_length,
                });
            }
            pdb::TypeData::StaticMember(s) => {
                let ty = ctx.name_of(s.field_type);
                rec.statics.push(StaticRec {
                    name: s.name.to_string().into_owned(),
                    ty,
                });
            }
            pdb::TypeData::BaseClass(b) => {
                let name = ctx.name_of(b.base_class);
                let size = ctx.size_of(b.base_class);
                rec.bases.push(BaseRec {
                    name,
                    offset: b.offset,
                    size,
                    virtual_base: false,
                });
            }
            pdb::TypeData::VirtualBaseClass(b) => {
                let name = ctx.name_of(b.base_class);
                let size = ctx.size_of(b.base_class);
                rec.bases.push(BaseRec {
                    name,
                    offset: b.base_pointer_offset,
                    size,
                    virtual_base: true,
                });
            }
            pdb::TypeData::VirtualFunctionTablePointer(_) => {
                rec.has_vftable = true;
            }
            pdb::TypeData::Method(m) => {
                rec.methods_declared += 1;
                if m.attributes.is_virtual() || m.attributes.is_intro_virtual() {
                    let name = m.name.to_string().into_owned();
                    let sig = ctx.signature(m.method_type, &name);
                    rec.methods.push(MethodRec {
                        name,
                        signature: sig,
                        vtable_offset: m.vtable_offset,
                        is_virtual: true,
                        is_static: m.attributes.is_static(),
                    });
                }
            }
            pdb::TypeData::OverloadedMethod(o) => {
                let name = o.name.to_string().into_owned();
                if let Some(pdb::TypeData::MethodList(ml)) = ctx.find(o.method_list) {
                    for m in &ml.methods {
                        rec.methods_declared += 1;
                        if !(m.attributes.is_virtual() || m.attributes.is_intro_virtual()) {
                            continue;
                        }
                        let sig = ctx.signature(m.method_type, &name);
                        rec.methods.push(MethodRec {
                            name: name.clone(),
                            signature: sig,
                            vtable_offset: m.vtable_offset,
                            is_virtual: true,
                            is_static: m.attributes.is_static(),
                        });
                    }
                }
            }
            pdb::TypeData::Nested(n) => {
                rec.nested.push(n.name.to_string().into_owned());
            }
            _ => {}
        }
    }
    if let Some(cont) = fl.continuation {
        walk_fields(ctx, cont, rec, depth + 1);
    }
}

fn collect_enum(ctx: &TypeCtx, fields: Ti, out: &mut Vec<(String, i64)>, depth: u32) {
    if depth > 64 {
        return;
    }
    let fl = match ctx.find(fields) {
        Some(pdb::TypeData::FieldList(f)) => f,
        _ => return,
    };
    for f in &fl.fields {
        if let pdb::TypeData::Enumerate(e) = f {
            let v = match e.value {
                pdb::Variant::U8(v) => v as i64,
                pdb::Variant::U16(v) => v as i64,
                pdb::Variant::U32(v) => v as i64,
                pdb::Variant::U64(v) => v as i64,
                pdb::Variant::I8(v) => v as i64,
                pdb::Variant::I16(v) => v as i64,
                pdb::Variant::I32(v) => v as i64,
                pdb::Variant::I64(v) => v,
            };
            out.push((e.name.to_string().into_owned(), v));
        }
    }
    if let Some(cont) = fl.continuation {
        collect_enum(ctx, cont, out, depth + 1);
    }
}
