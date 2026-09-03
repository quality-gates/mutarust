use cargo_metadata::{Metadata, MetadataCommand, Package, TargetKind};
use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::env;
use std::error::Error;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use syn::{Expr, Item, ItemMacro, ItemMod, Lit, Meta};

thread_local! {
    static AST_CACHE: RefCell<HashMap<PathBuf, Option<Rc<syn::File>>>> = RefCell::new(HashMap::new());
}

fn parse_file_cached(path: &Path) -> Option<Rc<syn::File>> {
    AST_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(entry) = cache.get(path) {
            return entry.clone();
        }
        let parsed = fs::read_to_string(path)
            .ok()
            .and_then(|text| syn::parse_file(&text).ok())
            .map(Rc::new);
        cache.insert(path.to_path_buf(), parsed.clone());
        parsed
    })
}

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
        if is_excluded_directory_target(&path) {
            return collect_directory(&path, recursive, files);
        }
        if let Some(metadata) = workspace_metadata(&path) {
            return collect_workspace(&metadata, &path, recursive, files);
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
    if !include_excluded_sources {
        if let Some((package, active_features)) = package_at_directory(directory) {
            return collect_package_sources(
                &package,
                &active_features,
                directory,
                recursive,
                files,
            );
        }
    }

    let excluded_sources = if include_excluded_sources {
        BTreeSet::new()
    } else {
        non_production_source_paths(directory)?
    };
    collect_directory_from_root(
        directory,
        directory,
        recursive,
        files,
        &excluded_sources,
        include_excluded_sources,
        None,
    )?;

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
    let metadata_directory = directory.parent().unwrap_or(directory);
    let manifest_path = directory.join("Cargo.toml");
    if !manifest_path.is_file() {
        return None;
    }
    let metadata = cargo_metadata(metadata_directory, Some(&manifest_path)).ok()?;
    let package = metadata
        .packages
        .iter()
        .filter(|package| metadata.workspace_members.contains(&package.id))
        .find(|package| package_directory(package).is_some_and(|path| path == directory))?
        .clone();
    let active_features = active_package_features(&metadata, &package);

    Some((package, active_features))
}

fn cargo_metadata(
    directory: &Path,
    manifest_path: Option<&Path>,
) -> Result<Metadata, cargo_metadata::Error> {
    let mut command = MetadataCommand::new();
    command.current_dir(directory);
    if let Some(manifest_path) = manifest_path {
        command.manifest_path(manifest_path);
    }

    command.exec().or_else(|error| {
        let host_target = cargo_host_target().ok_or(error)?;
        let mut fallback = MetadataCommand::new();
        fallback
            .current_dir(directory)
            .env("CARGO_BUILD_TARGET", host_target);
        if let Some(manifest_path) = manifest_path {
            fallback.manifest_path(manifest_path);
        }
        fallback.exec()
    })
}

fn cargo_host_target() -> Option<String> {
    let compiler = env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let output = Command::new(compiler).arg("-vV").output().ok()?;
    std::str::from_utf8(&output.stdout)
        .ok()?
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .map(ToOwned::to_owned)
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

fn non_production_source_paths(path: &Path) -> Result<BTreeSet<PathBuf>, SourceError> {
    let Some(cargo_root) = cargo_root(path) else {
        return Ok(BTreeSet::new());
    };
    let Ok(metadata) = MetadataCommand::new().current_dir(cargo_root).exec() else {
        return Ok(BTreeSet::new());
    };

    let configurations = rustc_configurations(path)?;
    let mut sources = BTreeSet::new();
    for package in metadata
        .packages
        .iter()
        .filter(|package| metadata.workspace_members.contains(&package.id))
    {
        let active_features = active_package_features(&metadata, package);
        sources.extend(non_production_package_sources(
            package,
            &active_features,
            &configurations,
        ));
    }

    Ok(sources)
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

fn inactive_source_tree(
    source: &Path,
    active_features: &BTreeSet<String>,
    configurations: &[BTreeSet<String>],
) -> BTreeSet<PathBuf> {
    let mut sources = BTreeSet::new();
    let Ok(source) = canonical_path(source) else {
        return sources;
    };
    let Some(directory) = source.parent() else {
        return sources;
    };

    for configurations in configurations {
        let mut target_sources = BTreeSet::new();
        collect_inactive_source_tree(
            &source,
            directory,
            active_features,
            configurations,
            &mut target_sources,
        );
        sources.extend(target_sources);
    }
    sources
}

fn collect_inactive_source_tree(
    source: &Path,
    directory: &Path,
    active_features: &BTreeSet<String>,
    configurations: &BTreeSet<String>,
    sources: &mut BTreeSet<PathBuf>,
) {
    let Some(syntax) = parse_file_cached(source) else {
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
            configurations,
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
    configurations: &BTreeSet<String>,
    sources: &mut BTreeSet<PathBuf>,
) {
    if let Item::Macro(item) = item {
        if !configuration_is_active_for_attributes(
            &item.attrs,
            false,
            active_features,
            configurations,
        ) {
            collect_include_source(item, module_root, source_directory, sources);
        }
        return;
    }
    let Item::Mod(module) = item else {
        return;
    };

    if !configuration_is_active_for_attributes(
        &module.attrs,
        false,
        active_features,
        configurations,
    ) {
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
                configurations,
                sources,
            );
        }
        return;
    }

    for source in active_production_module_sources(
        module,
        module_root,
        path_directory,
        active_features,
        configurations,
    ) {
        let module_directory = module_directory(&source);
        collect_inactive_source_tree(
            &source,
            &module_directory,
            active_features,
            configurations,
            sources,
        );
    }
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
    let Some(syntax) = parse_file_cached(source) else {
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

fn production_source_tree(
    source: &Path,
    active_features: &BTreeSet<String>,
    configurations: &[BTreeSet<String>],
) -> BTreeSet<PathBuf> {
    let mut sources = BTreeSet::new();
    let Ok(source) = canonical_path(source) else {
        return sources;
    };
    let Some(directory) = source.parent() else {
        return sources;
    };

    for configurations in configurations {
        let mut target_sources = BTreeSet::new();
        collect_production_source_tree(
            &source,
            directory,
            active_features,
            configurations,
            &mut target_sources,
        );
        sources.extend(target_sources);
    }
    sources
}

fn collect_production_source_tree(
    source: &Path,
    directory: &Path,
    active_features: &BTreeSet<String>,
    configurations: &BTreeSet<String>,
    sources: &mut BTreeSet<PathBuf>,
) {
    if !sources.insert(source.to_path_buf()) {
        return;
    }
    let Some(syntax) = parse_file_cached(source) else {
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
            configurations,
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
    configurations: &BTreeSet<String>,
    sources: &mut BTreeSet<PathBuf>,
) {
    if let Item::Macro(item) = item {
        collect_production_include_source(
            item,
            module_root,
            source_directory,
            active_features,
            configurations,
            sources,
        );
        return;
    }
    let Item::Mod(module) = item else {
        return;
    };
    if !configuration_is_active_for_attributes(
        &module.attrs,
        false,
        active_features,
        configurations,
    ) {
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
                configurations,
                sources,
            );
        }
        return;
    }

    for source in active_production_module_sources(
        module,
        module_root,
        path_directory,
        active_features,
        configurations,
    ) {
        let module_directory = module_directory(&source);
        collect_production_source_tree(
            &source,
            &module_directory,
            active_features,
            configurations,
            sources,
        );
    }
}

fn collect_production_include_source(
    item: &ItemMacro,
    module_root: &Path,
    source_directory: &Path,
    active_features: &BTreeSet<String>,
    configurations: &BTreeSet<String>,
    sources: &mut BTreeSet<PathBuf>,
) {
    if !configuration_is_active_for_attributes(&item.attrs, false, active_features, configurations)
    {
        return;
    }
    let Some(source) = include_source(item, source_directory) else {
        return;
    };

    collect_production_source_tree(
        &source,
        module_root,
        active_features,
        configurations,
        sources,
    );
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
    let Some(syntax) = parse_file_cached(source) else {
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

fn configuration_is_active_for_attributes(
    attributes: &[syn::Attribute],
    test_enabled: bool,
    active_features: &BTreeSet<String>,
    configurations: &BTreeSet<String>,
) -> bool {
    let direct_configuration_is_active = attributes
        .iter()
        .filter(|attribute| attribute.path().is_ident("cfg"))
        .filter_map(|attribute| match &attribute.meta {
            Meta::List(list) => syn::parse2::<Meta>(list.tokens.clone()).ok(),
            _ => None,
        })
        .all(|configuration| {
            configuration_is_active_for_target(
                configuration,
                test_enabled,
                active_features,
                configurations,
            )
        });
    let applied_configuration_is_active = attributes
        .iter()
        .filter(|attribute| attribute.path().is_ident("cfg_attr"))
        .flat_map(|attribute| {
            applied_cfg_conditions(attribute, test_enabled, active_features, configurations)
                .into_iter()
        })
        .flatten()
        .all(|configuration| {
            configuration_is_active_for_target(
                configuration,
                test_enabled,
                active_features,
                configurations,
            )
        });

    direct_configuration_is_active && applied_configuration_is_active
}

fn applied_cfg_conditions(
    attribute: &syn::Attribute,
    test_enabled: bool,
    active_features: &BTreeSet<String>,
    configurations: &BTreeSet<String>,
) -> Option<Vec<Meta>> {
    let Meta::List(list) = &attribute.meta else {
        return None;
    };
    let options = list
        .parse_args_with(syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated)
        .ok()?;
    let mut options = options.into_iter();
    let condition = options.next()?;
    let cfg_attr_is_active = configuration_is_active_for_target(
        condition,
        test_enabled,
        active_features,
        configurations,
    );

    cfg_attr_is_active.then(|| {
        options
            .filter_map(|option| match option {
                Meta::List(list) if list.path.is_ident("cfg") => syn::parse2(list.tokens).ok(),
                _ => None,
            })
            .collect()
    })
}

fn configuration_is_active_for_target(
    configuration: Meta,
    test_enabled: bool,
    active_features: &BTreeSet<String>,
    configurations: &BTreeSet<String>,
) -> bool {
    if configuration.path().is_ident("test") {
        return test_enabled;
    }
    if let Meta::NameValue(value) = &configuration {
        if value.path.is_ident("feature") {
            return feature_name(value).is_some_and(|feature| active_features.contains(&feature));
        }
        return target_configuration_is_active(value, configurations);
    }
    let Meta::List(list) = configuration else {
        return target_flag_is_active(&list_or_path_name(&configuration), configurations);
    };
    let Ok(options) =
        list.parse_args_with(syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated)
    else {
        return true;
    };

    if list.path.is_ident("all") {
        options.into_iter().all(|option| {
            configuration_is_active_for_target(
                option,
                test_enabled,
                active_features,
                configurations,
            )
        })
    } else if list.path.is_ident("any") {
        options.into_iter().any(|option| {
            configuration_is_active_for_target(
                option,
                test_enabled,
                active_features,
                configurations,
            )
        })
    } else if list.path.is_ident("not") {
        !options.into_iter().next().is_none_or(|option| {
            configuration_is_active_for_target(
                option,
                test_enabled,
                active_features,
                configurations,
            )
        })
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

fn target_flag_is_active(name: &str, configurations: &BTreeSet<String>) -> bool {
    configurations.contains(name) || !is_known_configuration_flag(name)
}

fn target_configuration_is_active(
    value: &syn::MetaNameValue,
    configurations: &BTreeSet<String>,
) -> bool {
    let Some(configuration_value) = feature_name(value) else {
        return false;
    };
    let Some(configuration_name) = value.path.get_ident() else {
        return false;
    };

    configurations.contains(&format!("{configuration_name}=\"{configuration_value}\""))
        || !is_known_configuration_value(configuration_name)
}

fn is_known_configuration_flag(name: &str) -> bool {
    matches!(name, "unix" | "windows" | "debug_assertions")
}

fn is_known_configuration_value(name: &syn::Ident) -> bool {
    name == "panic" || name.to_string().starts_with("target_")
}

fn rustc_configurations(directory: &Path) -> Result<Vec<BTreeSet<String>>, SourceError> {
    validate_cargo_configuration(directory)?;
    direct_rustc_configurations(directory)
}

fn direct_rustc_configurations(directory: &Path) -> Result<Vec<BTreeSet<String>>, SourceError> {
    let compiler = env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let targets = configured_cargo_targets(directory)?;

    if let Some(targets) = targets {
        if targets.is_empty() {
            return Ok(rustc_configurations_for(&compiler, None)
                .map(|configurations| vec![configurations])
                .unwrap_or_else(|| vec![default_rustc_configurations()]));
        }

        return targets
            .iter()
            .map(|target| {
                if target.value == "host-tuple" {
                    return rustc_configurations_for(&compiler, None).ok_or_else(|| {
                        SourceError::new(
                            "cannot read Rust compiler configuration for host-tuple".to_owned(),
                        )
                    });
                }

                let target_path = resolved_cargo_target(target);
                custom_target_configurations(&target_path)
                    .or_else(|| rustc_configurations_for(&compiler, Some(&target_path)))
                    .ok_or_else(|| {
                        SourceError::new(format!(
                            "cannot read Rust compiler configuration for target {}",
                            target.value
                        ))
                    })
            })
            .collect();
    }

    Ok(rustc_configurations_for(&compiler, None)
        .map(|configurations| vec![configurations])
        .unwrap_or_else(|| vec![default_rustc_configurations()]))
}

fn rustc_configurations_for(
    compiler: &std::ffi::OsStr,
    target: Option<&Path>,
) -> Option<BTreeSet<String>> {
    let mut command = Command::new(compiler);
    command.args(["--print", "cfg"]);
    if let Some(target) = target {
        command.arg("--target").arg(target);
    }

    command_configurations(&mut command)
}

fn custom_target_configurations(target: &Path) -> Option<BTreeSet<String>> {
    (target
        .extension()
        .is_some_and(|extension| extension == "json"))
    .then(|| fs::read_to_string(target).ok())
    .flatten()
    .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
    .and_then(|target| {
        let mut configurations = BTreeSet::new();
        insert_custom_target_required_configuration(
            &mut configurations,
            &target,
            "arch",
            "target_arch",
        )?;
        insert_custom_target_required_configuration(
            &mut configurations,
            &target,
            "target-pointer-width",
            "target_pointer_width",
        )?;
        for (key, name, default) in [
            ("os", "target_os", "none"),
            ("env", "target_env", ""),
            ("vendor", "target_vendor", "unknown"),
            ("abi", "target_abi", ""),
            ("panic-strategy", "panic", "unwind"),
        ] {
            insert_custom_target_optional_configuration(
                &mut configurations,
                &target,
                key,
                name,
                default,
            )?;
        }
        insert_custom_target_endian(&mut configurations, &target)?;
        insert_custom_target_families(&mut configurations, &target)?;
        insert_custom_target_features(&mut configurations, &target)?;
        insert_custom_target_atomics(&mut configurations, &target)?;
        Some(configurations)
    })
}

fn insert_custom_target_required_configuration(
    configurations: &mut BTreeSet<String>,
    target: &serde_json::Value,
    key: &str,
    name: &str,
) -> Option<()> {
    let value = custom_target_value(target, key)?;
    configurations.insert(format!("{name}=\"{value}\""));
    Some(())
}

fn insert_custom_target_optional_configuration(
    configurations: &mut BTreeSet<String>,
    target: &serde_json::Value,
    key: &str,
    name: &str,
    default: &str,
) -> Option<()> {
    let value = target
        .get(key)
        .map(json_configuration_value)
        .unwrap_or_else(|| Some(default.to_owned()))?;
    configurations.insert(format!("{name}=\"{value}\""));
    Some(())
}

fn custom_target_value(target: &serde_json::Value, key: &str) -> Option<String> {
    target.get(key).and_then(json_configuration_value)
}

fn json_configuration_value(value: &serde_json::Value) -> Option<String> {
    value
        .as_str()
        .map(ToOwned::to_owned)
        .or_else(|| value.as_u64().map(|value| value.to_string()))
}

fn insert_custom_target_endian(
    configurations: &mut BTreeSet<String>,
    target: &serde_json::Value,
) -> Option<()> {
    let endian = custom_target_value(target, "target-endian").or_else(|| {
        target
            .get("data-layout")?
            .as_str()?
            .chars()
            .next()
            .and_then(|value| match value {
                'e' => Some("little".to_owned()),
                'E' => Some("big".to_owned()),
                _ => None,
            })
    })?;
    configurations.insert(format!("target_endian=\"{endian}\""));
    Some(())
}

fn insert_custom_target_families(
    configurations: &mut BTreeSet<String>,
    target: &serde_json::Value,
) -> Option<()> {
    let mut families = custom_target_families(target)?;
    if families.is_empty()
        && target.get("os").and_then(serde_json::Value::as_str) == Some("windows")
    {
        families.push("windows".to_owned());
    }
    if families.is_empty()
        && target
            .get("os")
            .and_then(serde_json::Value::as_str)
            .is_some_and(is_unix_target)
    {
        families.push("unix".to_owned());
    }
    for family in families {
        configurations.insert(format!("target_family=\"{family}\""));
        if family == "unix" || family == "windows" {
            configurations.insert(family);
        }
    }
    Some(())
}

fn custom_target_families(target: &serde_json::Value) -> Option<Vec<String>> {
    let Some(families) = target.get("target-family") else {
        return Some(Vec::new());
    };
    if let Some(family) = families.as_str() {
        return Some(vec![family.to_owned()]);
    }
    families
        .as_array()?
        .iter()
        .map(|family| family.as_str().map(ToOwned::to_owned))
        .collect()
}

fn insert_custom_target_features(
    configurations: &mut BTreeSet<String>,
    target: &serde_json::Value,
) -> Option<()> {
    let Some(features) = target.get("features") else {
        return Some(());
    };
    for feature in features.as_str()?.split(',') {
        if let Some(feature) = feature.strip_prefix('+') {
            configurations.insert(format!("target_feature=\"{feature}\""));
        }
    }
    Some(())
}

fn insert_custom_target_atomics(
    configurations: &mut BTreeSet<String>,
    target: &serde_json::Value,
) -> Option<()> {
    let Some(width) = target
        .get("max-atomic-width")
        .and_then(serde_json::Value::as_u64)
    else {
        return Some(());
    };
    let pointer_width = custom_target_value(target, "target-pointer-width")?
        .parse::<u64>()
        .ok()?;
    let atomic_cas = target
        .get("atomic-cas")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(true);
    for atomic_width in [8, 16, 32, 64, 128] {
        if atomic_width <= width {
            insert_custom_atomic_width(
                configurations,
                "target_has_atomic_primitive_alignment",
                atomic_width,
            );
            if atomic_cas {
                insert_custom_atomic_width(configurations, "target_has_atomic", atomic_width);
            }
        }
    }
    if pointer_width <= width {
        configurations.insert("target_has_atomic_primitive_alignment=\"ptr\"".to_owned());
        if atomic_cas {
            configurations.insert("target_has_atomic=\"ptr\"".to_owned());
        }
    }
    Some(())
}

fn insert_custom_atomic_width(configurations: &mut BTreeSet<String>, name: &str, width: u64) {
    configurations.insert(format!("{name}=\"{width}\""));
}

fn is_unix_target(operating_system: &str) -> bool {
    matches!(
        operating_system,
        "aix"
            | "android"
            | "darwin"
            | "dragonfly"
            | "emscripten"
            | "freebsd"
            | "fuchsia"
            | "haiku"
            | "hermit"
            | "horizon"
            | "hurd"
            | "illumos"
            | "ios"
            | "cygwin"
            | "espidf"
            | "l4re"
            | "linux"
            | "lynxos178"
            | "managarm"
            | "macos"
            | "netbsd"
            | "nto"
            | "nuttx"
            | "openbsd"
            | "qurt"
            | "redox"
            | "rtems"
            | "solaris"
            | "tvos"
            | "visionos"
            | "vita"
            | "vxworks"
            | "watchos"
    )
}

fn configured_cargo_targets(directory: &Path) -> Result<Option<Vec<CargoTarget>>, SourceError> {
    let configuration_targets = cargo_configuration_targets(directory)?;

    Ok(env::var("CARGO_BUILD_TARGET")
        .ok()
        .map(|target| vec![CargoTarget::from(target)])
        .or(configuration_targets))
}

fn cargo_configuration_targets(directory: &Path) -> Result<Option<Vec<CargoTarget>>, SourceError> {
    let mut configuration = cargo_home_configuration_target()?;
    let mut directories = directory.ancestors().collect::<Vec<_>>();
    directories.reverse();

    for directory in directories {
        configuration = merge_cargo_target_configurations(
            configuration,
            cargo_configuration_target_in(directory)?,
        )?;
    }

    Ok(configuration.map(|configuration| match configuration {
        CargoTargetConfiguration::Single(target) => vec![target],
        CargoTargetConfiguration::Multiple(targets) => targets,
    }))
}

enum CargoTargetConfiguration {
    Single(CargoTarget),
    Multiple(Vec<CargoTarget>),
}

struct CargoTarget {
    value: String,
    configuration_directory: Option<PathBuf>,
}

impl From<String> for CargoTarget {
    fn from(value: String) -> Self {
        Self {
            value,
            configuration_directory: None,
        }
    }
}

fn resolved_cargo_target(target: &CargoTarget) -> PathBuf {
    let path = Path::new(&target.value);
    if path.is_absolute() || (!target.value.contains('/') && path.extension().is_none()) {
        return path.to_path_buf();
    }

    target
        .configuration_directory
        .as_ref()
        .map(|directory| directory.join(path))
        .unwrap_or_else(|| path.to_path_buf())
}

fn cargo_home_configuration_target() -> Result<Option<CargoTargetConfiguration>, SourceError> {
    let Some(directory) = cargo_home_directory() else {
        return Ok(None);
    };

    cargo_configuration_target_from_directory(&directory)
}

fn cargo_configuration_target_in(
    directory: &Path,
) -> Result<Option<CargoTargetConfiguration>, SourceError> {
    cargo_configuration_target_from_directory(&directory.join(".cargo"))
}

fn cargo_configuration_target_from_directory(
    directory: &Path,
) -> Result<Option<CargoTargetConfiguration>, SourceError> {
    let extensionless_path = directory.join("config");
    if extensionless_path.is_file() {
        return cargo_configuration_target_from(&extensionless_path);
    }

    cargo_configuration_target_from(&directory.join("config.toml"))
}

fn cargo_configuration_target_from(
    path: &Path,
) -> Result<Option<CargoTargetConfiguration>, SourceError> {
    cargo_configuration_target_from_with_includes(path, &mut BTreeSet::new())
}

fn cargo_configuration_target_from_with_includes(
    path: &Path,
    active_paths: &mut BTreeSet<PathBuf>,
) -> Result<Option<CargoTargetConfiguration>, SourceError> {
    if !path.exists() {
        return Ok(None);
    }
    let canonical_path = fs::canonicalize(path).map_err(|error| {
        SourceError::new(format!(
            "cannot read Cargo configuration {}: {error}",
            path.display()
        ))
    })?;
    if !active_paths.insert(canonical_path.clone()) {
        return Err(SourceError::new(format!(
            "Cargo configuration include cycle contains {}",
            canonical_path.display()
        )));
    }

    let result = (|| {
        let configuration = fs::read_to_string(path).map_err(|error| {
            SourceError::new(format!(
                "cannot read Cargo configuration {}: {error}",
                path.display()
            ))
        })?;
        let table = toml::from_str::<toml::Table>(&configuration).map_err(|error| {
            SourceError::new(format!(
                "cannot parse Cargo configuration {}: {error}",
                path.display()
            ))
        })?;
        let mut included_configuration = None;
        for path in cargo_configuration_includes(&table, &canonical_path) {
            included_configuration = merge_cargo_target_configurations(
                included_configuration,
                cargo_configuration_target_from_with_includes(&path, active_paths)?,
            )?;
        }

        merge_cargo_target_configurations(
            included_configuration,
            cargo_target_configuration_from_table(&table, &canonical_path)?,
        )
    })();
    active_paths.remove(&canonical_path);
    result
}

fn cargo_configuration_includes(table: &toml::Table, configuration_path: &Path) -> Vec<PathBuf> {
    let Some(includes) = table.get("include").and_then(toml::Value::as_array) else {
        return Vec::new();
    };
    let Some(directory) = configuration_path.parent() else {
        return Vec::new();
    };

    includes
        .iter()
        .filter_map(cargo_configuration_include_path)
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "toml")
        })
        .map(|path| directory.join(path))
        .collect()
}

fn cargo_configuration_include_path(value: &toml::Value) -> Option<PathBuf> {
    value
        .as_str()
        .map(PathBuf::from)
        .or_else(|| value.as_table()?.get("path")?.as_str().map(PathBuf::from))
}

fn validate_cargo_configuration(current_directory: &Path) -> Result<(), SourceError> {
    if let Some(directory) = cargo_home_directory() {
        validate_cargo_configuration_directory(&directory)?;
    }

    let mut directories = current_directory.ancestors().collect::<Vec<_>>();
    directories.reverse();
    for directory in directories {
        validate_cargo_configuration_directory(&directory.join(".cargo"))?;
    }

    Ok(())
}

fn cargo_home_directory() -> Option<PathBuf> {
    env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".cargo")))
}

fn validate_cargo_configuration_directory(directory: &Path) -> Result<(), SourceError> {
    let extensionless_path = directory.join("config");
    let path = if extensionless_path.is_file() {
        Some(extensionless_path)
    } else {
        let toml_path = directory.join("config.toml");
        toml_path.is_file().then_some(toml_path)
    };
    let Some(path) = path else {
        return Ok(());
    };

    validate_cargo_configuration_file(&path, &mut BTreeSet::new())
}

fn validate_cargo_configuration_file(
    path: &Path,
    active_paths: &mut BTreeSet<PathBuf>,
) -> Result<(), SourceError> {
    let canonical_path = fs::canonicalize(path).map_err(|error| {
        SourceError::new(format!(
            "cannot read Cargo configuration {}: {error}",
            path.display()
        ))
    })?;
    if !active_paths.insert(canonical_path.clone()) {
        return Err(SourceError::new(format!(
            "Cargo configuration include cycle contains {}",
            canonical_path.display()
        )));
    }

    let result = (|| {
        let configuration = fs::read_to_string(&canonical_path).map_err(|error| {
            SourceError::new(format!(
                "cannot read Cargo configuration {}: {error}",
                canonical_path.display()
            ))
        })?;
        let table = toml::from_str::<toml::Table>(&configuration).map_err(|error| {
            SourceError::new(format!(
                "cannot parse Cargo configuration {}: {error}",
                canonical_path.display()
            ))
        })?;

        for include in cargo_configuration_include_entries(&table, &canonical_path)? {
            if include.optional && !include.path.exists() {
                continue;
            }
            validate_cargo_configuration_file(&include.path, active_paths)?;
        }

        Ok(())
    })();
    active_paths.remove(&canonical_path);
    result
}

struct CargoConfigurationInclude {
    path: PathBuf,
    optional: bool,
}

fn cargo_configuration_include_entries(
    table: &toml::Table,
    configuration_path: &Path,
) -> Result<Vec<CargoConfigurationInclude>, SourceError> {
    let Some(includes) = table.get("include") else {
        return Ok(Vec::new());
    };
    let Some(includes) = includes.as_array() else {
        return Err(SourceError::new(
            "Cargo configuration include must be an array".to_owned(),
        ));
    };
    let Some(directory) = configuration_path.parent() else {
        return Err(SourceError::new(
            "Cargo configuration has no parent directory".to_owned(),
        ));
    };

    includes
        .iter()
        .map(|include| cargo_configuration_include_entry(include, directory))
        .collect()
}

fn cargo_configuration_include_entry(
    value: &toml::Value,
    directory: &Path,
) -> Result<CargoConfigurationInclude, SourceError> {
    let (path, optional) = if let Some(path) = value.as_str() {
        (path, false)
    } else if let Some(table) = value.as_table() {
        let Some(path) = table.get("path").and_then(toml::Value::as_str) else {
            return Err(SourceError::new(
                "Cargo configuration include table needs a path".to_owned(),
            ));
        };
        let optional = match table.get("optional") {
            Some(value) => value.as_bool().ok_or_else(|| {
                SourceError::new(
                    "Cargo configuration include optional must be a boolean".to_owned(),
                )
            })?,
            None => false,
        };
        (path, optional)
    } else {
        return Err(SourceError::new(
            "Cargo configuration include must be a path or table".to_owned(),
        ));
    };
    let path = PathBuf::from(path);
    if path.extension().is_none_or(|extension| extension != "toml") {
        return Err(SourceError::new(
            "Cargo configuration include path must end in .toml".to_owned(),
        ));
    }

    Ok(CargoConfigurationInclude {
        path: directory.join(path),
        optional,
    })
}

fn merge_cargo_target_configurations(
    current: Option<CargoTargetConfiguration>,
    additional: Option<CargoTargetConfiguration>,
) -> Result<Option<CargoTargetConfiguration>, SourceError> {
    let Some(additional) = additional else {
        return Ok(current);
    };

    match (current, additional) {
        (None, additional) => Ok(Some(additional)),
        (Some(CargoTargetConfiguration::Single(_)), CargoTargetConfiguration::Single(target)) => {
            Ok(Some(CargoTargetConfiguration::Single(target)))
        }
        (
            Some(CargoTargetConfiguration::Multiple(mut targets)),
            CargoTargetConfiguration::Multiple(mut additional_targets),
        ) => {
            targets.append(&mut additional_targets);
            Ok(Some(CargoTargetConfiguration::Multiple(targets)))
        }
        _ => Err(SourceError::new(
            "Cargo configuration build.target must have one type".to_owned(),
        )),
    }
}

fn cargo_target_configuration_from_table(
    table: &toml::Table,
    configuration_path: &Path,
) -> Result<Option<CargoTargetConfiguration>, SourceError> {
    let Some(target) = table
        .get("build")
        .and_then(toml::Value::as_table)
        .and_then(|build| build.get("target"))
    else {
        return Ok(None);
    };
    let configuration_directory = configuration_path
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf);

    if let Some(value) = target.as_str() {
        return Ok(Some(CargoTargetConfiguration::Single(CargoTarget {
            value: value.to_owned(),
            configuration_directory,
        })));
    }
    let Some(values) = target.as_array() else {
        return Err(SourceError::new(
            "Cargo configuration build.target must be a string or array".to_owned(),
        ));
    };
    let targets = values
        .iter()
        .map(|value| {
            value.as_str().map(|value| CargoTarget {
                value: value.to_owned(),
                configuration_directory: configuration_directory.clone(),
            })
        })
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| {
            SourceError::new(
                "Cargo configuration build.target array must contain strings".to_owned(),
            )
        })?;

    Ok(Some(CargoTargetConfiguration::Multiple(targets)))
}

fn command_configurations(command: &mut Command) -> Option<BTreeSet<String>> {
    let Ok(output) = command.output() else {
        return None;
    };
    if !output.status.success() {
        return None;
    }
    let Ok(configurations) = String::from_utf8(output.stdout) else {
        return None;
    };

    Some(configurations.lines().map(str::to_owned).collect())
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

fn active_production_module_sources(
    module: &ItemMod,
    module_directory: &Path,
    path_directory: &Path,
    active_features: &BTreeSet<String>,
    configurations: &BTreeSet<String>,
) -> Vec<PathBuf> {
    let fallback_source = external_module_source(module, module_directory, path_directory);
    let sources = active_production_path_attributes_for(module, active_features, configurations)
        .into_iter()
        .map(|path| path_directory.join(path))
        .filter_map(|path| canonical_path(&path).ok())
        .collect::<Vec<_>>();

    (!sources.is_empty())
        .then_some(sources)
        .or_else(|| fallback_source.map(|source| vec![source]))
        .unwrap_or_default()
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

fn active_production_path_attributes_for(
    module: &ItemMod,
    active_features: &BTreeSet<String>,
    configurations: &BTreeSet<String>,
) -> Vec<PathBuf> {
    module
        .attrs
        .iter()
        .filter_map(|attribute| {
            let Meta::List(list) = &attribute.meta else {
                return None;
            };
            if !attribute.path().is_ident("cfg_attr") {
                return None;
            }
            let Ok(options) = list.parse_args_with(
                syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated,
            ) else {
                return None;
            };
            let mut options = options.into_iter();
            let configuration = options.next()?;
            configuration_is_active_for_target(
                configuration,
                false,
                active_features,
                configurations,
            )
            .then(|| options.find_map(path_from_meta))
            .flatten()
        })
        .collect()
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
    let directory = env::current_dir()
        .map_err(|error| SourceError::new(format!("cannot read current directory: {error}")))?;
    let metadata = MetadataCommand::new()
        .exec()
        .map_err(|error| SourceError::new(format!("cannot read Cargo metadata: {error}")))?;
    let Some(package) = metadata.packages.iter().find(|package| {
        package.name.as_ref() == target.value && metadata.workspace_members.contains(&package.id)
    }) else {
        return Ok(false);
    };
    let active_features = active_package_features(&metadata, package);
    collect_package_sources(
        package,
        &active_features,
        &directory,
        target.recursive,
        files,
    )?;
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
    configuration_directory: &Path,
    recursive: bool,
    files: &mut BTreeSet<PathBuf>,
) -> Result<(), SourceError> {
    for package in &metadata.packages {
        if metadata.workspace_members.contains(&package.id) {
            let active_features = active_package_features(metadata, package);
            collect_package_sources(
                package,
                &active_features,
                configuration_directory,
                recursive,
                files,
            )?;
        }
    }

    Ok(())
}

fn collect_package_sources(
    package: &Package,
    active_features: &BTreeSet<String>,
    configuration_directory: &Path,
    recursive: bool,
    files: &mut BTreeSet<PathBuf>,
) -> Result<(), SourceError> {
    let configurations = rustc_configurations(configuration_directory)?;
    let excluded_sources =
        non_production_package_sources(package, active_features, &configurations);

    for target in &package.targets {
        if is_production_target(&target.kind) && is_active_target(target, active_features) {
            for source in production_source_tree(
                target.src_path.as_std_path(),
                active_features,
                &configurations,
            ) {
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
    configurations: &[BTreeSet<String>],
) -> BTreeSet<PathBuf> {
    let test_only_sources = test_only_package_sources(package, active_features);
    let inactive_sources = inactive_package_sources(package, active_features, configurations);
    let production_sources = production_package_sources(package, active_features, configurations);
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
    configurations: &[BTreeSet<String>],
) -> BTreeSet<PathBuf> {
    package
        .targets
        .iter()
        .filter(|target| {
            is_production_target(&target.kind) && is_active_target(target, active_features)
        })
        .flat_map(|target| {
            production_source_tree(
                target.src_path.as_std_path(),
                active_features,
                configurations,
            )
        })
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
    configurations: &[BTreeSet<String>],
) -> BTreeSet<PathBuf> {
    package
        .targets
        .iter()
        .filter(|target| {
            is_production_target(&target.kind) && is_active_target(target, active_features)
        })
        .flat_map(|target| {
            inactive_source_tree(
                target.src_path.as_std_path(),
                active_features,
                configurations,
            )
        })
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
