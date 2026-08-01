use cargo_metadata::MetadataCommand;
use std::collections::BTreeSet;
use std::error::Error;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

/// An error that prevents Rust source discovery.
#[derive(Debug)]
pub struct SourceError {
    message: String,
}

impl fmt::Display for SourceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for SourceError {}

/// Finds unique Rust production-source candidates for the requested targets.
///
/// A target can be an existing Rust file, an existing directory, or the name
/// of a package in the current Cargo workspace. An empty target list selects
/// the current directory. The returned paths are absolute and sorted.
pub fn find_rust_sources(targets: &[String]) -> Result<Vec<PathBuf>, SourceError> {
    let requested = default_targets(targets);
    let mut files = BTreeSet::new();

    for target in requested {
        collect_target(&target, &mut files)?;
    }

    Ok(files.into_iter().collect())
}

fn default_targets(targets: &[String]) -> Vec<String> {
    if targets.is_empty() {
        vec![String::from(".")]
    } else {
        targets.to_vec()
    }
}

fn collect_target(target: &str, files: &mut BTreeSet<PathBuf>) -> Result<(), SourceError> {
    let path = recursive_path(target);

    if path.exists() {
        return collect_path(&path, files);
    }

    collect_package(target, files)
}

fn recursive_path(target: &str) -> PathBuf {
    match target.strip_suffix("...") {
        Some("") => PathBuf::from("."),
        Some(prefix) => PathBuf::from(prefix),
        None => PathBuf::from(target),
    }
}

fn collect_path(path: &Path, files: &mut BTreeSet<PathBuf>) -> Result<(), SourceError> {
    let path = canonical_path(path)?;

    if path.is_file() {
        add_source_file(&path, files);
        return Ok(());
    }

    if path.is_dir() {
        return collect_directory(&path, files);
    }

    Err(SourceError::new(format!(
        "source target is not a file or directory: {}",
        path.display()
    )))
}

fn canonical_path(path: &Path) -> Result<PathBuf, SourceError> {
    fs::canonicalize(path).map_err(|error| {
        SourceError::new(format!(
            "cannot read source target {}: {error}",
            path.display()
        ))
    })
}

fn collect_directory(directory: &Path, files: &mut BTreeSet<PathBuf>) -> Result<(), SourceError> {
    for entry in fs::read_dir(directory).map_err(|error| read_error(directory, error))? {
        let entry = entry.map_err(|error| read_error(directory, error))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| read_error(&path, error))?;

        if file_type.is_file() {
            add_source_file(&path, files);
        } else if file_type.is_dir() && !skip_directory(entry.file_name().as_ref()) {
            collect_directory(&path, files)?;
        }
    }

    Ok(())
}

fn read_error(path: &Path, error: std::io::Error) -> SourceError {
    SourceError::new(format!(
        "cannot read source target {}: {error}",
        path.display()
    ))
}

fn add_source_file(path: &Path, files: &mut BTreeSet<PathBuf>) {
    if is_source_file(path) {
        files.insert(path.to_path_buf());
    }
}

fn is_source_file(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
        return false;
    };

    path.extension().is_some_and(|extension| extension == "rs")
        && name != "build.rs"
        && !name.ends_with("_test.rs")
        && !has_ignored_parent(path)
}

fn has_ignored_parent(path: &Path) -> bool {
    path.ancestors()
        .skip(1)
        .filter_map(Path::file_name)
        .any(skip_directory)
}

fn skip_directory(name: &std::ffi::OsStr) -> bool {
    let name = name.to_string_lossy();
    name.starts_with('.')
        || matches!(
            name.as_ref(),
            "target" | "tests" | "benches" | "examples" | "fixtures" | "testdata"
        )
}

fn collect_package(target: &str, files: &mut BTreeSet<PathBuf>) -> Result<(), SourceError> {
    let metadata = MetadataCommand::new()
        .exec()
        .map_err(|error| SourceError::new(format!("cannot read Cargo metadata: {error}")))?;
    let package = metadata
        .packages
        .iter()
        .find(|package| {
            package.name.as_ref() == target && metadata.workspace_members.contains(&package.id)
        })
        .ok_or_else(|| SourceError::new(format!("cannot find Cargo package: {target}")))?;
    let manifest_directory = package
        .manifest_path
        .parent()
        .ok_or_else(|| SourceError::new(format!("cannot read package manifest: {target}")))?;

    collect_directory(manifest_directory.as_std_path(), files)
}

impl SourceError {
    fn new(message: String) -> Self {
        Self { message }
    }
}
