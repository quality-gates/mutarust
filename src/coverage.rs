use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

/// A Cargo test that has one exact test name in one test target.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct TestIdentity {
    pub(crate) package: String,
    pub(crate) target: TestTarget,
    pub(crate) name: String,
}

/// The Cargo target that contains an exact test.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum TestTarget {
    Library,
    Binary(String),
    Example(String),
    Integration(String),
    Benchmark(String),
}

/// Line coverage collected from one or more LLVM coverage reports.
#[derive(Default)]
pub(crate) struct CoverageMap {
    lines: BTreeMap<PathBuf, BTreeSet<usize>>,
}

impl CoverageMap {
    pub(crate) fn add(&mut self, profile: CoverageProfile) {
        for (source, lines) in profile.lines {
            self.lines.entry(source).or_default().extend(lines);
        }
    }

    pub(crate) fn covers(&self, source: &Path, line: usize) -> bool {
        self.lines
            .get(source)
            .is_some_and(|lines| lines.contains(&line))
    }
}

/// Per-test line coverage collected from LLVM coverage reports.
#[derive(Default)]
pub(crate) struct PerTestCoverageMap {
    tests: BTreeMap<(PathBuf, usize), BTreeSet<TestIdentity>>,
}

impl PerTestCoverageMap {
    pub(crate) fn add(&mut self, profile: CoverageProfile, test: &TestIdentity) {
        for (source, lines) in profile.lines {
            for line in lines {
                self.tests
                    .entry((source.clone(), line))
                    .or_default()
                    .insert(test.clone());
            }
        }
    }

    pub(crate) fn tests_for(&self, source: &Path, line: usize) -> Option<Vec<TestIdentity>> {
        self.tests
            .get(&(source.to_path_buf(), line))
            .map(|tests| tests.iter().cloned().collect())
    }
}

/// One parsed LCOV report.
pub(crate) struct CoverageProfile {
    lines: BTreeMap<PathBuf, BTreeSet<usize>>,
}

impl CoverageProfile {
    /// Maps profile paths from an isolated workspace back to the user workspace.
    pub(crate) fn restore_workspace_paths(
        mut self,
        temporary: &Path,
        layout_root: &Path,
    ) -> Result<Self, String> {
        let mut lines: BTreeMap<PathBuf, BTreeSet<usize>> = BTreeMap::new();
        for (source, covered) in self.lines {
            let original = if let Ok(relative) = source.strip_prefix(temporary) {
                layout_root.join(relative)
            } else if source.starts_with(layout_root) {
                source
            } else {
                continue;
            };
            let original = fs::canonicalize(&original).map_err(|error| {
                format!(
                    "could not resolve isolated LLVM coverage source {}: {error}",
                    original.display()
                )
            })?;
            lines.entry(original).or_default().extend(covered);
        }
        self.lines = lines;
        Ok(self)
    }
}

/// Parses the LCOV data that `cargo llvm-cov --lcov` writes.
pub(crate) fn parse_lcov(path: &Path, source_root: &Path) -> Result<CoverageProfile, String> {
    let text = fs::read_to_string(path).map_err(|error| {
        format!(
            "could not read LLVM coverage data {}: {error}",
            path.display()
        )
    })?;
    parse_lcov_text(&text, source_root)
}

fn parse_lcov_text(text: &str, source_root: &Path) -> Result<CoverageProfile, String> {
    let mut profile = CoverageProfile {
        lines: BTreeMap::new(),
    };
    let mut source = None;
    let mut saw_data = false;
    for record in text.lines() {
        if record.is_empty() {
            continue;
        }
        parse_lcov_record(
            record,
            source_root,
            &mut source,
            &mut profile,
            &mut saw_data,
        )?;
    }
    if source.is_some() {
        return Err("LLVM coverage record has no end marker".to_owned());
    }
    if !text.trim().is_empty() && !saw_data {
        return Err("LLVM coverage data has no line records".to_owned());
    }
    Ok(profile)
}

fn parse_lcov_record(
    record: &str,
    source_root: &Path,
    source: &mut Option<PathBuf>,
    profile: &mut CoverageProfile,
    saw_data: &mut bool,
) -> Result<(), String> {
    if let Some(path) = record.strip_prefix("SF:") {
        return set_source(path, source_root, source);
    }
    if let Some(data) = record.strip_prefix("DA:") {
        return add_line_data(data, source, profile, saw_data);
    }
    if record == "end_of_record" {
        return end_record(source);
    }
    Ok(())
}

fn set_source(value: &str, source_root: &Path, source: &mut Option<PathBuf>) -> Result<(), String> {
    if source.is_some() {
        return Err("LLVM coverage record has more than one source file".to_owned());
    }
    *source = Some(resolve_source(value, source_root)?);
    Ok(())
}

fn add_line_data(
    value: &str,
    source: &Option<PathBuf>,
    profile: &mut CoverageProfile,
    saw_data: &mut bool,
) -> Result<(), String> {
    let source = source
        .as_ref()
        .ok_or_else(|| "LLVM coverage line has no source file".to_owned())?;
    let (line, hits) = parse_line_data(value)?;
    *saw_data = true;
    if hits > 0 {
        profile
            .lines
            .entry(source.clone())
            .or_default()
            .insert(line);
    }
    Ok(())
}

fn end_record(source: &mut Option<PathBuf>) -> Result<(), String> {
    if source.is_none() {
        return Err("LLVM coverage record has no source file".to_owned());
    }
    *source = None;
    Ok(())
}

fn resolve_source(value: &str, source_root: &Path) -> Result<PathBuf, String> {
    if value.is_empty() {
        return Err("LLVM coverage source path is empty".to_owned());
    }
    let path = Path::new(value);
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        source_root.join(path)
    };
    fs::canonicalize(&path).map_err(|error| {
        format!(
            "could not resolve LLVM coverage source {}: {error}",
            path.display()
        )
    })
}

fn parse_line_data(value: &str) -> Result<(usize, u64), String> {
    let mut fields = value.split(',');
    let line = fields
        .next()
        .ok_or_else(|| "LLVM coverage line record is invalid".to_owned())?
        .parse::<usize>()
        .ok()
        .filter(|line| *line > 0)
        .ok_or_else(|| "LLVM coverage line number is invalid".to_owned())?;
    let hits = fields
        .next()
        .ok_or_else(|| "LLVM coverage line record is invalid".to_owned())?
        .parse::<u64>()
        .map_err(|_| "LLVM coverage hit count is invalid".to_owned())?;
    if fields.next().is_some() {
        return Err("LLVM coverage line record is invalid".to_owned());
    }
    Ok((line, hits))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use super::{CoverageMap, PerTestCoverageMap, TestIdentity, TestTarget, parse_lcov_text};

    #[test]
    fn coverage_map_keeps_only_hit_lines() {
        let root = temporary_root("hit-lines");
        let source = root.join("src/lib.rs");
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        fs::write(&source, "one\ntwo\nthree\n").unwrap();
        let source = fs::canonicalize(source).unwrap();
        let profile = parse_lcov_text(
            "SF:src/lib.rs\nDA:1,2\nDA:2,0\nDA:3,1\nend_of_record\n",
            &root,
        )
        .unwrap();

        let mut coverage = CoverageMap::default();
        coverage.add(profile);
        assert!(coverage.covers(&source, 1));
        assert!(!coverage.covers(&source, 2));
        assert!(coverage.covers(&source, 3));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn per_test_coverage_sorts_and_merges_test_names() {
        let root = temporary_root("per-test");
        let source = root.join("src/lib.rs");
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        fs::write(&source, "one\n").unwrap();
        let source = fs::canonicalize(source).unwrap();
        let profile = || parse_lcov_text("SF:src/lib.rs\nDA:1,1\nend_of_record\n", &root).unwrap();

        let mut coverage = PerTestCoverageMap::default();
        coverage.add(profile(), &test("zebra"));
        coverage.add(profile(), &test("alpha"));
        coverage.add(profile(), &test("zebra"));
        assert_eq!(
            coverage.tests_for(&source, 1),
            Some(vec![test("alpha"), test("zebra")])
        );

        let _ = fs::remove_dir_all(root);
    }

    fn test(name: &str) -> TestIdentity {
        TestIdentity {
            package: "fixture".to_owned(),
            target: TestTarget::Integration("coverage".to_owned()),
            name: name.to_owned(),
        }
    }

    #[test]
    fn malformed_lcov_data_fails() {
        let root = temporary_root("invalid");
        let source = root.join("src/lib.rs");
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        fs::write(&source, "one\n").unwrap();
        for data in [
            "DA:1,1\nend_of_record\n",
            "SF:src/lib.rs\nDA:0,1\nend_of_record\n",
            "SF:src/lib.rs\nDA:1,one\nend_of_record\n",
            "SF:src/lib.rs\nDA:1,1\n",
        ] {
            assert!(parse_lcov_text(data, &root).is_err(), "{data}");
        }
        assert!(parse_lcov_text("SF:missing.rs\nDA:1,1\nend_of_record\n", &root).is_err());

        let _ = fs::remove_dir_all(root);
    }

    fn temporary_root(name: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "mutarust-coverage-test-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        root
    }

    #[test]
    fn empty_report_is_valid_and_covers_no_source() {
        let root = temporary_root("empty");
        let source = root.join("src/lib.rs");
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        fs::write(&source, "one\n").unwrap();
        let profile = parse_lcov_text("", Path::new(&root)).unwrap();
        let mut coverage = CoverageMap::default();
        coverage.add(profile);
        assert!(!coverage.covers(&source, 1));
        let _ = fs::remove_dir_all(root);
    }
}
