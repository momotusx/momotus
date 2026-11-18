//! Source pins for the core-lite crates (plans/webos.md 3.1): runtime, time,
//! thread and file calls go through `mq_rt` or sit in native-only code. Each
//! banned call builds for wasm32 and then panics or does nothing there, so
//! only a pin finds it.
//!
//! The scan walks each crate's module tree from `src/lib.rs`, with strings
//! and comments blanked, and skips: items under `#[cfg(test)]` (or
//! `cfg(all(test, ..))`), items under `#[cfg(feature = "torrent")]` or
//! `"plugins"` (never on for wasm, 3.5), and the item after a
//! `// mq-rt: native-only` line, which must carry a `target_arch = "wasm32"`
//! attribute (a `mod x;` so marked skips its file).

use std::path::{Path, PathBuf};

/// The core-lite crates already on `mq_rt`.
const SCANNED: &[&str] = &[
    "mq-protocol",
    "mq-relparse",
    "mq-streams",
    "mq-addons",
    "mq-debrid",
    "mq-media",
    "mq-sync",
    "mq-tracking",
    "mq-import",
    "mq-turkcealtyazi",
    "mq-server",
    "mq-runtime",
    "mq-bridge",
    "mq-http",
    "mq-library",
];

/// Allowed until the named lane cuts it on wasm: (file, banned text, lane).
/// An entry that no longer matches anything fails, so it gets removed.
const PENDING: &[(&str, &str, &str)] = &[
    (
        "crates/mq-runtime/src/core.rs",
        "std::fs",
        "L3, Core::open on wasm",
    ),
    (
        "crates/mq-runtime/src/incognito.rs",
        "std::fs",
        "L3, Incognito on the memory VFS",
    ),
];

/// Banned anywhere in scanned code, as written.
const BANNED_TEXT: &[&str] = &[
    "tokio::spawn",
    "tokio::time::",
    "tokio::task::",
    "tokio::runtime::",
    "tokio::fs",
    "std::fs",
    "std::thread::",
    "std::env::temp_dir",
    "SystemTime::now",
    "std::time::Instant",
];

/// Banned as `use` paths, after the tree is expanded.
const BANNED_PATHS: &[&str] = &[
    "tokio::spawn",
    "tokio::time",
    "tokio::task",
    "tokio::runtime",
    "tokio::fs",
    "std::fs",
    "std::thread",
    "std::env::temp_dir",
    "std::time::Instant",
];

const MARKER: &str = "// mq-rt: native-only";

/// Strings, chars and comments become spaces, byte for byte (newlines
/// kept), so offsets match the raw text.
fn blank(text: &str) -> Vec<u8> {
    let src = text.as_bytes();
    let mut out = src.to_vec();
    let n = src.len();
    let mut i = 0;
    let wipe = |out: &mut Vec<u8>, from: usize, to: usize| {
        for b in &mut out[from..to.min(n)] {
            if *b != b'\n' {
                *b = b' ';
            }
        }
    };
    while i < n {
        let rest = &src[i..];
        let ident_before = i > 0 && (src[i - 1].is_ascii_alphanumeric() || src[i - 1] == b'_');
        if rest.starts_with(b"//") {
            let end = rest.iter().position(|&b| b == b'\n').map_or(n, |p| i + p);
            wipe(&mut out, i, end);
            i = end;
        } else if rest.starts_with(b"/*") {
            let end = find(src, i + 2, b"*/").map_or(n, |p| p + 2);
            wipe(&mut out, i, end);
            i = end;
        } else if !ident_before && raw_string_open(rest).is_some() {
            let (open, hashes) = raw_string_open(rest).unwrap();
            let mut close = b"\"".to_vec();
            close.extend(std::iter::repeat_n(b'#', hashes));
            let end = find(src, i + open, &close).map_or(n, |p| p + close.len());
            wipe(&mut out, i, end);
            i = end;
        } else if rest[0] == b'"' {
            let mut j = i + 1;
            while j < n && src[j] != b'"' {
                j += if src[j] == b'\\' { 2 } else { 1 };
            }
            wipe(&mut out, i, j + 1);
            i = j + 1;
        } else if rest[0] == b'\'' {
            // A char literal ('x', '\n', '\u{..}', '{'); a lifetime stays.
            let len = match rest.get(1) {
                Some(b'\\') => rest[2..].iter().position(|&b| b == b'\'').map(|p| p + 3),
                Some(_) => {
                    // `i + 1` follows an ASCII quote, so it is a char boundary.
                    let ch = text[i + 1..].chars().next().map_or(1, char::len_utf8);
                    (rest.get(1 + ch) == Some(&b'\'')).then_some(ch + 2)
                }
                None => None,
            };
            match len {
                Some(len) => {
                    wipe(&mut out, i, i + len);
                    i += len;
                }
                None => i += 1,
            }
        } else {
            i += 1;
        }
    }
    out
}

fn raw_string_open(rest: &[u8]) -> Option<(usize, usize)> {
    let start = usize::from(rest.first() == Some(&b'b'));
    if rest.get(start) != Some(&b'r') {
        return None;
    }
    let hashes = rest[start + 1..].iter().take_while(|&&b| b == b'#').count();
    (rest.get(start + 1 + hashes) == Some(&b'"')).then_some((start + 2 + hashes, hashes))
}

fn find(hay: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    hay.get(from..)?
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + from)
}

/// The end (exclusive) of the `[..]` that opens at `open`.
fn bracket_end(code: &[u8], open: usize) -> usize {
    let mut depth = 0;
    for (i, &b) in code.iter().enumerate().skip(open) {
        match b {
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    return i + 1;
                }
            }
            _ => {}
        }
    }
    code.len()
}

/// From `at`: further attributes, then what they apply to: an item (to its
/// closing brace, or its `;`), or a field, variant, arm or statement (to its
/// `,` or `;`, or the brace that closes the list it sits in). Returns its end
/// and the attributes' raw text. Angle brackets are not counted, so a `,` in
/// a generic type ends it early: that can only show more, never hide code.
fn item_end(code: &[u8], raw: &[u8], mut at: usize) -> (usize, String) {
    let mut attrs = String::new();
    loop {
        while at < code.len() && code[at].is_ascii_whitespace() {
            at += 1;
        }
        if code[at..].starts_with(b"#[") {
            let end = bracket_end(code, at + 1);
            attrs.push_str(&String::from_utf8_lossy(&raw[at..end]));
            at = end;
        } else {
            break;
        }
    }
    // An item's signature may hold a `,` (`Result<A, B>`); only a field,
    // variant, arm or statement ends at one. The item's first word, after
    // its visibility and qualifiers, says which it is.
    let head = String::from_utf8_lossy(&code[at..code.len().min(at + 80)]).into_owned();
    let item = head
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .filter(|w| !w.is_empty())
        .find(|w| {
            !matches!(
                *w,
                "pub" | "crate" | "super" | "in" | "async" | "unsafe" | "extern"
            )
        })
        .is_some_and(|w| {
            matches!(
                w,
                "fn" | "struct"
                    | "enum"
                    | "impl"
                    | "mod"
                    | "trait"
                    | "type"
                    | "static"
                    | "const"
                    | "use"
                    | "union"
                    | "macro_rules"
            )
        });
    let mut depth = 0i32;
    for (i, &b) in code.iter().enumerate().skip(at) {
        match b {
            b'{' | b'(' | b'[' => depth += 1,
            b'}' | b')' | b']' => {
                depth -= 1;
                // The list it sits in closes: a last field or arm.
                if depth < 0 {
                    return (i, attrs);
                }
                // A body ends it, unless an arm's body or an `else` follows.
                let next = code[i + 1..].iter().position(|c| !c.is_ascii_whitespace());
                let next = next.map_or(&[][..], |n| &code[i + 1 + n..]);
                if b == b'}' && depth == 0 && !next.starts_with(b"=>") && !next.starts_with(b"else")
                {
                    return (i + 1, attrs);
                }
            }
            b';' if depth == 0 => return (i + 1, attrs),
            b',' if depth == 0 && !item => return (i + 1, attrs),
            _ => {}
        }
    }
    (code.len(), attrs)
}

fn skips(attr: &str) -> bool {
    let a: String = attr.chars().filter(|c| !c.is_whitespace()).collect();
    a == "#[cfg(test)]"
        || a.starts_with("#[cfg(all(test,")
        || a == r#"#[cfg(feature="torrent")]"#
        || a == r#"#[cfg(feature="plugins")]"#
}

struct Scanned {
    /// (1-based line, what was found)
    hits: Vec<(usize, String)>,
    /// Module files declared by kept code (`mod x;`).
    mods: Vec<String>,
    /// Markers whose item has no wasm32 attribute.
    bare_markers: Vec<usize>,
}

fn line_of(src: &[u8], at: usize) -> usize {
    src[..at].iter().filter(|&&b| b == b'\n').count() + 1
}

fn scan(src: &str) -> Scanned {
    let raw = src.as_bytes();
    let mut code = blank(src);
    let mut bare_markers = Vec::new();
    let whole_file_off = String::from_utf8_lossy(&code)
        .lines()
        .map(str::trim)
        .take_while(|l| l.is_empty() || l.starts_with("#!["))
        .any(|l| skips(&l.replacen("#![", "#[", 1)));
    if whole_file_off {
        return Scanned {
            hits: Vec::new(),
            mods: Vec::new(),
            bare_markers,
        };
    }
    // Regions to drop: (start, end).
    let mut drop = Vec::new();
    let mut i = 0;
    while i < code.len() {
        let line_start = || {
            raw[..i]
                .iter()
                .rev()
                .take_while(|&&b| b != b'\n')
                .all(u8::is_ascii_whitespace)
        };
        if raw[i..].starts_with(MARKER.as_bytes()) && line_start() {
            let line_end = find(raw, i, b"\n").unwrap_or(raw.len());
            let (end, attrs) = item_end(&code, raw, line_end);
            if !attrs.contains(r#"target_arch = "wasm32""#) {
                bare_markers.push(line_of(raw, i));
            }
            drop.push((i, end));
            i = end;
        } else if code[i..].starts_with(b"#[") {
            let end = bracket_end(&code, i + 1);
            if skips(&String::from_utf8_lossy(&raw[i..end])) {
                let (item, _) = item_end(&code, raw, end);
                drop.push((i, item));
                i = item;
            } else {
                i = end;
            }
        } else {
            i += 1;
        }
    }
    for (from, to) in drop {
        for b in &mut code[from..to] {
            if *b != b'\n' {
                *b = b' ';
            }
        }
    }
    let kept = String::from_utf8_lossy(&code).into_owned();
    let mut hits = Vec::new();
    let mut mods = Vec::new();
    let raw_lines: Vec<&str> = src.lines().collect();
    for (n, line) in kept.lines().enumerate() {
        for banned in BANNED_TEXT {
            // `std::thread::panicking()` reads a flag and works on wasm.
            let fine = |(at, b): (usize, &str)| line[at + b.len()..].starts_with("panicking()");
            if line.contains(banned) && !line.match_indices(banned).all(fine) {
                hits.push((n + 1, (*banned).to_owned()));
            }
        }
        if line
            .match_indices("spawn_blocking")
            .any(|(at, _)| !line[..at].ends_with("mq_rt::"))
        {
            hits.push((n + 1, "spawn_blocking".to_owned()));
        }
        let t = line.trim();
        let decl = t
            .strip_prefix("pub(crate) ")
            .or_else(|| t.strip_prefix("pub "))
            .unwrap_or(t);
        if let Some(name) = decl.strip_prefix("mod ").and_then(|r| r.strip_suffix(';')) {
            // `#[path = "x.rs"]` on the line before: that file, beside this one.
            let path = n
                .checked_sub(1)
                .and_then(|p| raw_lines.get(p))
                .and_then(|l| l.trim().strip_prefix("#[path = \""))
                .and_then(|l| l.strip_suffix("\"]"));
            mods.push(match path {
                Some(path) => format!("path:{path}"),
                None => name.trim().to_owned(),
            });
        }
    }
    for (at, stmt) in uses(&kept) {
        let mut paths = Vec::new();
        expand("", &stmt, &mut paths);
        for path in paths {
            if let Some(banned) = BANNED_PATHS
                .iter()
                .find(|b| path == **b || path.starts_with(&format!("{b}::")))
            {
                hits.push((line_of(kept.as_bytes(), at), format!("use {banned}")));
            }
        }
    }
    Scanned {
        hits,
        mods,
        bare_markers,
    }
}

/// Every `use` tree in kept code, whitespace folded (`as` becomes `@`).
fn uses(kept: &str) -> Vec<(usize, String)> {
    let b = kept.as_bytes();
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(at) = find(b, from, b"use ") {
        from = at + 4;
        let boundary = at == 0 || !(b[at - 1].is_ascii_alphanumeric() || b[at - 1] == b'_');
        if !boundary {
            continue;
        }
        let Some(end) = find(b, at, b";") else { break };
        let tree = kept[at + 4..end]
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let tree = tree.replace(" as ", "@").replace(' ', "");
        out.push((at, tree));
    }
    out
}

fn expand(prefix: &str, tree: &str, out: &mut Vec<String>) {
    match tree.find('{') {
        Some(open) if tree.ends_with('}') => {
            let head = format!("{prefix}{}", &tree[..open]);
            let inner = &tree[open + 1..tree.len() - 1];
            let mut depth = 0;
            let mut start = 0;
            for (i, c) in inner.char_indices() {
                match c {
                    '{' => depth += 1,
                    '}' => depth -= 1,
                    ',' if depth == 0 => {
                        expand(&head, &inner[start..i], out);
                        start = i + 1;
                    }
                    _ => {}
                }
            }
            expand(&head, &inner[start..], out);
        }
        _ => {
            let path = tree.split('@').next().unwrap_or(tree);
            if path.is_empty() {
                return;
            }
            let full = format!("{prefix}{path}");
            let full = full.strip_prefix("::").unwrap_or(&full);
            out.push(full.strip_suffix("::self").unwrap_or(full).to_owned());
        }
    }
}

/// `x.rs` or `x/mod.rs` under `dir`.
fn module_file(dir: &Path, name: &str) -> PathBuf {
    let flat = dir.join(format!("{name}.rs"));
    if flat.exists() {
        flat
    } else {
        dir.join(name).join("mod.rs")
    }
}

fn walk(file: &Path, workspace: &Path, report: &mut Vec<String>, seen: &mut Vec<(String, String)>) {
    let src = std::fs::read_to_string(file).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
    let rel = file.strip_prefix(workspace).unwrap().display().to_string();
    let scanned = scan(&src);
    for line in scanned.bare_markers {
        report.push(format!(
            "{rel}:{line}: `{MARKER}` on an item with no wasm32 cfg"
        ));
    }
    for (line, what) in scanned.hits {
        let pending = PENDING
            .iter()
            .any(|(f, b, _)| *f == rel && what.contains(b));
        if pending {
            seen.push((rel.clone(), what));
        } else {
            report.push(format!("{rel}:{line}: {what}"));
        }
    }
    let stem = file.file_stem().unwrap().to_str().unwrap();
    let dir = if matches!(stem, "lib" | "mod") {
        file.parent().unwrap().to_path_buf()
    } else {
        file.parent().unwrap().join(stem)
    };
    for name in scanned.mods {
        let next = match name.strip_prefix("path:") {
            Some(path) => file.parent().unwrap().join(path),
            None => module_file(&dir, &name),
        };
        walk(&next, workspace, report, seen);
    }
}

#[test]
fn core_lite_code_uses_mq_rt() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let workspace = workspace.canonicalize().unwrap();
    let mut report = Vec::new();
    let mut seen = Vec::new();
    for krate in SCANNED {
        walk(
            &workspace.join("crates").join(krate).join("src/lib.rs"),
            &workspace,
            &mut report,
            &mut seen,
        );
    }
    for (file, banned, lane) in PENDING {
        if !seen.iter().any(|(f, w)| f == file && w.contains(banned)) {
            report.push(format!(
                "{file}: pending `{banned}` ({lane}) is gone; drop the entry"
            ));
        }
    }
    assert!(
        report.is_empty(),
        "use mq_rt, or mark native-only code (plans/webos.md 3.1):\n{}",
        report.join("\n")
    );
}

#[test]
fn the_scan_sees_what_it_should() {
    let src = r#"
use std::time::{Duration, Instant as Clock};
use tokio::{sync::Mutex, time::sleep};
use std::thread_local;
const A: &str = "tokio::spawn in a string";
// tokio::spawn in a comment
fn ok() { let _ = '{'; mq_rt::spawn_blocking(|| ()); }
#[cfg(test)]
fn helper(
    x: u32,
) {
    tokio::spawn(async {});
}
#[cfg(test)]
mod tests;
#[cfg(feature = "torrent")]
mod torrent;
// mq-rt: native-only
#[cfg(not(target_arch = "wasm32"))]
mod file;
// mq-rt: native-only
fn bare() { std::fs::read("x"); }
mod kept;
fn bad() { tokio::time::sleep(D); let _ = SystemTime::now(); handle.spawn_blocking(f); }
fn fine() { if std::thread::panicking() { std::thread::sleep(D); } }
"#;
    let s = scan(src);
    let hits: Vec<String> = s.hits.iter().map(|(l, w)| format!("{l} {w}")).collect();
    assert_eq!(
        hits,
        [
            "24 tokio::time::",
            "24 SystemTime::now",
            "24 spawn_blocking",
            "25 std::thread::",
            "2 use std::time::Instant",
            "3 use tokio::time",
        ]
    );
    assert_eq!(s.mods, ["kept"]);
    assert_eq!(s.bare_markers, [21]);
}

#[test]
fn a_skipped_field_or_arm_hides_only_itself() {
    let src = r#"
struct S {
    #[cfg(feature = "torrent")]
    engine: HashMap<String, Engine>,
    #[cfg(feature = "torrent")]
    last: u8
}
impl S {
    fn new() -> Self { tokio::time::sleep(D); S {} }
}
fn f(c: Content) -> Result<u8, E> {
    match c {
        #[cfg(feature = "torrent")]
        Content::Torrent { claim, file } => { tokio::spawn(x); }
        #[cfg(feature = "plugins")]
        Content::Plugin => tokio::spawn(y),
        _ => tokio::spawn(z),
    }
}
#[cfg(test)]
fn helper(a: Result<u8, E>) { std::fs::read("x"); }
fn after() { std::fs::read("y"); }
"#;
    let hits: Vec<String> = scan(src)
        .hits
        .iter()
        .map(|(l, w)| format!("{l} {w}"))
        .collect();
    assert_eq!(hits, ["9 tokio::time::", "17 tokio::spawn", "22 std::fs"]);
}
