use cargo_metadata::{Metadata, MetadataCommand, Package, TargetKind};
use std::collections::BTreeSet;
use std::error::Error;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
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
    let include_excluded_sources = is_excluded_directory_target(directory);
    let excluded_sources = include_excluded_sources
        .then(BTreeSet::new)
        .unwrap_or_else(|| non_production_source_paths(directory));
    collect_directory_from_root(
        directory,
        directory,
        recursive,
        files,
        &excluded_sources,
        include_excluded_sources,
        None,
    )?;

    if let Some((package, active_features)) = package_at_directory(directory) {
        collect_package_sources(&package, &active_features, recursive, files)?;
    }

    Ok(())
}

fn is_excluded_directory_target(directory: &Path) -> bool {
    directory
        .ancestors()
        .filter_map(Path::file_name)
        .any(is_excluded_source_directory)
}

fn is_excluded_source_directory(name: &std::ffi::OsStr) -> bool {
    is_test_directory(name)
        || matches!(
            name.to_string_lossy().as_ref(),
            "vendor" | "generated" | "gen"
        )
}

fn package_at_directory(directory: &Path) -> Option<(Package, BTreeSet<String>)> {
    let metadata = MetadataCommand::new().current_dir(directory).exec().ok()?;
    let package = metadata
        .packages
        .iter()
        .filter(|package| metadata.workspace_members.contains(&package.id))
        .find(|package| package_directory(package).is_some_and(|path| path == directory))?
        .clone();
    let active_features = active_package_features(&metadata, &package);

    Some((package, active_features))
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
    package_root: Option<&Path>,
) -> Result<(), SourceError> {
    for entry in fs::read_dir(directory).map_err(|error| read_error(directory, error))? {
        let entry = entry.map_err(|error| read_error(directory, error))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| read_error(&path, error))?;

        if file_type.is_file() && !is_hidden(entry.file_name().as_ref()) {
            add_source_file_from_root(
                &path,
                source_root,
                files,
                excluded_sources,
                accept_all_rust,
                package_root,
            );
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
                package_root,
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
        .flat_map(|package| {
            non_production_package_sources(package, &active_package_features(&metadata, package))
        })
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

fn inactive_source_tree(source: &Path, active_features: &BTreeSet<String>) -> BTreeSet<PathBuf> {
    let mut sources = BTreeSet::new();
    let Ok(source) = canonical_path(source) else {
        return sources;
    };
    let Some(directory) = source.parent() else {
        return sources;
    };

    collect_inactive_source_tree(&source, directory, active_features, &mut sources);
    sources
}

fn collect_inactive_source_tree(
    source: &Path,
    directory: &Path,
    active_features: &BTreeSet<String>,
    sources: &mut BTreeSet<PathBuf>,
) {
    let Ok(text) = fs::read_to_string(source) else {
        return;
    };
    let Ok(syntax) = syn::parse_file(&text) else {
        return;
    };

    let source_directory = source.parent().unwrap_or(directory);
    for item in &syntax.items {
        collect_inactive_item_source_tree(
            item,
            directory,
            source_directory,
            source_directory,
            active_features,
            sources,
        );
    }
}

fn collect_inactive_item_source_tree(
    item: &Item,
    module_root: &Path,
    source_directory: &Path,
    path_directory: &Path,
    active_features: &BTreeSet<String>,
    sources: &mut BTreeSet<PathBuf>,
) {
    if let Item::Macro(item) = item {
        if !configuration_is_active(&item.attrs, false, active_features) {
            collect_include_source(item, module_root, source_directory, sources);
        }
        return;
    }
    let Item::Mod(module) = item else {
        return;
    };

    if !configuration_is_active(&module.attrs, false, active_features) {
        collect_module_source_candidates(module, module_root, path_directory, sources);
        if let Some((_, items)) = &module.content {
            let nested_directory = module_root.join(module.ident.to_string());
            for item in items {
                collect_item_source_tree(
                    item,
                    &nested_directory,
                    source_directory,
                    &nested_directory,
                    sources,
                );
            }
        }
        return;
    }

    collect_inactive_cfg_attr_sources(module, module_root, path_directory, sources);

    if let Some((_, items)) = &module.content {
        let nested_directory = module_root.join(module.ident.to_string());
        for item in items {
            collect_inactive_item_source_tree(
                item,
                &nested_directory,
                source_directory,
                &nested_directory,
                active_features,
                sources,
            );
        }
        return;
    }

    let Some(source) =
        active_production_module_source(module, module_root, path_directory, active_features)
    else {
        return;
    };
    let module_directory = module_directory(&source);
    collect_inactive_source_tree(&source, &module_directory, active_features, sources);
}

fn collect_inactive_cfg_attr_sources(
    module: &ItemMod,
    module_root: &Path,
    path_directory: &Path,
    sources: &mut BTreeSet<PathBuf>,
) {
    if cfg_attr_paths(module).is_empty() {
        return;
    }
    collect_module_source_candidates(module, module_root, path_directory, sources);
}

fn collect_module_source_candidates(
    module: &ItemMod,
    module_root: &Path,
    path_directory: &Path,
    sources: &mut BTreeSet<PathBuf>,
) {
    if let Some(source) = external_module_source(module, module_root, path_directory) {
        let module_directory = module_directory(&source);
        collect_source_tree(&source, &module_directory, sources);
    }
    for path in cfg_attr_paths(module) {
        let source = path_directory.join(path);
        let Ok(source) = canonical_path(&source) else {
            continue;
        };
        let module_directory = module_directory(&source);
        collect_source_tree(&source, &module_directory, sources);
    }
}

fn cfg_attr_paths(module: &ItemMod) -> Vec<PathBuf> {
    module
        .attrs
        .iter()
        .filter(|attribute| attribute.path().is_ident("cfg_attr"))
        .filter_map(|attribute| match &attribute.meta {
            Meta::List(list) => list
                .parse_args_with(
                    syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated,
                )
                .ok(),
            _ => None,
        })
        .filter_map(|options| options.into_iter().skip(1).find_map(path_from_meta))
        .collect()
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

    let source_directory = source.parent().unwrap_or(directory);
    for item in &syntax.items {
        collect_item_source_tree(item, directory, source_directory, source_directory, sources);
    }
}

fn collect_item_source_tree(
    item: &Item,
    module_root: &Path,
    source_directory: &Path,
    path_directory: &Path,
    sources: &mut BTreeSet<PathBuf>,
) {
    if let Item::Macro(item) = item {
        collect_include_source(item, module_root, source_directory, sources);
        return;
    }
    let Item::Mod(module) = item else {
        return;
    };

    if let Some((_, items)) = &module.content {
        let nested_directory = module_root.join(module.ident.to_string());
        for item in items {
            collect_item_source_tree(
                item,
                &nested_directory,
                source_directory,
                &nested_directory,
                sources,
            );
        }
        return;
    }

    let Some(source) = external_module_source(module, module_root, path_directory) else {
        return;
    };
    let module_directory = module_directory(&source);
    collect_source_tree(&source, &module_directory, sources);
}

fn collect_include_source(
    item: &ItemMacro,
    module_root: &Path,
    source_directory: &Path,
    sources: &mut BTreeSet<PathBuf>,
) {
    let Some(source) = include_source(item, source_directory) else {
        return;
    };

    collect_source_tree(&source, module_root, sources);
}

fn production_source_tree(source: &Path, active_features: &BTreeSet<String>) -> BTreeSet<PathBuf> {
    let mut sources = BTreeSet::new();
    let Ok(source) = canonical_path(source) else {
        return sources;
    };
    let Some(directory) = source.parent() else {
        return sources;
    };

    collect_production_source_tree(&source, directory, active_features, &mut sources);
    sources
}

fn collect_production_source_tree(
    source: &Path,
    directory: &Path,
    active_features: &BTreeSet<String>,
    sources: &mut BTreeSet<PathBuf>,
) {
    if !sources.insert(source.to_path_buf()) {
        return;
    }
    let Ok(text) = fs::read_to_string(source) else {
        return;
    };
    let Ok(syntax) = syn::parse_file(&text) else {
        return;
    };

    let source_directory = source.parent().unwrap_or(directory);
    for item in &syntax.items {
        collect_production_item_source_tree(
            item,
            directory,
            source_directory,
            source_directory,
            active_features,
            sources,
        );
    }
}

fn collect_production_item_source_tree(
    item: &Item,
    module_root: &Path,
    source_directory: &Path,
    path_directory: &Path,
    active_features: &BTreeSet<String>,
    sources: &mut BTreeSet<PathBuf>,
) {
    if let Item::Macro(item) = item {
        collect_production_include_source(
            item,
            module_root,
            source_directory,
            active_features,
            sources,
        );
        return;
    }
    let Item::Mod(module) = item else {
        return;
    };
    if !configuration_is_active(&module.attrs, false, active_features) {
        return;
    }

    if let Some((_, items)) = &module.content {
        let nested_directory = module_root.join(module.ident.to_string());
        for item in items {
            collect_production_item_source_tree(
                item,
                &nested_directory,
                source_directory,
                &nested_directory,
                active_features,
                sources,
            );
        }
        return;
    }

    let Some(source) =
        active_production_module_source(module, module_root, path_directory, active_features)
    else {
        return;
    };
    let module_directory = module_directory(&source);
    collect_production_source_tree(&source, &module_directory, active_features, sources);
}

fn collect_production_include_source(
    item: &ItemMacro,
    module_root: &Path,
    source_directory: &Path,
    active_features: &BTreeSet<String>,
    sources: &mut BTreeSet<PathBuf>,
) {
    if !configuration_is_active(&item.attrs, false, active_features) {
        return;
    }
    let Some(source) = include_source(item, source_directory) else {
        return;
    };

    collect_production_source_tree(&source, module_root, active_features, sources);
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

    let source_directory = source.parent().unwrap_or(directory);
    for item in &syntax.items {
        collect_test_only_item_source_tree(
            item,
            directory,
            source_directory,
            source_directory,
            sources,
        );
    }
}

fn collect_test_only_item_source_tree(
    item: &Item,
    module_root: &Path,
    source_directory: &Path,
    path_directory: &Path,
    sources: &mut BTreeSet<PathBuf>,
) {
    if let Item::Macro(item) = item {
        collect_test_only_include_source(item, module_root, source_directory, sources);
        return;
    }
    let Item::Mod(module) = item else {
        return;
    };

    if let Some((_, items)) = &module.content {
        let nested_directory = module_root.join(module.ident.to_string());
        if has_test_configuration(&module.attrs) {
            for item in items {
                collect_item_source_tree(
                    item,
                    &nested_directory,
                    source_directory,
                    &nested_directory,
                    sources,
                );
            }
        } else {
            for item in items {
                collect_test_only_item_source_tree(
                    item,
                    &nested_directory,
                    source_directory,
                    &nested_directory,
                    sources,
                );
            }
        }
        return;
    }

    if has_test_configuration(&module.attrs) {
        let Some(source) = test_module_source(module, module_root, path_directory) else {
            return;
        };
        let module_directory = module_directory(&source);
        collect_source_tree(&source, &module_directory, sources);
    } else {
        if let Some(source) = test_path_module_source(module, path_directory) {
            let module_directory = module_directory(&source);
            collect_source_tree(&source, &module_directory, sources);
        }
        let Some(source) = production_module_source(module, module_root, path_directory) else {
            return;
        };
        let module_directory = module_directory(&source);
        collect_test_only_source_tree(&source, &module_directory, sources);
    }
}

fn collect_test_only_include_source(
    item: &ItemMacro,
    module_root: &Path,
    source_directory: &Path,
    sources: &mut BTreeSet<PathBuf>,
) {
    let Some(source) = include_source(item, source_directory) else {
        return;
    };

    if has_test_configuration(&item.attrs) {
        collect_source_tree(&source, module_root, sources);
    } else {
        collect_test_only_source_tree(&source, module_root, sources);
    }
}

fn has_test_configuration(attributes: &[syn::Attribute]) -> bool {
    attributes
        .iter()
        .filter(|attribute| attribute.path().is_ident("cfg"))
        .filter_map(|attribute| match &attribute.meta {
            Meta::List(list) => syn::parse2::<Meta>(list.tokens.clone()).ok(),
            _ => None,
        })
        .any(configuration_requires_test)
}

fn configuration_is_active(
    attributes: &[syn::Attribute],
    test_enabled: bool,
    active_features: &BTreeSet<String>,
) -> bool {
    let direct_configuration_is_active = attributes
        .iter()
        .filter(|attribute| attribute.path().is_ident("cfg"))
        .filter_map(|attribute| match &attribute.meta {
            Meta::List(list) => syn::parse2::<Meta>(list.tokens.clone()).ok(),
            _ => None,
        })
        .all(|configuration| {
            configuration_is_active_for(configuration, test_enabled, active_features)
        });
    let applied_configuration_is_active = attributes
        .iter()
        .filter(|attribute| attribute.path().is_ident("cfg_attr"))
        .flat_map(|attribute| {
            applied_cfg_conditions(attribute, test_enabled, active_features).into_iter()
        })
        .flatten()
        .all(|configuration| {
            configuration_is_active_for(configuration, test_enabled, active_features)
        });

    direct_configuration_is_active && applied_configuration_is_active
}

fn applied_cfg_conditions(
    attribute: &syn::Attribute,
    test_enabled: bool,
    active_features: &BTreeSet<String>,
) -> Option<Vec<Meta>> {
    let Meta::List(list) = &attribute.meta else {
        return None;
    };
    let options = list
        .parse_args_with(syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated)
        .ok()?;
    let mut options = options.into_iter();
    let condition = options.next()?;
    let cfg_attr_is_active = configuration_is_active_for(condition, test_enabled, active_features);

    cfg_attr_is_active.then(|| {
        options
            .filter_map(|option| match option {
                Meta::List(list) if list.path.is_ident("cfg") => syn::parse2(list.tokens).ok(),
                _ => None,
            })
            .collect()
    })
}

fn configuration_is_active_for(
    configuration: Meta,
    test_enabled: bool,
    active_features: &BTreeSet<String>,
) -> bool {
    if configuration.path().is_ident("test") {
        return test_enabled;
    }
    if let Meta::NameValue(value) = &configuration {
        if value.path.is_ident("feature") {
            return feature_name(value).is_some_and(|feature| active_features.contains(&feature));
        }
        return target_configuration_is_active(value);
    }
    let Meta::List(list) = configuration else {
        return target_flag_is_active(&list_or_path_name(&configuration));
    };
    let Ok(options) =
        list.parse_args_with(syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated)
    else {
        return true;
    };

    if list.path.is_ident("all") {
        options
            .into_iter()
            .all(|option| configuration_is_active_for(option, test_enabled, active_features))
    } else if list.path.is_ident("any") {
        options
            .into_iter()
            .any(|option| configuration_is_active_for(option, test_enabled, active_features))
    } else if list.path.is_ident("not") {
        !options
            .into_iter()
            .next()
            .is_none_or(|option| configuration_is_active_for(option, test_enabled, active_features))
    } else {
        false
    }
}

fn list_or_path_name(configuration: &Meta) -> String {
    configuration
        .path()
        .get_ident()
        .map_or_else(String::new, ToString::to_string)
}

fn target_flag_is_active(name: &str) -> bool {
    rustc_configurations().contains(name)
}

fn target_configuration_is_active(value: &syn::MetaNameValue) -> bool {
    let Some(configuration_value) = feature_name(value) else {
        return false;
    };
    let Some(configuration_name) = value.path.get_ident() else {
        return false;
    };

    rustc_configurations().contains(&format!("{configuration_name}=\"{configuration_value}\""))
}

fn rustc_configurations() -> &'static BTreeSet<String> {
    static CONFIGURATIONS: OnceLock<BTreeSet<String>> = OnceLock::new();

    CONFIGURATIONS.get_or_init(read_rustc_configurations)
}

fn read_rustc_configurations() -> BTreeSet<String> {
    let Ok(output) = Command::new("rustc").args(["--print", "cfg"]).output() else {
        return default_rustc_configurations();
    };
    if !output.status.success() {
        return default_rustc_configurations();
    }
    let Ok(configurations) = String::from_utf8(output.stdout) else {
        return default_rustc_configurations();
    };

    configurations.lines().map(str::to_owned).collect()
}

fn default_rustc_configurations() -> BTreeSet<String> {
    [
        ("target_os", std::env::consts::OS.to_owned()),
        ("target_arch", std::env::consts::ARCH.to_owned()),
        ("target_family", std::env::consts::FAMILY.to_owned()),
        ("target_pointer_width", usize::BITS.to_string()),
    ]
    .into_iter()
    .map(|(name, value)| format!("{name}=\"{value}\""))
    .chain(cfg!(unix).then_some("unix".to_owned()))
    .chain(cfg!(windows).then_some("windows".to_owned()))
    .collect()
}

fn feature_name(value: &syn::MetaNameValue) -> Option<String> {
    let Expr::Lit(literal) = &value.value else {
        return None;
    };
    let Lit::Str(name) = &literal.lit else {
        return None;
    };

    Some(name.value())
}

fn configuration_requires_test(configuration: Meta) -> bool {
    !configuration_can_be_true_without_test(configuration)
}

fn configuration_can_be_true_without_test(configuration: Meta) -> bool {
    if configuration.path().is_ident("test") {
        return false;
    }
    let Meta::List(list) = configuration else {
        return true;
    };
    let Ok(options) =
        list.parse_args_with(syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated)
    else {
        return true;
    };

    if list.path.is_ident("all") {
        options
            .into_iter()
            .all(configuration_can_be_true_without_test)
    } else if list.path.is_ident("any") {
        options
            .into_iter()
            .any(configuration_can_be_true_without_test)
    } else if list.path.is_ident("not") {
        options
            .into_iter()
            .next()
            .is_none_or(configuration_can_be_false_without_test)
    } else {
        true
    }
}

fn configuration_can_be_false_without_test(configuration: Meta) -> bool {
    if configuration.path().is_ident("test") {
        return true;
    }
    let Meta::List(list) = configuration else {
        return true;
    };
    let Ok(options) =
        list.parse_args_with(syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated)
    else {
        return true;
    };

    if list.path.is_ident("all") {
        options
            .into_iter()
            .any(configuration_can_be_false_without_test)
    } else if list.path.is_ident("any") {
        options
            .into_iter()
            .all(configuration_can_be_false_without_test)
    } else if list.path.is_ident("not") {
        options
            .into_iter()
            .next()
            .is_none_or(configuration_can_be_true_without_test)
    } else {
        true
    }
}

fn external_module_source(
    module: &ItemMod,
    module_directory: &Path,
    path_directory: &Path,
) -> Option<PathBuf> {
    module_path_attribute(module)
        .map(|path| path_directory.join(path))
        .or_else(|| module_source_by_name(module, module_directory))
        .and_then(|path| canonical_path(&path).ok())
}

fn test_module_source(
    module: &ItemMod,
    module_directory: &Path,
    path_directory: &Path,
) -> Option<PathBuf> {
    test_path_module_source(module, path_directory)
        .or_else(|| external_module_source(module, module_directory, path_directory))
}

fn production_module_source(
    module: &ItemMod,
    module_directory: &Path,
    path_directory: &Path,
) -> Option<PathBuf> {
    production_path_module_source(module, path_directory)
        .or_else(|| external_module_source(module, module_directory, path_directory))
}

fn active_production_module_source(
    module: &ItemMod,
    module_directory: &Path,
    path_directory: &Path,
    active_features: &BTreeSet<String>,
) -> Option<PathBuf> {
    active_production_path_module_source(module, path_directory, active_features)
        .or_else(|| external_module_source(module, module_directory, path_directory))
}

fn active_production_path_module_source(
    module: &ItemMod,
    directory: &Path,
    active_features: &BTreeSet<String>,
) -> Option<PathBuf> {
    active_production_path_attribute(module, active_features)
        .map(|path| directory.join(path))
        .and_then(|path| canonical_path(&path).ok())
}

fn production_path_module_source(module: &ItemMod, directory: &Path) -> Option<PathBuf> {
    production_path_attribute(module)
        .map(|path| directory.join(path))
        .and_then(|path| canonical_path(&path).ok())
}

fn test_path_module_source(module: &ItemMod, directory: &Path) -> Option<PathBuf> {
    test_path_attribute(module)
        .map(|path| directory.join(path))
        .and_then(|path| canonical_path(&path).ok())
}

fn test_path_attribute(module: &ItemMod) -> Option<PathBuf> {
    module.attrs.iter().find_map(|attribute| {
        let Meta::List(list) = &attribute.meta else {
            return None;
        };
        if !attribute.path().is_ident("cfg_attr") {
            return None;
        }
        let Ok(options) = list
            .parse_args_with(syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated)
        else {
            return None;
        };
        let mut options = options.into_iter();
        let configuration = options.next()?;
        configuration_requires_test(configuration)
            .then(|| options.find_map(path_from_meta))
            .flatten()
    })
}

fn production_path_attribute(module: &ItemMod) -> Option<PathBuf> {
    module.attrs.iter().find_map(|attribute| {
        let Meta::List(list) = &attribute.meta else {
            return None;
        };
        if !attribute.path().is_ident("cfg_attr") {
            return None;
        }
        let Ok(options) = list
            .parse_args_with(syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated)
        else {
            return None;
        };
        let mut options = options.into_iter();
        let configuration = options.next()?;
        configuration_can_be_true_without_test(configuration)
            .then(|| options.find_map(path_from_meta))
            .flatten()
    })
}

fn active_production_path_attribute(
    module: &ItemMod,
    active_features: &BTreeSet<String>,
) -> Option<PathBuf> {
    module.attrs.iter().find_map(|attribute| {
        let Meta::List(list) = &attribute.meta else {
            return None;
        };
        if !attribute.path().is_ident("cfg_attr") {
            return None;
        }
        let Ok(options) = list
            .parse_args_with(syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated)
        else {
            return None;
        };
        let mut options = options.into_iter();
        let configuration = options.next()?;
        configuration_is_active_for(configuration, false, active_features)
            .then(|| options.find_map(path_from_meta))
            .flatten()
    })
}

fn path_from_meta(meta: Meta) -> Option<PathBuf> {
    let Meta::NameValue(value) = meta else {
        return None;
    };
    let Expr::Lit(literal) = value.value else {
        return None;
    };
    let Lit::Str(path) = literal.lit else {
        return None;
    };

    value
        .path
        .is_ident("path")
        .then(|| PathBuf::from(path.value()))
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
    package_root: Option<&Path>,
) {
    if (accept_all_rust && is_rust_file(path) || is_source_file(path))
        && (accept_all_rust || !has_test_parent_at_or_below(path, source_root, package_root))
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

fn has_test_parent_at_or_below(
    path: &Path,
    source_root: &Path,
    package_root: Option<&Path>,
) -> bool {
    source_root_is_in_test_area(source_root, package_root)
        || path
            .strip_prefix(source_root)
            .ok()
            .and_then(Path::parent)
            .into_iter()
            .flat_map(Path::ancestors)
            .filter_map(Path::file_name)
            .any(is_test_directory)
}

fn source_root_is_in_test_area(source_root: &Path, package_root: Option<&Path>) -> bool {
    package_root
        .and_then(|root| source_root.strip_prefix(root).ok())
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
    let active_features = active_package_features(&metadata, package);
    collect_package_sources(package, &active_features, target.recursive, files)?;
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
            let active_features = active_package_features(metadata, package);
            collect_package_sources(package, &active_features, recursive, files)?;
        }
    }

    Ok(())
}

fn collect_package_sources(
    package: &Package,
    active_features: &BTreeSet<String>,
    recursive: bool,
    files: &mut BTreeSet<PathBuf>,
) -> Result<(), SourceError> {
    let excluded_sources = non_production_package_sources(package, active_features);

    for target in &package.targets {
        if is_production_target(&target.kind) && is_active_target(target, active_features) {
            for source in production_source_tree(target.src_path.as_std_path(), active_features) {
                add_declared_source_file(&source, files);
            }
            collect_declared_target(
                package,
                target.src_path.as_std_path(),
                recursive,
                files,
                &excluded_sources,
            )?;
        }
    }

    Ok(())
}

fn non_production_package_sources(
    package: &Package,
    active_features: &BTreeSet<String>,
) -> BTreeSet<PathBuf> {
    let test_only_sources = test_only_package_sources(package, active_features);
    let inactive_sources = inactive_package_sources(package, active_features);
    let production_sources = production_package_sources(package, active_features);
    let mut non_production_sources = package
        .targets
        .iter()
        .filter(|target| {
            !is_production_target(&target.kind) || !is_active_target(target, active_features)
        })
        .flat_map(|target| source_tree(target.src_path.as_std_path()))
        .chain(test_only_sources)
        .chain(inactive_sources)
        .collect::<BTreeSet<_>>();

    non_production_sources.retain(|source| !production_sources.contains(source));
    non_production_sources
}

fn production_package_sources(
    package: &Package,
    active_features: &BTreeSet<String>,
) -> BTreeSet<PathBuf> {
    package
        .targets
        .iter()
        .filter(|target| {
            is_production_target(&target.kind) && is_active_target(target, active_features)
        })
        .flat_map(|target| production_source_tree(target.src_path.as_std_path(), active_features))
        .collect()
}

fn test_only_package_sources(
    package: &Package,
    active_features: &BTreeSet<String>,
) -> BTreeSet<PathBuf> {
    package
        .targets
        .iter()
        .filter(|target| {
            is_production_target(&target.kind) && is_active_target(target, active_features)
        })
        .flat_map(|target| test_only_source_tree(target.src_path.as_std_path()))
        .collect()
}

fn inactive_package_sources(
    package: &Package,
    active_features: &BTreeSet<String>,
) -> BTreeSet<PathBuf> {
    package
        .targets
        .iter()
        .filter(|target| {
            is_production_target(&target.kind) && is_active_target(target, active_features)
        })
        .flat_map(|target| inactive_source_tree(target.src_path.as_std_path(), active_features))
        .collect()
}

fn active_package_features(metadata: &Metadata, package: &Package) -> BTreeSet<String> {
    metadata
        .resolve
        .as_ref()
        .and_then(|resolve| resolve.nodes.iter().find(|node| node.id == package.id))
        .map(|node| node.features.iter().map(ToString::to_string).collect())
        .unwrap_or_default()
}

fn is_active_target(target: &cargo_metadata::Target, active_features: &BTreeSet<String>) -> bool {
    target
        .required_features
        .iter()
        .all(|feature| active_features.contains(feature))
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
    package: &Package,
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

    let package_root = package_directory(package);
    collect_directory_from_root(
        directory,
        directory,
        recursive,
        files,
        excluded_sources,
        false,
        package_root.as_deref(),
    )
}

fn add_declared_source_file(path: &Path, files: &mut BTreeSet<PathBuf>) {
    if is_rust_file(path) {
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
