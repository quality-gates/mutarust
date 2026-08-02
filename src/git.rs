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
    let path = header
        .strip_prefix("b/")
        .ok_or_else(|| "could not parse changed Git source path".to_owned())?;
    let path = path.split_once('\t').map_or(path, |(path, _)| path);
    Ok(Some(PathBuf::from(path)))
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
    let (start, count) = new.split_once(',').map_or((new, "1"), |range| range);
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
        if let Some(previous) = merged.last_mut()
            && range.first <= previous.last.saturating_add(1)
        {
            previous.last = previous.last.max(range.last);
        } else {
            merged.push(range);
        }
    }
    *ranges = merged;
}
