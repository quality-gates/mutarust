use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Changed source lines from a Git comparison.
pub(crate) struct ChangedLines {
    root: PathBuf,
    files: BTreeMap<PathBuf, Vec<LineRange>>,
}

#[derive(Clone, Copy)]
struct LineRange {
    first: usize,
    last: usize,
}

impl ChangedLines {
    pub(crate) fn load(base: Option<&str>) -> Result<Self, String> {
        let root = repository_root()?;
        let base = match base {
            Some(base) => base.to_owned(),
            None => default_base(&root)?,
        };
        verify_base(&root, &base)?;
        let comparison = merge_base(&root, &base)?;
        let output = run_git(
            &root,
            &[
                "diff",
                "--no-ext-diff",
                "--no-color",
                "--find-renames",
                "--unified=0",
                "--src-prefix=a/",
                "--dst-prefix=b/",
                &comparison,
                "--",
            ],
        )?;
        if !output.status.success() {
            return Err(git_failure("could not read changed Git lines", &output));
        }
        let text = String::from_utf8(output.stdout)
            .map_err(|error| format!("could not read changed Git lines: {error}"))?;
        Ok(Self {
            root,
            files: parse_changed_lines(&text)?,
        })
    }

    pub(crate) fn includes(&self, source: &Path, line: usize) -> bool {
        source
            .strip_prefix(&self.root)
            .ok()
            .and_then(|path| self.files.get(path))
            .is_some_and(|ranges| ranges.iter().any(|range| range.includes(line)))
    }

    pub(crate) fn validate_source(&self, source: &Path) -> Result<(), String> {
        if source.starts_with(&self.root) {
            Ok(())
        } else {
            Err(format!(
                "could not use {} for Git changed-line selection: it is outside Git repository {}",
                source.display(),
                self.root.display()
            ))
        }
    }
}

impl LineRange {
    fn includes(self, line: usize) -> bool {
        self.first <= line && line <= self.last
    }
}

fn repository_root() -> Result<PathBuf, String> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .map_err(|error| format!("could not run Git: {error}"))?;
    if !output.status.success() {
        return Err(git_failure("could not find a Git repository", &output));
    }
    let root = String::from_utf8(output.stdout)
        .map_err(|error| format!("could not read Git repository path: {error}"))?;
    fs::canonicalize(root.trim())
        .map_err(|error| format!("could not resolve Git repository: {error}"))
}

fn default_base(root: &Path) -> Result<String, String> {
    let output = run_git(
        root,
        &[
            "symbolic-ref",
            "--quiet",
            "--short",
            "refs/remotes/origin/HEAD",
        ],
    )?;
    if output.status.success() {
        let reference = String::from_utf8(output.stdout)
            .map_err(|error| format!("could not read Git remote default branch: {error}"))?;
        let reference = reference.trim();
        if !reference.is_empty() {
            return Ok(reference.to_owned());
        }
        return Err("could not read Git remote default branch".to_owned());
    }
    if output.status.code() == Some(1) {
        Ok("master".to_owned())
    } else {
        Err(git_failure(
            "could not find Git remote default branch",
            &output,
        ))
    }
}

fn verify_base(root: &Path, base: &str) -> Result<(), String> {
    let revision = format!("{base}^{{commit}}");
    let output = run_git(root, &["rev-parse", "--verify", "--quiet", &revision])?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!("could not resolve Git diff base {base}"))
    }
}

fn merge_base(root: &Path, base: &str) -> Result<String, String> {
    let output = run_git(root, &["merge-base", base, "HEAD"])?;
    if output.status.success() {
        let merge_base = String::from_utf8(output.stdout)
            .map_err(|error| format!("could not read Git merge base: {error}"))?;
        let merge_base = merge_base.trim();
        if !merge_base.is_empty() {
            return Ok(merge_base.to_owned());
        }
        return Err("could not read Git merge base".to_owned());
    }
    if output.status.code() == Some(1) {
        Ok(base.to_owned())
    } else {
        Err(git_failure("could not find Git merge base", &output))
    }
}

fn run_git(root: &Path, arguments: &[&str]) -> Result<Output, String> {
    Command::new("git")
        .args(arguments)
        .current_dir(root)
        .output()
        .map_err(|error| format!("could not run Git: {error}"))
}

fn git_failure(prefix: &str, output: &Output) -> String {
    let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    if detail.is_empty() {
        prefix.to_owned()
    } else {
        format!("{prefix}: {detail}")
    }
}

fn parse_changed_lines(text: &str) -> Result<BTreeMap<PathBuf, Vec<LineRange>>, String> {
    let mut files = BTreeMap::<PathBuf, Vec<LineRange>>::new();
    let mut source = None;
    for line in text.lines() {
        if let Some(header) = line.strip_prefix("+++ ") {
            source = parse_source_path(header)?;
            continue;
        }
        if !line.starts_with("@@ ") {
            continue;
        }
        let Some(path) = source.as_ref() else {
            continue;
        };
        let Some(range) = parse_hunk_range(line)? else {
            continue;
        };
        files.entry(path.clone()).or_default().push(range);
    }
    for ranges in files.values_mut() {
        merge_ranges(ranges);
    }
    Ok(files)
}

fn parse_source_path(header: &str) -> Result<Option<PathBuf>, String> {
    if header == "/dev/null" {
        return Ok(None);
    }
    if let Some(quoted) = header.strip_prefix("\"b/") {
        return unquote_git_path(quoted).map(Some);
    }
    let path = header
        .strip_prefix("b/")
        .ok_or_else(|| "could not parse changed Git source path".to_owned())?;
    let path = path.split_once('\t').map_or(path, |(path, _)| path);
    Ok(Some(PathBuf::from(path)))
}

fn unquote_git_path(quoted: &str) -> Result<PathBuf, String> {
    let mut bytes = Vec::<u8>::new();
    let mut chars = quoted.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '"' {
            let path = String::from_utf8(bytes)
                .map_err(|_| "could not parse changed Git source path".to_owned())?;
            return Ok(PathBuf::from(path));
        }
        if ch == '\\' {
            let escaped = chars
                .next()
                .ok_or_else(|| "could not parse changed Git source path".to_owned())?;
            decode_git_escape(escaped, &mut chars, &mut bytes)?;
        } else {
            let mut buffer = [0u8; 4];
            bytes.extend_from_slice(ch.encode_utf8(&mut buffer).as_bytes());
        }
    }
    Err("could not parse changed Git source path".to_owned())
}

fn decode_git_escape(
    escaped: char,
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    bytes: &mut Vec<u8>,
) -> Result<(), String> {
    if let Some(byte) = git_simple_escape(escaped) {
        bytes.push(byte);
        return Ok(());
    }
    if ('0'..='7').contains(&escaped) {
        decode_git_octal(escaped, chars, bytes);
        return Ok(());
    }
    Err("could not parse changed Git source path".to_owned())
}

fn git_simple_escape(escaped: char) -> Option<u8> {
    const ESCAPES: &[(char, u8)] = &[
        ('a', 0x07),
        ('b', 0x08),
        ('t', b'\t'),
        ('n', b'\n'),
        ('v', 0x0B),
        ('f', 0x0C),
        ('r', b'\r'),
        ('"', b'"'),
        ('\\', b'\\'),
    ];
    ESCAPES
        .iter()
        .find_map(|&(key, byte)| (key == escaped).then_some(byte))
}

fn decode_git_octal(
    escaped: char,
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    bytes: &mut Vec<u8>,
) {
    let mut value = (escaped as u8) - b'0';
    for _ in 0..2 {
        if let Some(&next @ '0'..='7') = chars.peek() {
            chars.next();
            value = (value << 3) + ((next as u8) - b'0');
        } else {
            break;
        }
    }
    bytes.push(value);
}

fn parse_hunk_range(header: &str) -> Result<Option<LineRange>, String> {
    let mut fields = header.split_whitespace();
    if fields.next() != Some("@@") || fields.next().is_none() {
        return Err("could not parse changed Git hunk".to_owned());
    }
    let new = fields
        .next()
        .and_then(|value| value.strip_prefix('+'))
        .ok_or_else(|| "could not parse changed Git hunk".to_owned())?;
    if fields.next() != Some("@@") {
        return Err("could not parse changed Git hunk".to_owned());
    }
    let (start, count) = new.split_once(',').unwrap_or((new, "1"));
    let start = start
        .parse::<usize>()
        .map_err(|_| "could not parse changed Git hunk".to_owned())?;
    let count = count
        .parse::<usize>()
        .map_err(|_| "could not parse changed Git hunk".to_owned())?;
    if count == 0 {
        return Ok(None);
    }
    let last = start
        .checked_add(count - 1)
        .ok_or_else(|| "could not parse changed Git hunk".to_owned())?;
    if start == 0 {
        return Err("could not parse changed Git hunk".to_owned());
    }
    Ok(Some(LineRange { first: start, last }))
}

fn merge_ranges(ranges: &mut Vec<LineRange>) {
    ranges.sort_by_key(|range| range.first);
    let mut merged = Vec::<LineRange>::with_capacity(ranges.len());
    for range in ranges.drain(..) {
        if let Some(previous) = merged.last_mut() {
            if range.first <= previous.last.saturating_add(1) {
                previous.last = previous.last.max(range.last);
                continue;
            }
        }
        merged.push(range);
    }
    *ranges = merged;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_source_path_unquoted_and_dev_null() {
        assert_eq!(
            parse_source_path("/dev/null").expect("dev null must parse"),
            None
        );
        assert_eq!(
            parse_source_path("b/src/lib.rs").expect("unquoted path must parse"),
            Some(PathBuf::from("src/lib.rs"))
        );
        assert_eq!(
            parse_source_path("b/src/lib.rs\t").expect("unquoted path with tab must parse"),
            Some(PathBuf::from("src/lib.rs"))
        );
        assert!(parse_source_path("a/src/lib.rs").is_err());
    }

    #[test]
    fn parse_source_path_quoted_variations() {
        assert_eq!(
            parse_source_path("\"b/path with spaces.txt\"").expect("spaces must parse"),
            Some(PathBuf::from("path with spaces.txt"))
        );
        assert_eq!(
            parse_source_path("\"b/path/with\\\"quote\\\".rs\"").expect("escaped quote must parse"),
            Some(PathBuf::from("path/with\"quote\".rs"))
        );
        assert_eq!(
            parse_source_path("\"b/file\\ttab.txt\"").expect("tab must parse"),
            Some(PathBuf::from("file\ttab.txt"))
        );
        assert_eq!(
            parse_source_path("\"b/file\\nnewline.txt\"").expect("newline must parse"),
            Some(PathBuf::from("file\nnewline.txt"))
        );
        assert_eq!(
            parse_source_path("\"b/path\\\\backslash.rs\"").expect("backslash must parse"),
            Some(PathBuf::from("path\\backslash.rs"))
        );
        assert_eq!(
            parse_source_path("\"b/\\321\\204\\320\\260\\320\\271\\320\\273.txt\"")
                .expect("octal UTF-8 bytes must parse"),
            Some(PathBuf::from("файл.txt"))
        );
        assert_eq!(
            parse_source_path("\"b/alerts\\a\\b\\f\\r\\v.txt\"").expect("c-escapes must parse"),
            Some(PathBuf::from("alerts\x07\x08\x0c\r\x0b.txt"))
        );
    }

    #[test]
    fn parse_source_path_invalid_quoted_inputs_fail() {
        assert!(parse_source_path("\"b/unterminated").is_err());
        assert!(parse_source_path("\"b/invalid\\xescape\"").is_err());
        assert!(parse_source_path("\"a/wrong_prefix\"").is_err());
    }

    #[test]
    fn parse_changed_lines_with_mixed_quoted_and_unquoted_diff() {
        let diff = "\
--- /dev/null
+++ \"b/non_rust with\\ttab.txt\"
@@ -0,0 +1,2 @@
+one
+two
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -10,1 +10,1 @@
-old
+new
--- /dev/null
+++ \"b/src/with space.rs\"
@@ -0,0 +5,3 @@
+line 5
+line 6
+line 7
";
        let changed = parse_changed_lines(diff).expect("diff must parse");
        assert!(changed.contains_key(Path::new("non_rust with\ttab.txt")));
        assert_eq!(changed[Path::new("src/lib.rs")].len(), 1);
        assert_eq!(changed[Path::new("src/lib.rs")][0].first, 10);
        assert_eq!(changed[Path::new("src/lib.rs")][0].last, 10);
        assert_eq!(changed[Path::new("src/with space.rs")].len(), 1);
        assert_eq!(changed[Path::new("src/with space.rs")][0].first, 5);
        assert_eq!(changed[Path::new("src/with space.rs")][0].last, 7);
    }
}
