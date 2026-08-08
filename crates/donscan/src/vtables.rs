//! The vtable map: `schema/vtables.json` (1,777 RTTI vtable VA -> class name)
//! compiled into a direct-index lookup table over the rebased address range.
//!
//! The table is `span/4` u16 slots, ~514 KB for the observed 0xac6d54..0xbc21d8
//! span, so it lives in L2 and a hit costs one load. The prefilter in front of it
//! (`v.wrapping_sub(lo) <= span`) rejects essentially every dword in the heap with
//! one subtract and one compare, which is what makes a whole-heap scan cheap.

/// The map is embedded so the deliverable is one file to copy into the guest.
const VTABLES_JSON: &str = include_str!("../../../schema/vtables.json");

pub const NONE: u16 = u16::MAX;

#[derive(Clone)]
pub struct VtEntry {
    pub static_va: u32,
    pub runtime_va: u64,
    pub name_idx: u32,
}

pub struct VtMap {
    pub names: Vec<String>,
    pub entries: Vec<VtEntry>,
    pub lo: u64,
    pub span: u64,
    table: Vec<u16>,
}

impl VtMap {
    /// `delta` = runtime image base - preferred image base (wrapping).
    pub fn build(delta: u64) -> Result<VtMap, String> {
        let pairs = parse_flat_string_map(VTABLES_JSON)?;
        if pairs.is_empty() {
            return Err("vtables.json parsed to zero entries".into());
        }

        let mut names: Vec<String> = Vec::new();
        let mut name_idx_of: std::collections::HashMap<&str, u32> = std::collections::HashMap::new();
        let mut entries: Vec<VtEntry> = Vec::with_capacity(pairs.len());

        for (k, v) in &pairs {
            let s = k.trim();
            let s = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).unwrap_or(s);
            let va = u32::from_str_radix(s, 16)
                .map_err(|_| format!("vtables.json: bad address key {k:?}"))?;
            let idx = match name_idx_of.get(v.as_str()) {
                Some(&i) => i,
                None => {
                    let i = names.len() as u32;
                    names.push(v.clone());
                    name_idx_of.insert(v.as_str(), i);
                    i
                }
            };
            entries.push(VtEntry {
                static_va: va,
                runtime_va: (va as u64).wrapping_add(delta),
                name_idx: idx,
            });
        }
        // `name_idx_of` borrows `pairs`; drop it before `pairs` goes out of scope.
        drop(name_idx_of);

        if entries.len() > NONE as usize {
            return Err(format!(
                "{} vtables exceeds the u16 slot encoding ({} max)",
                entries.len(),
                NONE
            ));
        }

        entries.sort_by_key(|e| e.runtime_va);
        let lo = entries.first().unwrap().runtime_va & !3;
        let hi = entries.last().unwrap().runtime_va;
        let span = hi - lo;
        let mut table = vec![NONE; (span / 4) as usize + 1];
        for (i, e) in entries.iter().enumerate() {
            if e.runtime_va % 4 != 0 {
                return Err(format!("vtable {:#x} is not 4-aligned", e.static_va));
            }
            let slot = ((e.runtime_va - lo) / 4) as usize;
            table[slot] = i as u16;
        }
        Ok(VtMap { names, entries, lo, span, table })
    }

    /// One subtract, one compare, one load. `v` is a candidate dword read out of
    /// the target's memory.
    #[inline(always)]
    pub fn lookup(&self, v: u32) -> u16 {
        let d = (v as u64).wrapping_sub(self.lo);
        if d > self.span {
            return NONE;
        }
        // Non-4-aligned candidates can never be a vtable we know about, and the
        // shift below would alias them onto a real slot.
        if d & 3 != 0 {
            return NONE;
        }
        self.table[(d >> 2) as usize]
    }
}

/// Minimal reader for the exact shape `schema/vtables.json` has: a flat object of
/// string -> string. Verified [measured] to contain no escapes and no non-ASCII,
/// but `\"` `\\` `\/` `\n` `\r` `\t` `\uXXXX` are handled anyway so a future
/// regeneration cannot silently corrupt a class name.
fn parse_flat_string_map(src: &str) -> Result<Vec<(String, String)>, String> {
    let b = src.as_bytes();
    let mut i = 0usize;
    let mut out = Vec::new();

    fn skip_ws(b: &[u8], i: &mut usize) {
        while *i < b.len() && matches!(b[*i], b' ' | b'\t' | b'\n' | b'\r') {
            *i += 1;
        }
    }
    fn parse_string(b: &[u8], i: &mut usize) -> Result<String, String> {
        if *i >= b.len() || b[*i] != b'"' {
            return Err(format!("expected '\"' at byte {}", *i));
        }
        *i += 1;
        let mut s = String::new();
        while *i < b.len() {
            match b[*i] {
                b'"' => {
                    *i += 1;
                    return Ok(s);
                }
                b'\\' => {
                    *i += 1;
                    if *i >= b.len() {
                        return Err("truncated escape".into());
                    }
                    let c = b[*i];
                    *i += 1;
                    match c {
                        b'"' => s.push('"'),
                        b'\\' => s.push('\\'),
                        b'/' => s.push('/'),
                        b'b' => s.push('\u{8}'),
                        b'f' => s.push('\u{c}'),
                        b'n' => s.push('\n'),
                        b'r' => s.push('\r'),
                        b't' => s.push('\t'),
                        b'u' => {
                            if *i + 4 > b.len() {
                                return Err("truncated \\u escape".into());
                            }
                            let hex = std::str::from_utf8(&b[*i..*i + 4])
                                .map_err(|_| "bad \\u escape".to_string())?;
                            let cp = u32::from_str_radix(hex, 16)
                                .map_err(|_| "bad \\u escape".to_string())?;
                            *i += 4;
                            s.push(char::from_u32(cp).unwrap_or('\u{fffd}'));
                        }
                        other => return Err(format!("unknown escape \\{}", other as char)),
                    }
                }
                _ => {
                    // Copy the whole UTF-8 sequence.
                    let start = *i;
                    let len = utf8_len(b[*i]);
                    *i += len;
                    if *i > b.len() {
                        return Err("truncated utf-8".into());
                    }
                    s.push_str(
                        std::str::from_utf8(&b[start..*i]).map_err(|_| "bad utf-8".to_string())?,
                    );
                }
            }
        }
        Err("unterminated string".into())
    }
    fn utf8_len(c: u8) -> usize {
        if c < 0x80 {
            1
        } else if c >> 5 == 0b110 {
            2
        } else if c >> 4 == 0b1110 {
            3
        } else {
            4
        }
    }

    skip_ws(b, &mut i);
    if i >= b.len() || b[i] != b'{' {
        return Err("expected '{' at start of vtables.json".into());
    }
    i += 1;
    loop {
        skip_ws(b, &mut i);
        if i < b.len() && b[i] == b'}' {
            break;
        }
        let k = parse_string(b, &mut i)?;
        skip_ws(b, &mut i);
        if i >= b.len() || b[i] != b':' {
            return Err(format!("expected ':' at byte {i}"));
        }
        i += 1;
        skip_ws(b, &mut i);
        let v = parse_string(b, &mut i)?;
        out.push((k, v));
        skip_ws(b, &mut i);
        if i < b.len() && b[i] == b',' {
            i += 1;
            continue;
        }
        if i < b.len() && b[i] == b'}' {
            break;
        }
        return Err(format!("expected ',' or '}}' at byte {i}"));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_map_parses_and_has_the_known_anchors() {
        let m = VtMap::build(0).expect("build");
        assert_eq!(m.entries.len(), 1777, "vtables.json entry count changed");
        let find = |va: u32| -> Option<&str> {
            m.entries
                .iter()
                .find(|e| e.static_va == va)
                .map(|e| m.names[e.name_idx as usize].as_str())
        };
        assert_eq!(find(0xb417d0), Some("Unit"));
        assert_eq!(find(0xb434ac), Some("Object"));
    }

    #[test]
    fn lookup_hits_only_on_exact_rebased_addresses() {
        let delta = 0x0123_0000u64;
        let m = VtMap::build(delta).expect("build");
        let unit_rt = 0xb417d0u64 + delta;
        let idx = m.lookup(unit_rt as u32);
        assert_ne!(idx, NONE);
        assert_eq!(m.names[m.entries[idx as usize].name_idx as usize], "Unit");
        assert_eq!(m.lookup((unit_rt + 4) as u32), NONE);
        assert_eq!(m.lookup((unit_rt + 1) as u32), NONE);
        assert_eq!(m.lookup(0), NONE);
    }
}
