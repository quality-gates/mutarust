use cargo_metadata::{Metadata, MetadataCommand, Package, TargetKind};
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
        collect_target(target, &mut files)?;
    }

    Ok(files.into_iter().collect())
}

fn default_targets(targets: &[String]) -> Vec<Target> {
    if targets.is_empty() {
        vec![Target::recursive(".".into())]
    } else {
        targets.iter().map(|target| Target::parse(target)).collect()
    }
}

fn collect_target(target: Target, files: &mut BTreeSet<PathBuf>) -> Result<(), SourceError> {
    let path = PathBuf::from(&target.value);

    if path.is_file() || is_path_target(&target.value) {
        return collect_path(&path, target.recursive, files);
    }

    if collect_package(&target, files)? {
        return Ok(());
    }

    if path.exists() {
        return collect_path(&path, target.recursive, files);
    }

    Err(SourceError::new(format!(
        "cannot find Cargo package: {}",
        target.value
    )))
}

fn is_path_target(value: &str) -> bool {
    Path::new(value).is_absolute()
        || value == "."
        || value == ".."
        || value.starts_with("./")
        || value.starts_with(".\\")
        || value.starts_with("../")
        || value.starts_with("..\\")
}

fn collect_path(
    path: &Path,
    recursive: bool,
    files: &mut BTreeSet<PathBuf>,
) -> Result<(), SourceError> {
    let path = canonical_path(path)?;

    if path.is_file() {
        add_direct_source_file(&path, files);
        return Ok(());
    }

    if path.is_dir() {
        if let Some(metadata) = workspace_metadata(&path) {
            return collect_workspace(&metadata, recursive, files);
        }

        return collect_directory(&path, recursive, files);
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

fn collect_directory(
    directory: &Path,
    recursive: bool,
    files: &mut BTreeSet<PathBuf>,
) -> Result<(), SourceError> {
    if directory.file_name().is_some_and(is_test_directory) {
        return Ok(());
    }

    let excluded_sources = non_production_source_paths(directory);
    collect_directory_from_root(directory, directory, recursive, files, &excluded_sources)
}

fn collect_directory_from_root(
    directory: &Path,
    source_root: &Path,
    recursive: bool,
    files: &mut BTreeSet<PathBuf>,
    excluded_sources: &BTreeSet<PathBuf>,
) -> Result<(), SourceError> {
    for entry in fs::read_dir(directory).map_err(|error| read_error(directory, error))? {
        let entry = entry.map_err(|error| read_error(directory, error))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| read_error(&path, error))?;

        if file_type.is_file() && !is_hidden(entry.file_name().as_ref()) {
            add_source_file_from_root(&path, source_root, files, excluded_sources);
        } else if recursive && file_type.is_dir() && !skip_directory(entry.file_name().as_ref()) {
            collect_directory_from_root(&path, source_root, true, files, excluded_sources)?;
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

fn add_direct_source_file(path: &Path, files: &mut BTreeSet<PathBuf>) {
    let source_root =
        cargo_root(path).unwrap_or_else(|| path.parent().unwrap_or(path).to_path_buf());
    add_source_file_from_root(path, &source_root, files, &BTreeSet::new());
}

fn cargo_root(path: &Path) -> Option<PathBuf> {
    path.ancestors()
        .skip(1)
        .find(|candidate| candidate.join("Cargo.toml").is_file())
        .map(Path::to_path_buf)
}

fn non_production_source_paths(path: &Path) -> BTreeSet<PathBuf> {
    let Some(cargo_root) = cargo_root(path) else {
        return BTreeSet::new();
    };
    let Ok(metadata) = MetadataCommand::new().current_dir(cargo_root).exec() else {
        return BTreeSet::new();
    };

    metadata
        .packages
        .iter()
        .filter(|package| metadata.workspace_members.contains(&package.id))
        .flat_map(|package| &package.targets)
        .filter(|target| !is_production_target(&target.kind))
        .filter_map(|target| canonical_path(target.src_path.as_std_path()).ok())
        .collect()
}

fn add_source_file_from_root(
    path: &Path,
    source_root: &Path,
    files: &mut BTreeSet<PathBuf>,
    excluded_sources: &BTreeSet<PathBuf>,
) {
    if is_source_file(path)
        && !has_test_parent_below(path, source_root)
        && !excluded_sources.contains(path)
    {
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
}

fn has_test_parent_below(path: &Path, source_root: &Path) -> bool {
    path.strip_prefix(source_root)
        .ok()
        .and_then(Path::parent)
        .into_iter()
        .flat_map(Path::ancestors)
        .filter_map(Path::file_name)
        .any(is_test_directory)
}

fn skip_directory(name: &std::ffi::OsStr) -> bool {
    is_hidden(name)
        || matches!(
            name.to_string_lossy().as_ref(),
            "target"
                | "tests"
                | "benches"
                | "examples"
                | "fixtures"
                | "testdata"
                | "vendor"
                | "generated"
                | "gen"
        )
}

fn is_hidden(name: &std::ffi::OsStr) -> bool {
    name.to_string_lossy().starts_with('.')
}

fn is_test_directory(name: &std::ffi::OsStr) -> bool {
    matches!(
        name.to_string_lossy().as_ref(),
        "tests" | "benches" | "examples" | "fixtures" | "testdata"
    )
}

fn collect_package(target: &Target, files: &mut BTreeSet<PathBuf>) -> Result<bool, SourceError> {
    let metadata = MetadataCommand::new()
        .exec()
        .map_err(|error| SourceError::new(format!("cannot read Cargo metadata: {error}")))?;
    let Some(package) = metadata.packages.iter().find(|package| {
        package.name.as_ref() == target.value && metadata.workspace_members.contains(&package.id)
    }) else {
        return Ok(false);
    };
    collect_package_sources(package, target.recursive, files)?;
    Ok(true)
}

fn workspace_metadata(path: &Path) -> Option<Metadata> {
    if !path.join("Cargo.toml").is_file() {
        return None;
    }

    let metadata = MetadataCommand::new().current_dir(path).exec().ok()?;
    let workspace_root = canonical_path(metadata.workspace_root.as_std_path()).ok()?;

    (workspace_root == path).then_some(metadata)
}

fn collect_workspace(
    metadata: &Metadata,
    recursive: bool,
    files: &mut BTreeSet<PathBuf>,
) -> Result<(), SourceError> {
    for package in &metadata.packages {
        if metadata.workspace_members.contains(&package.id) {
            collect_package_sources(package, recursive, files)?;
        }
    }

    Ok(())
}

fn collect_package_sources(
    package: &Package,
    recursive: bool,
    files: &mut BTreeSet<PathBuf>,
) -> Result<(), SourceError> {
    let excluded_sources = non_production_package_sources(package);

    for target in &package.targets {
        if is_production_target(&target.kind) {
            collect_declared_target(
                target.src_path.as_std_path(),
                recursive,
                files,
                &excluded_sources,
            )?;
        }
    }

    Ok(())
}

fn non_production_package_sources(package: &Package) -> BTreeSet<PathBuf> {
    package
        .targets
        .iter()
        .filter(|target| !is_production_target(&target.kind))
        .filter_map(|target| canonical_path(target.src_path.as_std_path()).ok())
        .collect()
}

fn is_production_target(kinds: &[TargetKind]) -> bool {
    kinds.iter().any(|kind| {
        matches!(
            kind,
            TargetKind::Bin
                | TargetKind::CDyLib
                | TargetKind::DyLib
                | TargetKind::Lib
                | TargetKind::ProcMacro
                | TargetKind::RLib
                | TargetKind::StaticLib
        )
    })
}

fn collect_declared_target(
    source: &Path,
    recursive: bool,
    files: &mut BTreeSet<PathBuf>,
    excluded_sources: &BTreeSet<PathBuf>,
) -> Result<(), SourceError> {
    let source = canonical_path(source)?;
    add_declared_source_file(&source, files);

    let Some(directory) = source.parent() else {
        return Ok(());
    };

    collect_directory_from_root(directory, directory, recursive, files, excluded_sources)
}

fn add_declared_source_file(path: &Path, files: &mut BTreeSet<PathBuf>) {
    if is_source_file(path) {
        files.insert(path.to_path_buf());
    }
}

impl SourceError {
    fn new(message: String) -> Self {
        Self { message }
    }
}

struct Target {
    value: String,
    recursive: bool,
}

impl Target {
    fn parse(value: &str) -> Self {
        match value.strip_suffix("...") {
            Some(prefix) => Self::recursive(prefix.trim_end_matches(['/', '\\']).into()),
            None => Self::direct(value.to_owned()),
        }
    }

    fn direct(value: String) -> Self {
        Self {
            value,
            recursive: false,
        }
    }

    fn recursive(value: String) -> Self {
        Self {
            value: if value.is_empty() { ".".into() } else { value },
            recursive: true,
        }
    }
}
