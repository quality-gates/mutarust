use cargo_metadata::{Metadata, MetadataCommand, Package, TargetKind};
use std::collections::BTreeSet;
use std::error::Error;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use syn::{Expr, Item, ItemMacro, ItemMod, Lit, Meta};

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

    match collect_package(&target, files) {
        Ok(true) => return Ok(()),
        Ok(false) | Err(_) if path.exists() => return collect_path(&path, target.recursive, files),
        Ok(false) => {}
        Err(error) => return Err(error),
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
        add_explicit_source_file(&path, files);
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
    let excluded_sources = directory
        .file_name()
        .is_some_and(is_test_directory)
        .then(BTreeSet::new)
        .unwrap_or_else(|| non_production_source_paths(directory));
    let accept_all_rust = directory.file_name().is_some_and(is_test_directory);
    collect_directory_from_root(
        directory,
        directory,
        recursive,
        files,
        &excluded_sources,
        accept_all_rust,
    )?;

    if let Some(package) = package_at_directory(directory) {
        collect_package_sources(&package, recursive, files)?;
    }

    Ok(())
}

fn package_at_directory(directory: &Path) -> Option<Package> {
    let metadata = MetadataCommand::new().current_dir(directory).exec().ok()?;
    metadata
        .packages
        .into_iter()
        .filter(|package| metadata.workspace_members.contains(&package.id))
        .find(|package| package_directory(package).is_some_and(|path| path == directory))
}

fn package_directory(package: &Package) -> Option<PathBuf> {
    package
        .manifest_path
        .as_std_path()
        .parent()
        .and_then(|path| canonical_path(path).ok())
}

fn collect_directory_from_root(
    directory: &Path,
    source_root: &Path,
    recursive: bool,
    files: &mut BTreeSet<PathBuf>,
    excluded_sources: &BTreeSet<PathBuf>,
    accept_all_rust: bool,
) -> Result<(), SourceError> {
    for entry in fs::read_dir(directory).map_err(|error| read_error(directory, error))? {
        let entry = entry.map_err(|error| read_error(directory, error))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| read_error(&path, error))?;

        if file_type.is_file() && !is_hidden(entry.file_name().as_ref()) {
            add_source_file_from_root(&path, source_root, files, excluded_sources, accept_all_rust);
        } else if recursive
            && file_type.is_dir()
            && (accept_all_rust || !skip_directory(entry.file_name().as_ref()))
        {
            collect_directory_from_root(
                &path,
                source_root,
                true,
                files,
                excluded_sources,
                accept_all_rust,
            )?;
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

fn add_explicit_source_file(path: &Path, files: &mut BTreeSet<PathBuf>) {
    if path.extension().is_some_and(|extension| extension == "rs") {
        files.insert(path.to_path_buf());
    }
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
        .flat_map(|target| source_tree(target.src_path.as_std_path()))
        .chain(
            metadata
                .packages
                .iter()
                .filter(|package| metadata.workspace_members.contains(&package.id))
                .flat_map(test_only_package_sources),
        )
        .collect()
}

fn source_tree(source: &Path) -> BTreeSet<PathBuf> {
    let mut sources = BTreeSet::new();
    let Ok(source) = canonical_path(source) else {
        return sources;
    };
    let Some(directory) = source.parent() else {
        return sources;
    };

    collect_source_tree(&source, directory, &mut sources);
    sources
}

fn collect_source_tree(source: &Path, directory: &Path, sources: &mut BTreeSet<PathBuf>) {
    if !sources.insert(source.to_path_buf()) {
        return;
    }
    let Ok(text) = fs::read_to_string(source) else {
        return;
    };
    let Ok(syntax) = syn::parse_file(&text) else {
        return;
    };

    for item in &syntax.items {
        collect_item_source_tree(item, directory, sources);
    }
}

fn collect_item_source_tree(item: &Item, directory: &Path, sources: &mut BTreeSet<PathBuf>) {
    if let Item::Macro(item) = item {
        collect_include_source(item, directory, sources);
        return;
    }
    let Item::Mod(module) = item else {
        return;
    };

    if let Some((_, items)) = &module.content {
        let nested_directory = directory.join(module.ident.to_string());
        for item in items {
            collect_item_source_tree(item, &nested_directory, sources);
        }
        return;
    }

    let Some(source) = external_module_source(module, directory) else {
        return;
    };
    let module_directory = module_directory(&source);
    collect_source_tree(&source, &module_directory, sources);
}

fn collect_include_source(item: &ItemMacro, directory: &Path, sources: &mut BTreeSet<PathBuf>) {
    let Some(source) = include_source(item, directory) else {
        return;
    };

    collect_source_tree(&source, directory, sources);
}

fn include_source(item: &ItemMacro, directory: &Path) -> Option<PathBuf> {
    item.mac
        .path
        .is_ident("include")
        .then(|| syn::parse2::<syn::LitStr>(item.mac.tokens.clone()).ok())
        .flatten()
        .map(|path| directory.join(path.value()))
        .and_then(|path| canonical_path(&path).ok())
}

fn test_only_source_tree(source: &Path) -> BTreeSet<PathBuf> {
    let mut sources = BTreeSet::new();
    let Ok(source) = canonical_path(source) else {
        return sources;
    };
    let Some(directory) = source.parent() else {
        return sources;
    };

    collect_test_only_source_tree(&source, directory, &mut sources);
    sources
}

fn collect_test_only_source_tree(source: &Path, directory: &Path, sources: &mut BTreeSet<PathBuf>) {
    let Ok(text) = fs::read_to_string(source) else {
        return;
    };
    let Ok(syntax) = syn::parse_file(&text) else {
        return;
    };

    for item in &syntax.items {
        collect_test_only_item_source_tree(item, directory, sources);
    }
}

fn collect_test_only_item_source_tree(
    item: &Item,
    directory: &Path,
    sources: &mut BTreeSet<PathBuf>,
) {
    let Item::Mod(module) = item else {
        return;
    };

    if let Some((_, items)) = &module.content {
        let nested_directory = directory.join(module.ident.to_string());
        if has_test_configuration(module) {
            for item in items {
                collect_item_source_tree(item, &nested_directory, sources);
            }
        } else {
            for item in items {
                collect_test_only_item_source_tree(item, &nested_directory, sources);
            }
        }
        return;
    }

    let Some(source) = external_module_source(module, directory) else {
        return;
    };
    let module_directory = module_directory(&source);
    if has_test_configuration(module) {
        collect_source_tree(&source, &module_directory, sources);
    } else {
        collect_test_only_source_tree(&source, &module_directory, sources);
    }
}

fn has_test_configuration(module: &ItemMod) -> bool {
    module
        .attrs
        .iter()
        .filter(|attribute| attribute.path().is_ident("cfg"))
        .filter_map(|attribute| match &attribute.meta {
            Meta::List(list) => syn::parse2::<Meta>(list.tokens.clone()).ok(),
            _ => None,
        })
        .any(configuration_requires_test)
}

fn configuration_requires_test(configuration: Meta) -> bool {
    if configuration.path().is_ident("test") {
        return true;
    }
    let Meta::List(list) = configuration else {
        return false;
    };
    let Ok(options) =
        list.parse_args_with(syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated)
    else {
        return false;
    };

    if list.path.is_ident("all") {
        options.into_iter().any(configuration_requires_test)
    } else if list.path.is_ident("any") {
        !options.is_empty() && options.into_iter().all(configuration_requires_test)
    } else {
        false
    }
}

fn external_module_source(module: &ItemMod, directory: &Path) -> Option<PathBuf> {
    module_path_attribute(module)
        .map(|path| directory.join(path))
        .or_else(|| module_source_by_name(module, directory))
        .and_then(|path| canonical_path(&path).ok())
}

fn module_path_attribute(module: &ItemMod) -> Option<PathBuf> {
    module.attrs.iter().find_map(|attribute| {
        let Meta::NameValue(value) = &attribute.meta else {
            return None;
        };
        let Expr::Lit(literal) = &value.value else {
            return None;
        };
        let Lit::Str(path) = &literal.lit else {
            return None;
        };

        attribute
            .path()
            .is_ident("path")
            .then(|| PathBuf::from(path.value()))
    })
}

fn module_source_by_name(module: &ItemMod, directory: &Path) -> Option<PathBuf> {
    let name = module.ident.to_string();
    [
        directory.join(format!("{name}.rs")),
        directory.join(name).join("mod.rs"),
    ]
    .into_iter()
    .find(|path| path.is_file())
}

fn module_directory(source: &Path) -> PathBuf {
    let parent = source.parent().unwrap_or(source);
    if source.file_stem().is_some_and(|name| name == "mod") {
        parent.to_path_buf()
    } else {
        parent.join(source.file_stem().unwrap_or_default())
    }
}

fn add_source_file_from_root(
    path: &Path,
    source_root: &Path,
    files: &mut BTreeSet<PathBuf>,
    excluded_sources: &BTreeSet<PathBuf>,
    accept_all_rust: bool,
) {
    if (accept_all_rust && is_rust_file(path) || is_source_file(path))
        && !has_test_parent_below(path, source_root)
        && !excluded_sources.contains(path)
    {
        files.insert(path.to_path_buf());
    }
}

fn is_rust_file(path: &Path) -> bool {
    path.extension().is_some_and(|extension| extension == "rs")
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
        .flat_map(|target| source_tree(target.src_path.as_std_path()))
        .chain(test_only_package_sources(package))
        .collect()
}

fn test_only_package_sources(package: &Package) -> BTreeSet<PathBuf> {
    package
        .targets
        .iter()
        .filter(|target| is_production_target(&target.kind))
        .flat_map(|target| test_only_source_tree(target.src_path.as_std_path()))
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

    collect_directory_from_root(
        directory,
        directory,
        recursive,
        files,
        excluded_sources,
        false,
    )
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
