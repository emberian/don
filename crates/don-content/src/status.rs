//! `mod-status.txt` — the file retail uses to remember which mods are on and in what order.
//!
//! `ModManager::readModStatus` `0x00A244E0` opens `L"mod-status.txt"` at storage point
//! `MYMODS` (7) with `FileOpen` flags `0x1004` (`Read | ShareRead`);
//! `ModManager::writeModStatus` `0x00A240F0` writes it with flags `10`
//! (`CreateAsNeeded | OverwriteExisting`). Both are guarded by
//! `ModManager::disableModStatus` — reading sets a latch, and the writer refuses to run if
//! the reader never did, so a failed load cannot truncate the user's mod order.
//!
//! # Format [measured]
//!
//! One fixed-width header line then one line per mod. The two `printf` format strings are
//! `.rdata` literals, and the argument order was taken from the push order in
//! `writeModStatus`, not guessed:
//!
//! ```text
//! header  0x00B149D0  "%-10s%-50s%-10s%-10s%-10s%-12s%-12s%-24s%-24s"
//! row     0x00B14950  "%-10d%-49s %-10d%-10s%-10s%-12d%-12d%-24llu%-24llu"
//! ```
//!
//! | # | header | row | meaning |
//! |--:|---|---|---|
//! | 1 | `ID` | `%-10d` | list index |
//! | 2 | `MOD NAME` | `%-49s ` | display name, **quoted** (`"%s"` `0x00B149B8`) so spaces survive |
//! | 3 | `PRIORITY` | `%-10d` | lower wins |
//! | 4 | `ENABLED` | `%-10s` | `Yes` / `No` |
//! | 5 | `LOCAL` | `%-10s` | `Yes` = `StoragePoint::MYMODS`, `No` = Workshop |
//! | 6 | `TIMESTAMP` | `%-12d` | stored at `ModPackage + 0x164` |
//! | 7 | `TIMESTAMP2` | `%-12d` | stored at `ModPackage + 0x160` |
//! | 8 | `AUTHOR` | `%-24llu` | SteamID64; validated, not restored to the package |
//! | 9 | `WORKSHOPID` | `%-24llu` | `PublishedFileId`; validated, not restored |
//!
//! The reader validates every field and **skips the whole line** on any failure — a malformed
//! line is not fatal and is not partially applied. It then matches rows to already-discovered
//! packages by `(name, location)`; a row naming a mod that is not installed is dropped, and an
//! installed mod with no row keeps the defaults `ModPackage::buildPackage` gave it
//! (`enabled = true`, `priority` = scan order). Finally it calls `sortViaPriority`.
//!
//! So `mod-status.txt` is **advisory state, not a manifest**: it can never introduce a mod.

use crate::vfs::{ContentStack, ModPackage, StorageLocation};

/// One parsed row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusRow {
    pub id: i32,
    pub name: String,
    pub priority: i32,
    pub enabled: bool,
    pub local: bool,
    pub timestamp: i32,
    pub timestamp2: i32,
    pub author: u64,
    pub workshop_id: u64,
}

pub const HEADER_COLUMNS: [&str; 9] = [
    "ID",
    "MOD NAME",
    "PRIORITY",
    "ENABLED",
    "LOCAL",
    "TIMESTAMP",
    "TIMESTAMP2",
    "AUTHOR",
    "WORKSHOPID",
];

const HEADER_WIDTHS: [usize; 9] = [10, 50, 10, 10, 10, 12, 12, 24, 24];

/// Render the header line exactly as `writeModStatus` does.
pub fn header_line() -> String {
    let mut s = String::new();
    for (col, w) in HEADER_COLUMNS.iter().zip(HEADER_WIDTHS) {
        s.push_str(&format!("{col:<w$}", w = w));
    }
    s
}

impl StatusRow {
    /// Render one row with retail's widths. The name is quoted and padded to 49 plus the
    /// literal space the format string carries, which is how a name with spaces round-trips.
    pub fn to_line(&self) -> String {
        let quoted = format!("\"{}\"", self.name);
        format!(
            "{:<10}{:<49} {:<10}{:<10}{:<10}{:<12}{:<12}{:<24}{:<24}",
            self.id,
            quoted,
            self.priority,
            if self.enabled { "Yes" } else { "No" },
            if self.local { "Yes" } else { "No" },
            self.timestamp,
            self.timestamp2,
            self.author,
            self.workshop_id,
        )
    }

    /// `readModStatus`'s per-line parse. Returns `None` on any malformed field, matching
    /// retail, which skips the line rather than failing the file.
    pub fn parse(line: &str) -> Option<StatusRow> {
        let (name, rest) = take_quoted_after_int(line)?;
        let id = line.split_whitespace().next()?.parse::<i32>().ok()?;
        if id == -1 {
            return None;
        }
        if name.is_empty() {
            return None;
        }
        let mut t = rest.split_whitespace();
        let priority = t.next()?.parse::<i32>().ok()?;
        if priority == -1 {
            return None;
        }
        let enabled = yes_no(t.next()?)?;
        let local = yes_no(t.next()?)?;
        let timestamp = t.next()?.parse::<i32>().ok()?;
        let timestamp2 = t.next()?.parse::<i32>().ok()?;
        if timestamp == -1 || timestamp2 == -1 {
            return None;
        }
        let author = t.next()?.parse::<u64>().ok()?;
        let workshop_id = t.next()?.parse::<u64>().ok()?;
        Some(StatusRow {
            id,
            name,
            priority,
            enabled,
            local,
            timestamp,
            timestamp2,
            author,
            workshop_id,
        })
    }
}

fn yes_no(s: &str) -> Option<bool> {
    match s {
        "Yes" => Some(true),
        "No" => Some(false),
        _ => None,
    }
}

/// Pull the `"quoted name"` field and return everything after it.
fn take_quoted_after_int(line: &str) -> Option<(String, &str)> {
    let open = line.find('"')?;
    let close = line[open + 1..].find('"')? + open + 1;
    Some((line[open + 1..close].to_string(), &line[close + 1..]))
}

/// Parse a whole file. The header line (and anything else that does not parse) is skipped.
pub fn parse(text: &str) -> Vec<StatusRow> {
    text.lines().filter_map(StatusRow::parse).collect()
}

/// The same retail-compatible parse, plus the lines a human should inspect. Retail silently
/// skips malformed rows; the workflow preserves that behavior while refusing to hide it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParseReport {
    pub rows: Vec<StatusRow>,
    /// One-based line number and source text. Blank lines and the measured header are not
    /// findings; every other non-row line is.
    pub ignored: Vec<(usize, String)>,
}

pub fn parse_report(text: &str) -> ParseReport {
    let mut out = ParseReport::default();
    for (i, line) in text.lines().enumerate() {
        if let Some(row) = StatusRow::parse(line) {
            out.rows.push(row);
        } else if !line.trim().is_empty() && !line.trim_start().starts_with("ID") {
            out.ignored.push((i + 1, line.to_string()));
        }
    }
    out
}

/// Render a whole file from a stack, in list order.
pub fn render(stack: &ContentStack) -> String {
    let mut out = header_line();
    out.push('\n');
    for (i, m) in stack.mods().iter().enumerate() {
        out.push_str(&row_for(i as i32, m).to_line());
        out.push('\n');
    }
    out
}

fn row_for(id: i32, m: &ModPackage) -> StatusRow {
    StatusRow {
        id,
        name: m.name.clone(),
        priority: m.priority,
        enabled: m.enabled,
        local: m.location == Some(StorageLocation::MyMods),
        timestamp: m.timestamp,
        timestamp2: m.timestamp2,
        author: m.author_id,
        workshop_id: m.published_file_id,
    }
}

/// `readModStatus`'s apply step: match rows to installed packages by `(name, local)`, set
/// `enabled` and `priority`, then `sortViaPriority`. Rows naming an uninstalled mod are
/// dropped; packages with no row keep whatever `buildPackage` gave them.
///
/// Returns the number of rows that found a package.
pub fn apply(stack: &mut ContentStack, rows: &[StatusRow]) -> usize {
    let mut applied = 0;
    for row in rows {
        let want_location = if row.local {
            StorageLocation::MyMods
        } else {
            StorageLocation::None
        };
        if let Some(m) = stack
            .mods_mut()
            .iter_mut()
            .find(|m| m.name.eq_ignore_ascii_case(&row.name) && m.location == Some(want_location))
        {
            m.priority = row.priority;
            m.enabled = row.enabled;
            m.timestamp = row.timestamp;
            m.timestamp2 = row.timestamp2;
            applied += 1;
        }
    }
    stack.sort_via_priority();
    applied
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vfs::ModPackage;

    #[test]
    fn header_widths_are_the_engines() {
        let h = header_line();
        assert!(h.starts_with("ID        "));
        assert_eq!(h.len(), HEADER_WIDTHS.iter().sum::<usize>());
        assert!(h.contains("WORKSHOPID"));
    }

    #[test]
    fn a_row_round_trips_including_a_name_with_spaces() {
        let r = StatusRow {
            id: 3,
            name: "Rise of the Moderns".into(),
            priority: 2,
            enabled: true,
            local: false,
            timestamp: 17,
            timestamp2: 19,
            author: 76561198000000001,
            workshop_id: 1234567890,
        };
        let back = StatusRow::parse(&r.to_line()).expect("parses");
        assert_eq!(back, r);
    }

    #[test]
    fn a_malformed_line_is_skipped_not_fatal() {
        let good = StatusRow {
            id: 0,
            name: "Good".into(),
            priority: 1,
            enabled: false,
            local: true,
            timestamp: 0,
            timestamp2: 0,
            author: 0,
            workshop_id: 0,
        };
        let text = format!("{}\ngarbage garbage\n{}\n", header_line(), good.to_line());
        let rows = parse(&text);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "Good");
        let detail = parse_report(&text);
        assert_eq!(detail.rows, rows);
        assert_eq!(detail.ignored, vec![(2, "garbage garbage".to_string())]);
    }

    #[test]
    fn apply_reorders_and_disables_but_cannot_introduce_a_mod() {
        let mut stack = ContentStack::new();
        let mut a = ModPackage::new("Alpha", "Alpha");
        a.priority = 1;
        let mut b = ModPackage::new("Bravo", "Bravo");
        b.priority = 2;
        stack.push(a).push(b);

        let rows = vec![
            StatusRow {
                id: 0,
                name: "Bravo".into(),
                priority: 1,
                enabled: true,
                local: true,
                timestamp: 0,
                timestamp2: 0,
                author: 0,
                workshop_id: 0,
            },
            StatusRow {
                id: 1,
                name: "Alpha".into(),
                priority: 2,
                enabled: false,
                local: true,
                timestamp: 0,
                timestamp2: 0,
                author: 0,
                workshop_id: 0,
            },
            StatusRow {
                id: 2,
                name: "NotInstalled".into(),
                priority: 3,
                enabled: true,
                local: true,
                timestamp: 0,
                timestamp2: 0,
                author: 0,
                workshop_id: 0,
            },
        ];
        assert_eq!(apply(&mut stack, &rows), 2);
        assert_eq!(stack.mods().len(), 2);
        assert_eq!(stack.mods()[0].name, "Bravo");
        assert!(!stack.mods()[1].enabled);
    }

    #[test]
    fn render_then_parse_is_stable() {
        let mut stack = ContentStack::new();
        let mut one = ModPackage::new("One", "One");
        one.timestamp = 123;
        one.timestamp2 = 456;
        stack.push(one);
        stack.push(ModPackage::new("Two", "Two"));
        let text = render(&stack);
        let rows = parse(&text);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].name, "One");
        assert_eq!(rows[0].timestamp, 123);
        assert_eq!(rows[0].timestamp2, 456);
        assert_eq!(rows[1].priority, 2);
    }

    #[test]
    fn apply_preserves_the_two_status_timestamps_but_not_identity_columns() {
        let mut stack = ContentStack::new();
        stack.push(ModPackage::new("Workshop Mod", "C:/ugc/42"));
        stack.mods_mut()[0].location = Some(StorageLocation::None);
        stack.mods_mut()[0].author_id = 33;
        stack.mods_mut()[0].published_file_id = 44;

        let row = StatusRow {
            id: 0,
            name: "Workshop Mod".into(),
            priority: 7,
            enabled: false,
            local: false,
            timestamp: 111,
            timestamp2: 222,
            author: 333,
            workshop_id: 444,
        };
        assert_eq!(apply(&mut stack, &[row]), 1);
        let m = &stack.mods()[0];
        assert_eq!((m.timestamp, m.timestamp2), (111, 222));
        assert_eq!((m.author_id, m.published_file_id), (33, 44));
    }
}
