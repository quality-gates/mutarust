use std::collections::BTreeSet;
use std::env;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use cargo_metadata::{Metadata, MetadataCommand};

#[cfg(unix)]
use std::sync::atomic::AtomicBool;

use crate::{Mutation, Mutator, Registry, SourceError, find_rust_sources};

static NEXT_TEMPORARY_WORKSPACE: AtomicU64 = AtomicU64::new(0);
#[cfg(unix)]
static MUTATION_RUN_INTERRUPTED: AtomicBool = AtomicBool::new(false);

/// The fixed test timeout for a mutation run without a timeout option.
pub const DEFAULT_TEST_TIMEOUT: Duration = Duration::from_secs(60);

/// A complete sequential mutation test run.
pub struct MutationRun {
    results: Vec<MutationResult>,
}

impl MutationRun {
    /// Returns the result for each generated mutant.
    pub fn results(&self) -> &[MutationResult] {
        &self.results
    }

    /// Returns the number of killed mutants.
    pub fn killed(&self) -> usize {
        self.count(MutationState::Killed)
    }

    /// Returns the number of escaped mutants.
    pub fn escaped(&self) -> usize {
        self.count(MutationState::Escaped)
    }

    /// Returns the number of errored mutants.
    pub fn errored(&self) -> usize {
        self.count(MutationState::Errored)
    }

    /// Returns the number of mutants with no test coverage.
    pub fn not_covered(&self) -> usize {
        self.count(MutationState::NotCovered)
    }

    /// Returns the number of mutants that Mutarust did not run.
    pub fn skipped(&self) -> usize {
        self.count(MutationState::Skipped)
    }

    fn count(&self, expected: MutationState) -> usize {
        self.results
            .iter()
            .filter(|result| result.state == expected)
            .count()
    }
}

/// The result of testing one mutant.
pub struct MutationResult {
    /// The mutated production source file.
    pub source: PathBuf,
    /// The stable name of the mutator that produced this mutant.
    pub mutator: String,
    /// The mutation test result state.
    pub state: MutationState,
    /// The error detail when Mutarust could not complete the test run.
    pub error: Option<String>,
}

/// The classification of one mutation test result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MutationState {
    /// Tests detected the mutant.
    Killed,
    /// Tests did not detect the mutant.
    Escaped,
    /// Mutarust could not complete the mutant test run.
    Errored,
    /// No selected test covers the mutant.
    NotCovered,
    /// Mutarust did not run the mutant.
    Skipped,
}

impl fmt::Display for MutationState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Killed => formatter.write_str("killed"),
            Self::Escaped => formatter.write_str("escaped"),
            Self::Errored => formatter.write_str("errored"),
            Self::NotCovered => formatter.write_str("not covered"),
            Self::Skipped => formatter.write_str("skipped"),
        }
    }
}

/// Describes a mutation run setup failure.
#[derive(Debug)]
pub struct RunError {
    message: String,
}

impl fmt::Display for RunError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for RunError {}

/// Runs each generated mutant in an isolated copy of its Cargo workspace.
pub fn run_mutation_tests(
    targets: &[String],
    registry: &Registry,
) -> Result<MutationRun, RunError> {
    run_mutation_tests_with_timeout(targets, registry, DEFAULT_TEST_TIMEOUT)
}

/// Runs each generated mutant with a fixed test timeout.
pub fn run_mutation_tests_with_timeout(
    targets: &[String],
    registry: &Registry,
    timeout: Duration,
) -> Result<MutationRun, RunError> {
    prepare_interrupt_handling()?;
    let candidates = mutation_candidates(targets, registry)?;
    let mut results = Vec::new();
    for candidate in candidates {
        if mutation_run_was_interrupted() {
            return Err(run_error("mutation run interrupted"));
        }
        results.push(test_candidate(candidate, timeout));
        if mutation_run_was_interrupted() {
            return Err(run_error("mutation run interrupted"));
        }
    }
    Ok(MutationRun { results })
}

#[cfg(unix)]
fn prepare_interrupt_handling() -> Result<(), RunError> {
    MUTATION_RUN_INTERRUPTED.store(false, Ordering::SeqCst);
    let handler = record_interrupt as *const () as usize;
    let previous = unsafe { libc::signal(libc::SIGINT, handler) };
    if previous == libc::SIG_ERR {
        return Err(run_error(format!(
            "could not handle mutation run interrupts: {}",
            std::io::Error::last_os_error()
        )));
    }
    Ok(())
}

#[cfg(not(unix))]
fn prepare_interrupt_handling() -> Result<(), RunError> {
    Ok(())
}

#[cfg(unix)]
extern "C" fn record_interrupt(_: libc::c_int) {
    MUTATION_RUN_INTERRUPTED.store(true, Ordering::SeqCst);
}

#[cfg(unix)]
fn mutation_run_was_interrupted() -> bool {
    MUTATION_RUN_INTERRUPTED.load(Ordering::SeqCst)
}

#[cfg(not(unix))]
fn mutation_run_was_interrupted() -> bool {
    false
}

fn mutation_candidates(
    targets: &[String],
    registry: &Registry,
) -> Result<Vec<MutationCandidate>, RunError> {
    let sources = find_rust_sources(targets).map_err(source_error)?;
    if sources.is_empty() {
        return Err(run_error(
            "could not find any suitable Rust production source files",
        ));
    }
    let mut candidates = Vec::new();
    for source in sources {
        let source = fs::canonicalize(&source).map_err(|error| {
            run_error(format!("could not resolve {}: {error}", source.display()))
        })?;
        let workspace = workspace_for(&source)?;
        let text = fs::read_to_string(&source)
            .map_err(|error| run_error(format!("could not read {}: {error}", source.display())))?;
        add_source_candidates(&mut candidates, registry, &workspace, &source, &text);
    }
    deduplicate_candidates(&mut candidates);
    Ok(candidates)
}

fn deduplicate_candidates(candidates: &mut Vec<MutationCandidate>) {
    let mut seen = BTreeSet::new();
    candidates.retain(|candidate| {
        let (range, replacement) = candidate.mutation.identity();
        seen.insert((
            candidate.source.clone(),
            range.start,
            range.end,
            replacement.to_owned(),
        ))
    });
}

fn workspace_for(source: &Path) -> Result<Workspace, RunError> {
    let directory = source.parent().ok_or_else(|| {
        run_error(format!(
            "source file has no parent directory: {}",
            source.display()
        ))
    })?;
    let metadata = metadata_for_directory(directory, source).or_else(|_| {
        let current = env::current_dir()
            .map_err(|error| run_error(format!("could not read the current directory: {error}")))?;
        metadata_for_directory(&current, source)
    })?;
    let root = fs::canonicalize(metadata.workspace_root.as_std_path()).map_err(|error| {
        run_error(format!(
            "could not resolve Cargo workspace for {}: {error}",
            source.display()
        ))
    })?;
    let manifest = package_manifest_for(&metadata, source)?;
    let configurations = cargo_configurations(&root);
    let mut copy_paths = copy_paths_for(&metadata, &root, source)?;
    copy_paths.extend(configuration_paths(&configurations)?);
    let copy_paths = copy_roots(copy_paths.into_iter().collect());
    let mut layout_paths = copy_paths.clone();
    layout_paths.extend(configurations.iter().cloned());
    let layout_root = common_ancestor(&layout_paths)?;
    Ok(Workspace {
        root,
        manifest,
        layout_root,
        copy_paths,
        configurations,
    })
}

fn metadata_for_directory(directory: &Path, source: &Path) -> Result<Metadata, RunError> {
    MetadataCommand::new()
        .current_dir(directory)
        .no_deps()
        .exec()
        .map_err(|error| {
            run_error(format!(
                "could not find a Cargo workspace for {}: {error}",
                source.display()
            ))
        })
}

fn package_manifest_for(metadata: &Metadata, source: &Path) -> Result<PathBuf, RunError> {
    let manifest = metadata
        .packages
        .iter()
        .filter_map(|package| {
            let source_root = package
                .targets
                .iter()
                .filter_map(|target| target.src_path.parent())
                .filter_map(|path| fs::canonicalize(path.as_std_path()).ok())
                .filter(|path| source.starts_with(path))
                .max_by_key(|path| path.components().count())?;
            Some((source_root, package.manifest_path.as_std_path()))
        })
        .max_by_key(|(source_root, _)| source_root.components().count())
        .map(|(_, manifest)| manifest)
        .ok_or_else(|| {
            run_error(format!(
                "could not find the Cargo package that owns {}",
                source.display()
            ))
        })?;
    fs::canonicalize(manifest)
        .map_err(|error| run_error(format!("could not resolve {}: {error}", manifest.display())))
}

fn copy_paths_for(
    metadata: &Metadata,
    root: &Path,
    source: &Path,
) -> Result<Vec<PathBuf>, RunError> {
    let mut paths = BTreeSet::new();
    paths.insert(root.to_path_buf());
    let mut dependency_paths = Vec::new();
    add_metadata_paths(&mut paths, &mut dependency_paths, metadata)?;
    while let Some(path) = dependency_paths.pop() {
        if paths.contains(&path) {
            continue;
        }
        let manifest = path.join("Cargo.toml");
        let dependency_metadata = MetadataCommand::new()
            .manifest_path(&manifest)
            .no_deps()
            .exec()
            .map_err(|error| {
                run_error(format!(
                    "could not read local dependency {}: {error}",
                    path.display()
                ))
            })?;
        add_metadata_paths(&mut paths, &mut dependency_paths, &dependency_metadata)?;
    }
    add_manifest_paths(&mut paths)?;
    paths.insert(source.to_path_buf());
    Ok(copy_roots(paths))
}

fn add_manifest_paths(paths: &mut BTreeSet<PathBuf>) -> Result<(), RunError> {
    let mut inspected = BTreeSet::new();
    loop {
        let manifests = paths
            .iter()
            .map(|path| path.join("Cargo.toml"))
            .filter(|path| path.is_file() && inspected.insert(path.clone()))
            .collect::<Vec<_>>();
        if manifests.is_empty() {
            return Ok(());
        }
        for manifest in manifests {
            let text = fs::read_to_string(&manifest).map_err(|error| {
                run_error(format!("could not read {}: {error}", manifest.display()))
            })?;
            let value = text.parse::<toml::Value>().map_err(|error| {
                run_error(format!("could not read {}: {error}", manifest.display()))
            })?;
            collect_manifest_paths(
                &value,
                manifest.parent().expect("manifest must have a parent"),
                paths,
                &[],
            )?;
        }
    }
}

fn collect_manifest_paths(
    value: &toml::Value,
    directory: &Path,
    paths: &mut BTreeSet<PathBuf>,
    sections: &[String],
) -> Result<(), RunError> {
    match value {
        toml::Value::Table(table) => {
            for (name, value) in table {
                if name == "path" && is_cargo_dependency_path(sections) {
                    add_manifest_path(value, directory, paths)?;
                }
                let mut nested_sections = sections.to_vec();
                nested_sections.push(name.to_owned());
                collect_manifest_paths(value, directory, paths, &nested_sections)?;
            }
        }
        toml::Value::Array(values) => {
            for value in values {
                collect_manifest_paths(value, directory, paths, sections)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn add_manifest_path(
    value: &toml::Value,
    directory: &Path,
    paths: &mut BTreeSet<PathBuf>,
) -> Result<(), RunError> {
    let Some(path) = value.as_str() else {
        return Ok(());
    };
    let path = Path::new(path);
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        directory.join(path)
    };
    if path.exists() {
        paths.insert(fs::canonicalize(&path).map_err(|error| {
            run_error(format!("could not resolve {}: {error}", path.display()))
        })?);
    }
    Ok(())
}

fn is_cargo_dependency_path(sections: &[String]) -> bool {
    let Some(last) = sections.last() else {
        return false;
    };
    if matches!(
        last.as_str(),
        "dependencies" | "dev-dependencies" | "build-dependencies"
    ) {
        return matches!(
            sections.first().map(String::as_str),
            None | Some("target") | Some("workspace")
        );
    }
    matches!(
        sections.first().map(String::as_str),
        Some("patch") | Some("replace")
    )
}

fn add_metadata_paths(
    paths: &mut BTreeSet<PathBuf>,
    dependency_paths: &mut Vec<PathBuf>,
    metadata: &Metadata,
) -> Result<(), RunError> {
    for package in &metadata.packages {
        let package_root = package
            .manifest_path
            .parent()
            .expect("manifest must have a parent");
        paths.insert(
            fs::canonicalize(package_root).map_err(|error| {
                run_error(format!("could not resolve {}: {error}", package_root))
            })?,
        );
        for target in &package.targets {
            let target_root = target
                .src_path
                .parent()
                .expect("target source must have a parent");
            paths.insert(fs::canonicalize(target_root).map_err(|error| {
                run_error(format!("could not resolve {}: {error}", target_root))
            })?);
        }
        for dependency in &package.dependencies {
            if let Some(path) = &dependency.path {
                dependency_paths.push(fs::canonicalize(path).map_err(|error| {
                    run_error(format!(
                        "could not resolve local dependency {path}: {error}"
                    ))
                })?);
            }
        }
    }
    Ok(())
}

fn copy_roots(paths: BTreeSet<PathBuf>) -> Vec<PathBuf> {
    let mut roots = paths.into_iter().collect::<Vec<_>>();
    roots.sort_by_key(|path| path.components().count());
    roots
        .iter()
        .filter(|path| {
            !roots
                .iter()
                .any(|root| root != *path && path.starts_with(root))
        })
        .cloned()
        .collect()
}

fn common_ancestor(paths: &[PathBuf]) -> Result<PathBuf, RunError> {
    let Some(first) = paths.first() else {
        return Err(run_error(
            "could not determine an isolated workspace layout",
        ));
    };
    first
        .ancestors()
        .find(|ancestor| paths.iter().all(|path| path.starts_with(ancestor)))
        .map(Path::to_path_buf)
        .ok_or_else(|| run_error("could not determine an isolated workspace layout"))
}

fn cargo_configurations(root: &Path) -> Vec<PathBuf> {
    root.ancestors()
        .flat_map(|directory| {
            ["config.toml", "config"]
                .into_iter()
                .map(|name| directory.join(".cargo").join(name))
        })
        .filter(|path| path.is_file())
        .collect()
}

fn configuration_paths(configurations: &[PathBuf]) -> Result<Vec<PathBuf>, RunError> {
    let mut paths = BTreeSet::new();
    for configuration in configurations {
        let text = fs::read_to_string(configuration).map_err(|error| {
            run_error(format!(
                "could not read Cargo configuration {}: {error}",
                configuration.display()
            ))
        })?;
        let value = text.parse::<toml::Value>().map_err(|error| {
            run_error(format!(
                "could not parse Cargo configuration {}: {error}",
                configuration.display()
            ))
        })?;
        let directory = configuration
            .parent()
            .expect("Cargo configuration must have a parent");
        collect_configuration_paths(&value, directory, &mut paths)?;
    }
    Ok(paths.into_iter().collect())
}

fn collect_configuration_paths(
    value: &toml::Value,
    directory: &Path,
    paths: &mut BTreeSet<PathBuf>,
) -> Result<(), RunError> {
    let Some(table) = value.as_table() else {
        return Ok(());
    };
    add_configuration_values(table.get("paths"), directory, paths)?;
    if let Some(build) = table.get("build").and_then(toml::Value::as_table) {
        add_configuration_values(build.get("target"), directory, paths)?;
    }
    if let Some(target) = table.get("target").and_then(toml::Value::as_table) {
        for settings in target.values().filter_map(toml::Value::as_table) {
            add_configuration_values(settings.get("runner"), directory, paths)?;
            add_configuration_values(settings.get("linker"), directory, paths)?;
        }
    }
    Ok(())
}

fn add_configuration_values(
    value: Option<&toml::Value>,
    directory: &Path,
    paths: &mut BTreeSet<PathBuf>,
) -> Result<(), RunError> {
    let Some(value) = value else {
        return Ok(());
    };
    if let Some(path) = value.as_str() {
        add_configuration_path(path, directory, paths)?;
    } else if let Some(values) = value.as_array() {
        for value in values {
            if let Some(path) = value.as_str() {
                add_configuration_path(path, directory, paths)?;
            }
        }
    }
    Ok(())
}

fn add_configuration_path(
    value: &str,
    directory: &Path,
    paths: &mut BTreeSet<PathBuf>,
) -> Result<(), RunError> {
    let path = Path::new(value);
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        directory.join(path)
    };
    if path.exists() {
        paths.insert(fs::canonicalize(&path).map_err(|error| {
            run_error(format!("could not resolve {}: {error}", path.display()))
        })?);
    }
    Ok(())
}

fn source_error(error: SourceError) -> RunError {
    run_error(error.to_string())
}

fn add_source_candidates(
    candidates: &mut Vec<MutationCandidate>,
    registry: &Registry,
    workspace: &Workspace,
    source: &Path,
    text: &str,
) {
    for name in registry.names() {
        let mutator = registry
            .get(name)
            .expect("registered mutator name must resolve to a mutator");
        add_mutator_candidates(candidates, workspace, source, name, mutator, text);
    }
}

fn add_mutator_candidates(
    candidates: &mut Vec<MutationCandidate>,
    workspace: &Workspace,
    source: &Path,
    name: &str,
    mutator: &dyn Mutator,
    text: &str,
) {
    for mutation in mutator.mutations(text) {
        candidates.push(MutationCandidate {
            workspace: workspace.clone(),
            source: source.to_path_buf(),
            mutator: name.to_owned(),
            mutation,
        });
    }
}

fn test_candidate(candidate: MutationCandidate, timeout: Duration) -> MutationResult {
    let (state, error) = test_candidate_state(&candidate, timeout)
        .unwrap_or_else(|error| (MutationState::Errored, Some(error.to_string())));
    MutationResult {
        source: candidate.source,
        mutator: candidate.mutator,
        state,
        error,
    }
}

fn test_candidate_state(
    candidate: &MutationCandidate,
    timeout: Duration,
) -> Result<(MutationState, Option<String>), RunError> {
    let temporary = TemporaryWorkspace::create()?;
    let workspace = copy_workspace(&candidate.workspace, temporary.path())?;
    let (baseline, error) =
        run_cargo_test(temporary.path(), &workspace, &candidate.workspace, timeout)?;
    if baseline != MutationState::Escaped {
        let error = error.unwrap_or_else(|| "cargo test exited with a failure status".to_owned());
        return Ok((
            MutationState::Errored,
            Some(format!("unmodified cargo test did not pass: {error}")),
        ));
    }
    write_mutant(&candidate.workspace, temporary.path(), candidate)?;
    run_cargo_test(temporary.path(), &workspace, &candidate.workspace, timeout)
}

fn write_mutant(
    workspace: &Workspace,
    temporary: &Path,
    candidate: &MutationCandidate,
) -> Result<(), RunError> {
    let path = copied_path(workspace, temporary, &candidate.source)?;
    let source = fs::read_to_string(&path)
        .map_err(|error| run_error(format!("could not read {}: {error}", path.display())))?;
    let mutant = candidate.mutation.apply(&source).ok_or_else(|| {
        run_error(format!(
            "could not apply mutation to {}",
            candidate.source.display()
        ))
    })?;
    fs::write(&path, mutant)
        .map_err(|error| run_error(format!("could not write {}: {error}", path.display())))
}

fn run_cargo_test(
    temporary: &Path,
    copied_workspace: &Path,
    workspace: &Workspace,
    timeout: Duration,
) -> Result<(MutationState, Option<String>), RunError> {
    let manifest = copied_path(workspace, temporary, &workspace.manifest)?;
    let cargo = env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut command = Command::new(cargo);
    command
        .args(["test", "--manifest-path"])
        .arg(manifest)
        .args(["--target-dir"])
        .arg(temporary.join("target"))
        .current_dir(copied_workspace)
        .env_remove("CARGO_TARGET_DIR")
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    configure_process_group(&mut command);
    let mut child = command
        .spawn()
        .map_err(|error| run_error(format!("could not run cargo test: {error}")))?;
    wait_for_cargo_test(&mut child, timeout)
}

fn wait_for_cargo_test(
    child: &mut std::process::Child,
    timeout: Duration,
) -> Result<(MutationState, Option<String>), RunError> {
    let started = Instant::now();
    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| run_error(format!("could not wait for cargo test: {error}")))?
        {
            let state = if status.success() {
                MutationState::Escaped
            } else {
                MutationState::Killed
            };
            return Ok((state, None));
        }
        if mutation_run_was_interrupted() {
            stop_cargo_test(child)?;
            child.wait().map_err(|error| {
                run_error(format!("could not reap interrupted cargo test: {error}"))
            })?;
            return Err(run_error("mutation run interrupted"));
        }
        if started.elapsed() >= timeout {
            stop_cargo_test(child)?;
            child.wait().map_err(|error| {
                run_error(format!("could not reap timed out cargo test: {error}"))
            })?;
            return Ok((
                MutationState::Errored,
                Some(format!(
                    "cargo test timed out after {} seconds",
                    timeout.as_secs()
                )),
            ));
        }
        thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(unix)]
fn configure_process_group(command: &mut Command) {
    use std::os::unix::process::CommandExt;

    command.process_group(0);
}

#[cfg(not(unix))]
fn configure_process_group(_command: &mut Command) {}

#[cfg(unix)]
fn stop_cargo_test(child: &mut std::process::Child) -> Result<(), RunError> {
    let process_group = i32::try_from(child.id())
        .map_err(|error| run_error(format!("could not identify timed out cargo test: {error}")))?;
    let result = unsafe { libc::kill(-process_group, libc::SIGKILL) };
    if result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
        Ok(())
    } else {
        Err(run_error(format!(
            "could not stop timed out cargo test: {}",
            std::io::Error::last_os_error()
        )))
    }
}

#[cfg(windows)]
fn stop_cargo_test(child: &mut std::process::Child) -> Result<(), RunError> {
    let identifier = child.id().to_string();
    let status = Command::new("taskkill")
        .args(["/PID", &identifier, "/T", "/F"])
        .status()
        .map_err(|error| run_error(format!("could not stop timed out cargo test: {error}")))?;
    if status.success() {
        Ok(())
    } else {
        Err(run_error("could not stop timed out cargo test"))
    }
}

#[cfg(all(not(unix), not(windows)))]
fn stop_cargo_test(child: &mut std::process::Child) -> Result<(), RunError> {
    child
        .kill()
        .map_err(|error| run_error(format!("could not stop timed out cargo test: {error}")))
}

fn copy_workspace(workspace: &Workspace, destination: &Path) -> Result<PathBuf, RunError> {
    for source in &workspace.copy_paths {
        let copied = copied_path(workspace, destination, source)?;
        let parent = copied.parent().expect("copied path must have a parent");
        fs::create_dir_all(parent).map_err(|error| {
            run_error(format!("could not create {}: {error}", parent.display()))
        })?;
        if copied.exists() {
            copy_directory(source, &copied)?;
        } else {
            copy_entry(source, &copied)?;
        }
    }
    copy_cargo_configurations(workspace, destination)?;
    rewrite_cargo_configurations(workspace, destination)?;
    rewrite_cargo_manifests(workspace, destination)?;
    copied_path(workspace, destination, &workspace.root)
}

fn rewrite_cargo_manifests(workspace: &Workspace, destination: &Path) -> Result<(), RunError> {
    let mut manifests = Vec::new();
    find_cargo_manifests(destination, &mut manifests)?;
    for manifest in manifests {
        let text = fs::read_to_string(&manifest).map_err(|error| {
            run_error(format!("could not read {}: {error}", manifest.display()))
        })?;
        let mut value = text.parse::<toml::Value>().map_err(|error| {
            run_error(format!("could not read {}: {error}", manifest.display()))
        })?;
        rewrite_manifest_paths(&mut value, workspace, destination)?;
        fs::write(
            &manifest,
            toml::to_string(&value).expect("manifest value must serialize"),
        )
        .map_err(|error| run_error(format!("could not write {}: {error}", manifest.display())))?;
    }
    Ok(())
}

fn find_cargo_manifests(directory: &Path, manifests: &mut Vec<PathBuf>) -> Result<(), RunError> {
    for entry in fs::read_dir(directory)
        .map_err(|error| run_error(format!("could not read {}: {error}", directory.display())))?
    {
        let entry =
            entry.map_err(|error| run_error(format!("could not read workspace entry: {error}")))?;
        let path = entry.path();
        if entry.file_name() == "target" {
            continue;
        }
        if path.file_name().is_some_and(|name| name == "Cargo.toml") {
            manifests.push(path);
        } else if path.is_dir() {
            find_cargo_manifests(&path, manifests)?;
        }
    }
    Ok(())
}

fn rewrite_manifest_paths(
    value: &mut toml::Value,
    workspace: &Workspace,
    destination: &Path,
) -> Result<(), RunError> {
    rewrite_manifest_paths_in_section(value, workspace, destination, &[])
}

fn rewrite_manifest_paths_in_section(
    value: &mut toml::Value,
    workspace: &Workspace,
    destination: &Path,
    sections: &[String],
) -> Result<(), RunError> {
    match value {
        toml::Value::Table(table) => {
            for (name, value) in table.iter_mut() {
                if name == "path" && is_cargo_dependency_path(sections) {
                    rewrite_absolute_path(value, workspace, destination)?;
                }
                let mut nested_sections = sections.to_vec();
                nested_sections.push(name.to_owned());
                rewrite_manifest_paths_in_section(value, workspace, destination, &nested_sections)?;
            }
        }
        toml::Value::Array(values) => {
            for value in values {
                rewrite_manifest_paths_in_section(value, workspace, destination, sections)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn rewrite_absolute_path(
    value: &mut toml::Value,
    workspace: &Workspace,
    destination: &Path,
) -> Result<(), RunError> {
    if let toml::Value::String(path) = value {
        if Path::new(path).is_absolute() {
            let original = Path::new(path);
            let original = fs::canonicalize(original).map_err(|error| {
                run_error(format!(
                    "could not resolve absolute Cargo path {}: {error}",
                    original.display()
                ))
            })?;
            if !workspace
                .copy_paths
                .iter()
                .any(|root| original.starts_with(root))
            {
                return Err(run_error(format!(
                    "could not isolate absolute Cargo path: {}",
                    original.display()
                )));
            }
            *path = copied_path(workspace, destination, &original)?
                .display()
                .to_string();
        }
    }
    Ok(())
}

fn copied_path(
    workspace: &Workspace,
    destination: &Path,
    source: &Path,
) -> Result<PathBuf, RunError> {
    let relative = source.strip_prefix(&workspace.layout_root).map_err(|_| {
        run_error(format!(
            "path is outside the isolated workspace layout: {}",
            source.display()
        ))
    })?;
    Ok(destination.join(relative))
}

fn copy_cargo_configurations(workspace: &Workspace, destination: &Path) -> Result<(), RunError> {
    let mut copied_directories = BTreeSet::new();
    for configuration in &workspace.configurations {
        let configuration_directory = configuration
            .parent()
            .expect("configuration must have a parent");
        if !copied_directories.insert(configuration_directory) {
            continue;
        }
        let relative = configuration_directory
            .strip_prefix(&workspace.layout_root)
            .map_err(|_| {
                run_error(format!(
                    "Cargo configuration is outside the isolated workspace layout: {}",
                    configuration.display()
                ))
            })?;
        let copied = destination.join(relative);
        let parent = copied
            .parent()
            .expect("configuration directory must have a parent");
        fs::create_dir_all(parent).map_err(|error| {
            run_error(format!("could not create {}: {error}", parent.display()))
        })?;
        if !copied.exists() {
            fs::create_dir(&copied).map_err(|error| {
                run_error(format!("could not create {}: {error}", copied.display()))
            })?;
        }
        copy_cargo_configuration_directory(configuration_directory, &copied)?;
    }
    Ok(())
}

fn copy_cargo_configuration_directory(source: &Path, destination: &Path) -> Result<(), RunError> {
    for entry in fs::read_dir(source)
        .map_err(|error| run_error(format!("could not read {}: {error}", source.display())))?
    {
        let entry = entry.map_err(|error| {
            run_error(format!("could not read Cargo configuration entry: {error}"))
        })?;
        let name = entry.file_name();
        if name == "credentials" || name == "credentials.toml" {
            continue;
        }
        copy_entry(&entry.path(), &destination.join(name))?;
    }
    Ok(())
}

fn rewrite_cargo_configurations(workspace: &Workspace, destination: &Path) -> Result<(), RunError> {
    for configuration in &workspace.configurations {
        let copied = copied_path(workspace, destination, configuration)?;
        let text = fs::read_to_string(&copied).map_err(|error| {
            run_error(format!(
                "could not read Cargo configuration {}: {error}",
                copied.display()
            ))
        })?;
        let mut value = text.parse::<toml::Value>().map_err(|error| {
            run_error(format!(
                "could not parse Cargo configuration {}: {error}",
                copied.display()
            ))
        })?;
        rewrite_configuration_paths(&mut value, workspace, destination)?;
        fs::write(
            &copied,
            toml::to_string(&value).expect("Cargo configuration value must serialize"),
        )
        .map_err(|error| {
            run_error(format!(
                "could not write Cargo configuration {}: {error}",
                copied.display()
            ))
        })?;
    }
    Ok(())
}

fn rewrite_configuration_paths(
    value: &mut toml::Value,
    workspace: &Workspace,
    destination: &Path,
) -> Result<(), RunError> {
    let Some(table) = value.as_table_mut() else {
        return Ok(());
    };
    rewrite_configuration_values(table.get_mut("paths"), workspace, destination)?;
    if let Some(build) = table.get_mut("build").and_then(toml::Value::as_table_mut) {
        rewrite_configuration_values(build.get_mut("target"), workspace, destination)?;
    }
    if let Some(target) = table.get_mut("target").and_then(toml::Value::as_table_mut) {
        for (_, settings) in target.iter_mut() {
            if let Some(settings) = settings.as_table_mut() {
                rewrite_configuration_values(settings.get_mut("runner"), workspace, destination)?;
                rewrite_configuration_values(settings.get_mut("linker"), workspace, destination)?;
            }
        }
    }
    Ok(())
}

fn rewrite_configuration_values(
    value: Option<&mut toml::Value>,
    workspace: &Workspace,
    destination: &Path,
) -> Result<(), RunError> {
    let Some(value) = value else {
        return Ok(());
    };
    if value.is_str() {
        rewrite_absolute_path(value, workspace, destination)?;
    } else if let Some(values) = value.as_array_mut() {
        for value in values {
            rewrite_absolute_path(value, workspace, destination)?;
        }
    }
    Ok(())
}

fn copy_directory(source: &Path, destination: &Path) -> Result<(), RunError> {
    for entry in fs::read_dir(source)
        .map_err(|error| run_error(format!("could not read {}: {error}", source.display())))?
    {
        let entry =
            entry.map_err(|error| run_error(format!("could not read workspace entry: {error}")))?;
        let name = entry.file_name();
        if skip_workspace_entry(&name) {
            continue;
        }
        copy_entry(&entry.path(), &destination.join(name))?;
    }
    Ok(())
}

fn skip_workspace_entry(name: &std::ffi::OsStr) -> bool {
    name == ".cargo" || name == ".git" || name == "target"
}

fn copy_entry(source: &Path, destination: &Path) -> Result<(), RunError> {
    let file_type = fs::symlink_metadata(source)
        .map_err(|error| run_error(format!("could not inspect {}: {error}", source.display())))?
        .file_type();
    if file_type.is_symlink() {
        return Err(run_error(format!(
            "could not copy symbolic link in workspace: {}",
            source.display()
        )));
    }
    if file_type.is_dir() {
        fs::create_dir(destination).map_err(|error| {
            run_error(format!(
                "could not create {}: {error}",
                destination.display()
            ))
        })?;
        copy_directory(source, destination)
    } else if file_type.is_file() {
        fs::copy(source, destination)
            .map(|_| ())
            .map_err(|error| run_error(format!("could not copy {}: {error}", source.display())))
    } else {
        Err(run_error(format!(
            "could not copy unsupported workspace entry: {}",
            source.display()
        )))
    }
}

struct MutationCandidate {
    workspace: Workspace,
    source: PathBuf,
    mutator: String,
    mutation: Mutation,
}

#[derive(Clone)]
struct Workspace {
    root: PathBuf,
    manifest: PathBuf,
    layout_root: PathBuf,
    copy_paths: Vec<PathBuf>,
    configurations: Vec<PathBuf>,
}

struct TemporaryWorkspace {
    path: PathBuf,
}

impl TemporaryWorkspace {
    fn create() -> Result<Self, RunError> {
        for _ in 0..100 {
            let path = temporary_workspace_path();
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self { path }),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(run_error(format!(
                        "could not create temporary workspace {}: {error}",
                        path.display()
                    )));
                }
            }
        }
        Err(run_error("could not create a unique temporary workspace"))
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TemporaryWorkspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn temporary_workspace_path() -> PathBuf {
    let id = NEXT_TEMPORARY_WORKSPACE.fetch_add(1, Ordering::Relaxed);
    env::temp_dir().join(format!("mutarust-{}-{id}", std::process::id()))
}

fn run_error(message: impl Into<String>) -> RunError {
    RunError {
        message: message.into(),
    }
}
