//! Receiver-side file name safety (spec 6.6).

use crate::consts::{MAX_NAME_LEN, PART_SUFFIX};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1", "LPT2",
    "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Turn an untrusted name from the sender into a safe single path component.
pub fn sanitize_name(raw: &str) -> String {
    // Keep only the last path component, whatever separator the sender used.
    let last = raw.rsplit(['/', '\\']).next().unwrap_or("");
    let mut s: String = last
        .chars()
        .filter(|c| !c.is_control())
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '|' | '?' | '*' => '_',
            c => c,
        })
        .collect();
    s = s.trim_matches(|c: char| c == '.' || c == ' ').to_string();
    if s.is_empty() {
        s = "file".to_string();
    }
    let stem_upper = s.split('.').next().unwrap_or("").trim_end().to_ascii_uppercase();
    if RESERVED.contains(&stem_upper.as_str()) {
        s = format!("_{s}");
    }
    // Leave room for " (999)" and the temporary suffix.
    let budget = MAX_NAME_LEN - 6 - PART_SUFFIX.len();
    truncate_keep_ext(&s, budget)
}

fn truncate_keep_ext(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let (stem, ext) = match s.rfind('.') {
        Some(i) if i > 0 && s.len() - i <= 16 => (&s[..i], &s[i..]),
        _ => (s, ""),
    };
    let mut keep = max_bytes.saturating_sub(ext.len());
    while keep > 0 && !stem.is_char_boundary(keep) {
        keep -= 1;
    }
    format!("{}{}", &stem[..keep], ext)
}

/// `name.ext` -> `name (n).ext`
pub fn numbered(name: &str, n: u32) -> String {
    match name.rfind('.') {
        Some(i) if i > 0 => format!("{} ({n}){}", &name[..i], &name[i..]),
        _ => format!("{name} ({n})"),
    }
}

/// Pick a final path in `dir` that neither exists on disk nor is reserved by `taken`.
pub fn unique_target(dir: &Path, name: &str, taken: &mut HashSet<String>) -> PathBuf {
    let mut n = 0u32;
    loop {
        let candidate = if n == 0 { name.to_string() } else { numbered(name, n) };
        let key = candidate.to_lowercase();
        let path = dir.join(&candidate);
        let part = part_path(&path);
        if !taken.contains(&key) && !path.exists() && !part.exists() {
            taken.insert(key);
            return path;
        }
        n += 1;
    }
}

pub fn part_path(final_path: &Path) -> PathBuf {
    let mut s = final_path.as_os_str().to_owned();
    s.push(PART_SUFFIX);
    PathBuf::from(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_traversal_and_separators() {
        assert_eq!(sanitize_name("../../etc/passwd"), "passwd");
        assert_eq!(sanitize_name("..\\..\\Windows\\win.ini"), "win.ini");
        assert_eq!(sanitize_name(".."), "file");
        assert_eq!(sanitize_name("/"), "file");
        assert_eq!(sanitize_name("C:evil.txt"), "C_evil.txt");
    }

    #[test]
    fn removes_controls_and_edges() {
        assert_eq!(sanitize_name("a\u{0}b\nc.txt"), "abc.txt");
        assert_eq!(sanitize_name("  .hidden. "), "hidden");
        assert_eq!(sanitize_name("x?.txt"), "x_.txt");
    }

    #[test]
    fn reserved_names() {
        assert_eq!(sanitize_name("CON"), "_CON");
        assert_eq!(sanitize_name("nul.txt"), "_nul.txt");
        assert_eq!(sanitize_name("com9.tar.gz"), "_com9.tar.gz");
        assert_eq!(sanitize_name("console.txt"), "console.txt");
    }

    #[test]
    fn length_limit_keeps_extension() {
        let long = format!("{}.mp4", "a".repeat(400));
        let s = sanitize_name(&long);
        assert!(s.len() + 6 + PART_SUFFIX.len() <= MAX_NAME_LEN);
        assert!(s.ends_with(".mp4"));
        let multi = "é".repeat(300);
        assert!(sanitize_name(&multi).len() <= MAX_NAME_LEN);
    }

    #[test]
    fn collisions() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.txt"), b"x").unwrap();
        let mut taken = HashSet::new();
        assert_eq!(unique_target(d.path(), "a.txt", &mut taken), d.path().join("a (1).txt"));
        assert_eq!(unique_target(d.path(), "a.txt", &mut taken), d.path().join("a (2).txt"));
        assert_eq!(numbered("noext", 3), "noext (3)");
    }
}
