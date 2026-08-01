use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use cargo_metadata::{Metadata, MetadataCommand};

#[cfg(any(unix, windows))]
use std::sync::atomic::AtomicBool;

use crate::evidence::{MutationEvidence, StableMutantId, mutation_evidence};
use crate::filter::{SourceFilter, SourceFilters};
use crate::{Mutation, Mutator, Registry, SourceError, find_rust_sources};

static NEXT_TEMPORARY_WORKSPACE: AtomicU64 = AtomicU64::new(0);
static MUTATION_RUN_LOCK: Mutex<()> = Mutex::new(());
#[cfg(any(unix, windows))]
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

    /// Returns the number of generated mutants.
    pub fn total(&self) -> usize {
        self.results.len()
    }

    /// Returns the total mutation score as a ratio from zero to one.
    pub fn mutation_score(&self) -> f64 {
        let scored = self.killed() + self.errored() + self.skipped();
        if self.total() == 0 {
            0.0
        } else {
            scored as f64 / self.total() as f64
        }
    }

    /// Returns sorted result counts for each mutator.
    pub fn mutator_summaries(&self) -> Vec<MutatorSummary> {
        let mut summaries = BTreeMap::new();
        for result in &self.results {
            if result.state == MutationState::NotCovered {
                continue;
            }
            let summary = summaries
                .entry(result.mutator.clone())
                .or_insert_with(|| MutatorSummary::new(&result.mutator));
            summary.record(result.state);
        }
        summaries.into_values().collect()
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
    /// The source file name relative to the isolated workspace layout.
    pub source: PathBuf,
    /// The stable ID for this source change.
    pub stable_id: String,
    /// The stable name of the mutator that produced this mutant.
    pub mutator: String,
    /// The unified source diff for this mutant.
    pub diff: String,
    /// The mutation test result state.
    pub state: MutationState,
    /// The error detail when Mutarust could not complete the test run.
    pub error: Option<String>,
}

/// Sorted result counts for one mutator.
pub struct MutatorSummary {
    /// The stable name of the mutator.
    pub mutator: String,
    /// The count of killed and errored mutants.
    pub killed: usize,
    /// The count of escaped mutants.
    pub escaped: usize,
    /// The count of skipped mutants.
    pub skipped: usize,
    /// The count of all mutants except not-covered mutants.
    pub total: usize,
}

impl MutatorSummary {
    fn new(mutator: &str) -> Self {
        Self {
            mutator: mutator.to_owned(),
            killed: 0,
            escaped: 0,
            skipped: 0,
            total: 0,
        }
    }

    fn record(&mut self, state: MutationState) {
        self.total += 1;
        match state {
            MutationState::Killed | MutationState::Errored => self.killed += 1,
            MutationState::Escaped => self.escaped += 1,
            MutationState::Skipped => self.skipped += 1,
            MutationState::NotCovered => {}
        }
    }
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

/// Test-command settings for one mutation run.
#[derive(Clone, Debug)]
pub struct TestExecution {
    command: TestCommand,
    recursive: bool,
    verbose: bool,
    debug: bool,
}

impl TestExecution {
    /// Uses the built-in Cargo test command.
    pub fn cargo() -> Self {
        Self {
            command: TestCommand::Cargo,
            recursive: false,
            verbose: false,
            debug: false,
        }
    }

    /// Uses a shell-quoted custom command for each mutant.
    pub fn custom(
        command: &str,
        recursive: bool,
        verbose: bool,
        debug: bool,
    ) -> Result<Self, RunError> {
        Ok(Self {
            command: TestCommand::Custom(CustomCommand::parse(command)?),
            recursive,
            verbose,
            debug,
        })
    }
}

#[derive(Clone, Debug)]
enum TestCommand {
    Cargo,
    Custom(CustomCommand),
}

#[derive(Clone, Debug)]
struct CustomCommand {
    arguments: Vec<String>,
}

impl CustomCommand {
    fn parse(command: &str) -> Result<Self, RunError> {
        let arguments = shell_words::split(command)
            .map_err(|error| run_error(format!("could not parse custom command: {error}")))?;
        let Some(program) = arguments.first() else {
            return Err(run_error("custom command must not be empty"));
        };
        validate_custom_program(program)?;
        Ok(Self { arguments })
    }
}

fn validate_custom_program(program: &str) -> Result<(), RunError> {
    let path = Path::new(program);
    let available = if path.components().count() > 1 {
        custom_program_is_available(path)
    } else {
        env::var_os("PATH").is_some_and(|paths| {
            env::split_paths(&paths)
                .any(|directory| custom_program_is_available(&directory.join(program)))
        })
    };
    if available {
        Ok(())
    } else {
        Err(run_error(format!(
            "could not run custom command: program not found: {program}"
        )))
    }
}

fn custom_program_is_available(path: &Path) -> bool {
    if path.is_file() {
        return custom_program_is_executable(path);
    }
    #[cfg(windows)]
    {
        return path.extension().is_none() && windows_path_program_is_available(path);
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(unix)]
fn custom_program_is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    fs::metadata(path).is_ok_and(|metadata| metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn custom_program_is_executable(_path: &Path) -> bool {
    true
}

#[cfg(windows)]
fn windows_path_program_is_available(path: &Path) -> bool {
    let extensions =
        env::var_os("PATHEXT").unwrap_or_else(|| std::ffi::OsString::from(".COM;.EXE;.BAT;.CMD"));
    extensions
        .to_string_lossy()
        .split(';')
        .map(str::trim)
        .filter(|extension| !extension.is_empty())
        .map(|extension| path.with_extension(extension.trim_start_matches('.')))
        .any(|path| path.is_file())
}

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
    run_mutation_tests_with_timeout_for_mutant(targets, registry, timeout, None)
}

/// Runs all mutants, or one stable mutant ID, with a fixed test timeout.
pub fn run_mutation_tests_with_timeout_for_mutant(
    targets: &[String],
    registry: &Registry,
    timeout: Duration,
    stable_id: Option<&str>,
) -> Result<MutationRun, RunError> {
    let names = registry.names().map(str::to_owned).collect::<Vec<_>>();
    let filters = SourceFilters::new(&[], &[], None, &names).map_err(run_error)?;
    run_mutation_tests_with_timeout_for_mutant_and_filters(
        targets, registry, timeout, stable_id, &filters,
    )
}

/// Runs mutants in the selected source scope with a fixed test timeout.
pub fn run_mutation_tests_with_timeout_for_mutant_and_filters(
    targets: &[String],
    registry: &Registry,
    timeout: Duration,
    stable_id: Option<&str>,
    filters: &SourceFilters,
) -> Result<MutationRun, RunError> {
    run_mutation_tests_with_test_execution(
        targets,
        registry,
        timeout,
        stable_id,
        filters,
        &TestExecution::cargo(),
    )
}

/// Runs mutants with the selected test command and source scope.
pub fn run_mutation_tests_with_test_execution(
    targets: &[String],
    registry: &Registry,
    timeout: Duration,
    stable_id: Option<&str>,
    filters: &SourceFilters,
    execution: &TestExecution,
) -> Result<MutationRun, RunError> {
    let _run_lock = MUTATION_RUN_LOCK
        .lock()
        .map_err(|_| run_error("could not start mutation run after a previous panic"))?;
    let _interrupt_guard = prepare_interrupt_handling()?;
    let plan = selected_mutation_plan(mutation_plan(targets, registry, filters)?, stable_id)?;
    stop_if_interrupted()?;
    test_clean_workspaces(&plan.workspaces, timeout)?;
    let mut results = Vec::new();
    for candidate in plan.candidates {
        if mutation_run_was_interrupted() {
            return Err(run_error("mutation run interrupted"));
        }
        results.push(test_candidate(candidate, timeout, execution));
        if mutation_run_was_interrupted() {
            return Err(run_error("mutation run interrupted"));
        }
    }
    Ok(MutationRun { results })
}

fn selected_mutation_plan(
    plan: MutationPlan,
    stable_id: Option<&str>,
) -> Result<MutationPlan, RunError> {
    let Some(stable_id) = stable_id else {
        return Ok(plan);
    };
    let stable_id = StableMutantId::parse(stable_id).ok_or_else(|| {
        run_error("mutant ID must be a 32-character lower-case hexadecimal value")
    })?;
    let candidates = plan
        .candidates
        .into_iter()
        .filter(|candidate| candidate.evidence.stable_id == stable_id)
        .collect::<Vec<_>>();
    match candidates.len() {
        0 => Err(run_error(format!(
            "could not find mutant ID {}",
            stable_id.as_str()
        ))),
        1 => Ok(MutationPlan {
            workspaces: candidates
                .iter()
                .map(|candidate| candidate.workspace.clone())
                .collect(),
            candidates,
        }),
        _ => Err(run_error(format!(
            "mutant ID {} identifies more than one mutant",
            stable_id.as_str()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{MutationResult, MutationRun, MutationState};

    #[test]
    fn mutation_score_uses_the_parity_states() {
        let run = MutationRun {
            results: [
                MutationState::Killed,
                MutationState::Escaped,
                MutationState::Errored,
                MutationState::NotCovered,
                MutationState::Skipped,
            ]
            .into_iter()
            .map(test_result)
            .collect(),
        };

        assert_eq!(run.mutation_score(), 3.0 / 5.0);
        let summaries = run.mutator_summaries();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].killed, 2);
        assert_eq!(summaries[0].escaped, 1);
        assert_eq!(summaries[0].skipped, 1);
        assert_eq!(summaries[0].total, 4);
    }

    fn test_result(state: MutationState) -> MutationResult {
        MutationResult {
            source: PathBuf::from("src/lib.rs"),
            stable_id: "a".repeat(32),
            mutator: "conditional/bool-literal".to_owned(),
            diff: String::new(),
            state,
            error: None,
        }
    }
}

fn test_clean_workspaces(workspaces: &[Workspace], timeout: Duration) -> Result<(), RunError> {
    let mut tested_manifests = BTreeSet::new();
    for workspace in workspaces {
        stop_if_interrupted()?;
        if !tested_manifests.insert(workspace.manifest.clone()) {
            continue;
        }
        test_clean_workspace(workspace, timeout)?;
    }
    Ok(())
}

fn test_clean_workspace(workspace: &Workspace, timeout: Duration) -> Result<(), RunError> {
    let temporary = TemporaryWorkspace::create()?;
    let result = (|| {
        let copied_workspace = copy_workspace(workspace, temporary.path())?;
        match run_cargo_command(
            temporary.path(),
            &copied_workspace,
            workspace,
            CargoAction::Test,
            timeout,
        )? {
            CargoOutcome::Passed => Ok(()),
            CargoOutcome::Failed(detail) => Err(run_error(format!(
                "clean cargo test failed: {}",
                cargo_detail(detail)
            ))),
            CargoOutcome::TimedOut => Err(run_error(format!(
                "clean cargo test timed out after {} seconds",
                timeout.as_secs()
            ))),
        }
    })();
    temporary.finish(result)
}

#[cfg(unix)]
struct InterruptGuard {
    previous: libc::sighandler_t,
}

#[cfg(unix)]
impl Drop for InterruptGuard {
    fn drop(&mut self) {
        unsafe {
            libc::signal(libc::SIGINT, self.previous);
        }
    }
}

#[cfg(unix)]
fn prepare_interrupt_handling() -> Result<InterruptGuard, RunError> {
    MUTATION_RUN_INTERRUPTED.store(false, Ordering::SeqCst);
    let handler = record_interrupt as *const () as usize;
    let previous = unsafe { libc::signal(libc::SIGINT, handler) };
    if previous == libc::SIG_ERR {
        return Err(run_error(format!(
            "could not handle mutation run interrupts: {}",
            std::io::Error::last_os_error()
        )));
    }
    Ok(InterruptGuard { previous })
}

#[cfg(windows)]
struct InterruptGuard;

#[cfg(windows)]
impl Drop for InterruptGuard {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::System::Console::SetConsoleCtrlHandler(
                Some(record_console_interrupt),
                0,
            );
        }
    }
}

#[cfg(windows)]
fn prepare_interrupt_handling() -> Result<InterruptGuard, RunError> {
    use windows_sys::Win32::System::Console::SetConsoleCtrlHandler;

    MUTATION_RUN_INTERRUPTED.store(false, Ordering::SeqCst);
    let installed = unsafe { SetConsoleCtrlHandler(Some(record_console_interrupt), 1) };
    if installed == 0 {
        return Err(run_error(format!(
            "could not handle mutation run interrupts: {}",
            std::io::Error::last_os_error()
        )));
    }
    Ok(InterruptGuard)
}

#[cfg(not(any(unix, windows)))]
struct InterruptGuard;

#[cfg(not(any(unix, windows)))]
fn prepare_interrupt_handling() -> Result<InterruptGuard, RunError> {
    Err(run_error(
        "mutation run interrupts are not supported on this platform",
    ))
}

#[cfg(unix)]
extern "C" fn record_interrupt(_: libc::c_int) {
    MUTATION_RUN_INTERRUPTED.store(true, Ordering::SeqCst);
}

#[cfg(windows)]
unsafe extern "system" fn record_console_interrupt(control_type: u32) -> i32 {
    use windows_sys::Win32::System::Console::{CTRL_BREAK_EVENT, CTRL_C_EVENT};

    if control_type == CTRL_C_EVENT || control_type == CTRL_BREAK_EVENT {
        MUTATION_RUN_INTERRUPTED.store(true, Ordering::SeqCst);
        1
    } else {
        0
    }
}

#[cfg(any(unix, windows))]
fn mutation_run_was_interrupted() -> bool {
    MUTATION_RUN_INTERRUPTED.load(Ordering::SeqCst)
}

#[cfg(not(any(unix, windows)))]
fn mutation_run_was_interrupted() -> bool {
    false
}

fn stop_if_interrupted() -> Result<(), RunError> {
    if mutation_run_was_interrupted() {
        Err(run_error("mutation run interrupted"))
    } else {
        Ok(())
    }
}

fn mutation_plan(
    targets: &[String],
    registry: &Registry,
    filters: &SourceFilters,
) -> Result<MutationPlan, RunError> {
    let sources = find_rust_sources(targets).map_err(source_error)?;
    if sources.is_empty() {
        return Err(run_error(
            "could not find any suitable Rust production source files",
        ));
    }
    let mut workspaces = Vec::new();
    let mut candidates = Vec::new();
    for source in sources {
        let source = fs::canonicalize(&source).map_err(|error| {
            run_error(format!("could not resolve {}: {error}", source.display()))
        })?;
        if !filters.allows_source_before_workspace(&source) {
            continue;
        }
        let workspace = workspace_for(&source)?;
        if !filters.allows_source(&source, &workspace.source_root) {
            continue;
        }
        let text = fs::read_to_string(&source)
            .map_err(|error| run_error(format!("could not read {}: {error}", source.display())))?;
        let source_filter = filters.for_source(&source, &text).map_err(run_error)?;
        workspaces.push(workspace.clone());
        add_source_candidates(
            &mut candidates,
            registry,
            &workspace,
            &source,
            &text,
            &source_filter,
        )?;
    }
    deduplicate_candidates(&mut candidates);
    Ok(MutationPlan {
        workspaces,
        candidates,
    })
}

fn deduplicate_candidates(candidates: &mut Vec<MutationCandidate>) {
    let mut seen = BTreeSet::new();
    candidates.retain(|candidate| {
        let (range, replacement) = candidate.mutation.identity();
        seen.insert((
            candidate.source.clone(),
            candidate.mutator.clone(),
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
    let package = package_for(&metadata, source)?;
    let source_root = common_ancestor(&[root.clone(), source.to_path_buf()])?;
    let (configurations, cargo_home) = cargo_configurations(&root)?;
    let CopyPaths {
        mut roots,
        manifests,
        build_targets,
    } = copy_paths_for(&metadata, &root, source)?;
    roots.extend(configuration_paths(&configurations)?);
    let copy_paths = copy_roots(roots.into_iter().collect());
    let excluded_copy_roots = excluded_copy_roots(&manifests, &build_targets)?;
    let mut layout_paths = copy_paths.clone();
    layout_paths.extend(configurations.iter().cloned());
    let layout_root = common_ancestor(&layout_paths)?;
    Ok(Workspace {
        root,
        source_root,
        manifest: package.manifest,
        package_name: package.name,
        layout_root,
        copy_paths,
        configurations,
        cargo_home,
        excluded_copy_roots,
        manifests,
    })
}

fn excluded_copy_roots(
    manifests: &[PathBuf],
    build_targets: &[PathBuf],
) -> Result<Vec<PathBuf>, RunError> {
    let mut excluded = BTreeSet::new();
    for build_target in build_targets.iter().filter(|path| path.exists()) {
        excluded.insert(fs::canonicalize(build_target).map_err(|error| {
            run_error(format!(
                "could not resolve Cargo target directory {}: {error}",
                build_target.display()
            ))
        })?);
    }
    for root in manifests.iter().filter_map(|manifest| manifest.parent()) {
        for name in [".cargo", ".git"] {
            let path = root.join(name);
            if path.exists() {
                excluded.insert(fs::canonicalize(&path).map_err(|error| {
                    run_error(format!("could not resolve {}: {error}", path.display()))
                })?);
            }
        }
    }
    Ok(excluded.into_iter().collect())
}

fn metadata_target_directory(metadata: &Metadata) -> Result<PathBuf, RunError> {
    let target = metadata.target_directory.as_std_path();
    if target.exists() {
        fs::canonicalize(target)
            .map_err(|error| run_error(format!("could not resolve {}: {error}", target.display())))
    } else {
        Ok(target.to_path_buf())
    }
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

fn package_for(metadata: &Metadata, source: &Path) -> Result<CargoPackage, RunError> {
    let package = metadata
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
            Some((source_root, package))
        })
        .max_by_key(|(source_root, _)| source_root.components().count())
        .map(|(_, package)| package)
        .ok_or_else(|| {
            run_error(format!(
                "could not find the Cargo package that owns {}",
                source.display()
            ))
        })?;
    let manifest = fs::canonicalize(package.manifest_path.as_std_path()).map_err(|error| {
        run_error(format!(
            "could not resolve {}: {error}",
            package.manifest_path
        ))
    })?;
    Ok(CargoPackage {
        manifest,
        name: package.name.to_string(),
    })
}

struct CargoPackage {
    manifest: PathBuf,
    name: String,
}

fn copy_paths_for(metadata: &Metadata, root: &Path, source: &Path) -> Result<CopyPaths, RunError> {
    let mut paths = BTreeSet::new();
    let mut manifests = BTreeSet::new();
    let mut build_targets = BTreeSet::new();
    paths.insert(root.to_path_buf());
    let root_manifest = root.join("Cargo.toml");
    if root_manifest.is_file() {
        manifests.insert(fs::canonicalize(&root_manifest).map_err(|error| {
            run_error(format!(
                "could not resolve {}: {error}",
                root_manifest.display()
            ))
        })?);
    }
    let mut dependency_paths = Vec::new();
    add_metadata_paths(
        &mut paths,
        &mut dependency_paths,
        &mut manifests,
        &mut build_targets,
        metadata,
    )?;
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
        add_metadata_paths(
            &mut paths,
            &mut dependency_paths,
            &mut manifests,
            &mut build_targets,
            &dependency_metadata,
        )?;
    }
    add_manifest_paths(&mut paths, &mut manifests)?;
    paths.insert(source.to_path_buf());
    Ok(CopyPaths {
        roots: copy_roots(paths),
        manifests: manifests.into_iter().collect(),
        build_targets: build_targets.into_iter().collect(),
    })
}

struct CopyPaths {
    roots: Vec<PathBuf>,
    manifests: Vec<PathBuf>,
    build_targets: Vec<PathBuf>,
}

fn add_manifest_paths(
    paths: &mut BTreeSet<PathBuf>,
    manifests: &mut BTreeSet<PathBuf>,
) -> Result<(), RunError> {
    let mut inspected = BTreeSet::new();
    loop {
        let discovered = paths
            .iter()
            .map(|path| path.join("Cargo.toml"))
            .filter(|path| path.is_file() && inspected.insert(path.clone()))
            .collect::<Vec<_>>();
        if discovered.is_empty() {
            return Ok(());
        }
        for manifest in discovered {
            let manifest = fs::canonicalize(&manifest).map_err(|error| {
                run_error(format!("could not resolve {}: {error}", manifest.display()))
            })?;
            manifests.insert(manifest.clone());
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
                if is_manifest_path_value(name, sections) {
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

fn is_cargo_manifest_path(sections: &[String]) -> bool {
    is_cargo_dependency_path(sections) || is_cargo_target_path(sections)
}

fn is_manifest_path_value(name: &str, sections: &[String]) -> bool {
    (name == "path" && is_cargo_manifest_path(sections))
        || (name == "build" && sections == ["package"])
}

fn is_cargo_dependency_path(sections: &[String]) -> bool {
    match sections.first().map(String::as_str) {
        Some("dependencies" | "dev-dependencies" | "build-dependencies" | "patch" | "replace") => {
            true
        }
        Some("workspace") => sections
            .get(1)
            .is_some_and(|section| is_dependency_section(section)),
        Some("target") => sections
            .get(2)
            .is_some_and(|section| is_dependency_section(section)),
        _ => false,
    }
}

fn is_dependency_section(section: &str) -> bool {
    matches!(
        section,
        "dependencies" | "dev-dependencies" | "build-dependencies"
    )
}

fn is_cargo_target_path(sections: &[String]) -> bool {
    matches!(
        sections.first().map(String::as_str),
        Some("lib" | "bin" | "test" | "bench" | "example")
    )
}

fn add_metadata_paths(
    paths: &mut BTreeSet<PathBuf>,
    dependency_paths: &mut Vec<PathBuf>,
    manifests: &mut BTreeSet<PathBuf>,
    build_targets: &mut BTreeSet<PathBuf>,
    metadata: &Metadata,
) -> Result<(), RunError> {
    build_targets.insert(metadata_target_directory(metadata)?);
    for package in &metadata.packages {
        manifests.insert(
            fs::canonicalize(package.manifest_path.as_std_path()).map_err(|error| {
                run_error(format!(
                    "could not resolve {}: {error}",
                    package.manifest_path
                ))
            })?,
        );
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

fn cargo_configurations(root: &Path) -> Result<(Vec<PathBuf>, Option<PathBuf>), RunError> {
    let mut pending = root
        .ancestors()
        .filter_map(|directory| active_cargo_configuration(&directory.join(".cargo")))
        .collect::<Vec<_>>();
    let cargo_home = cargo_home_directory()?;
    let active_home = cargo_home.as_deref().and_then(active_cargo_configuration);
    let has_active_home = active_home.is_some();
    pending.extend(active_home);
    let mut seen = BTreeSet::new();
    let mut configurations = Vec::new();
    while let Some(configuration) = pending.pop() {
        let configuration = fs::canonicalize(&configuration).map_err(|error| {
            run_error(format!(
                "could not resolve Cargo configuration {}: {error}",
                configuration.display()
            ))
        })?;
        if !seen.insert(configuration.clone()) {
            continue;
        }
        let text = fs::read_to_string(&configuration).map_err(|error| {
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
        pending.extend(configuration_includes(&value, &configuration)?);
        configurations.push(configuration);
    }
    configurations.sort();
    Ok((configurations, cargo_home.filter(|_| has_active_home)))
}

fn active_cargo_configuration(directory: &Path) -> Option<PathBuf> {
    let extensionless = directory.join("config");
    if extensionless.is_file() {
        return Some(extensionless);
    }
    let toml = directory.join("config.toml");
    toml.is_file().then_some(toml)
}

fn cargo_home_directory() -> Result<Option<PathBuf>, RunError> {
    let configured = env::var_os("CARGO_HOME").map(PathBuf::from);
    #[cfg(unix)]
    let default = || env::var_os("HOME").map(|home| PathBuf::from(home).join(".cargo"));
    #[cfg(windows)]
    let default = || env::var_os("USERPROFILE").map(|home| PathBuf::from(home).join(".cargo"));
    #[cfg(not(any(unix, windows)))]
    let default = || None;
    let Some(mut cargo_home) = configured.or_else(default) else {
        return Ok(None);
    };
    if cargo_home.is_relative() {
        cargo_home = env::current_dir()
            .map_err(|error| run_error(format!("could not read the current directory: {error}")))?
            .join(cargo_home);
    }
    if !cargo_home.is_dir() {
        return Ok(None);
    }
    fs::canonicalize(&cargo_home).map(Some).map_err(|error| {
        run_error(format!(
            "could not resolve Cargo home {}: {error}",
            cargo_home.display()
        ))
    })
}

fn configuration_includes(
    value: &toml::Value,
    configuration: &Path,
) -> Result<Vec<PathBuf>, RunError> {
    let Some(values) = value
        .as_table()
        .and_then(|table| table.get("include"))
        .and_then(toml::Value::as_array)
    else {
        return Ok(Vec::new());
    };
    let directory = configuration
        .parent()
        .expect("Cargo configuration must have a parent");
    let mut includes = Vec::new();
    for value in values {
        let (path, optional) = if let Some(path) = value.as_str() {
            (path, false)
        } else if let Some(table) = value.as_table() {
            let path = table
                .get("path")
                .and_then(toml::Value::as_str)
                .ok_or_else(|| {
                    run_error(format!(
                        "Cargo configuration include has no path in {}",
                        configuration.display()
                    ))
                })?;
            let optional = table
                .get("optional")
                .and_then(toml::Value::as_bool)
                .unwrap_or(false);
            (path, optional)
        } else {
            return Err(run_error(format!(
                "Cargo configuration include is invalid in {}",
                configuration.display()
            )));
        };
        let path = directory.join(path);
        if path.is_file() {
            includes.push(path);
        } else if !optional {
            return Err(run_error(format!(
                "Cargo configuration include does not exist: {}",
                path.display()
            )));
        }
    }
    Ok(includes)
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
        let base = configuration
            .parent()
            .and_then(Path::parent)
            .ok_or_else(|| {
                run_error(format!(
                    "Cargo configuration has no value base: {}",
                    configuration.display()
                ))
            })?;
        let dependency_base = configuration
            .parent()
            .expect("Cargo configuration must have a parent");
        collect_configuration_paths(&value, base, dependency_base, &mut paths)?;
    }
    Ok(paths.into_iter().collect())
}

fn collect_configuration_paths(
    value: &toml::Value,
    directory: &Path,
    dependency_directory: &Path,
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
            add_configuration_executable(settings.get("runner"), directory, paths)?;
            add_configuration_executable(settings.get("linker"), directory, paths)?;
        }
    }
    add_configuration_table_paths(table.get("patch"), dependency_directory, paths)?;
    add_configuration_table_paths(table.get("replace"), dependency_directory, paths)?;
    Ok(())
}

fn add_configuration_table_paths(
    value: Option<&toml::Value>,
    directory: &Path,
    paths: &mut BTreeSet<PathBuf>,
) -> Result<(), RunError> {
    let Some(table) = value.and_then(toml::Value::as_table) else {
        return Ok(());
    };
    add_configuration_values(table.get("path"), directory, paths)?;
    for child in table.values() {
        add_configuration_table_paths(Some(child), directory, paths)?;
    }
    Ok(())
}

fn add_configuration_executable(
    value: Option<&toml::Value>,
    directory: &Path,
    paths: &mut BTreeSet<PathBuf>,
) -> Result<(), RunError> {
    let Some(value) = value else {
        return Ok(());
    };
    if let Some(command) = value.as_str() {
        if let Some(program) = command.split_whitespace().next() {
            add_configuration_path(program, directory, paths)?;
        }
    } else if let Some(program) = value
        .as_array()
        .and_then(|values| values.first())
        .and_then(toml::Value::as_str)
    {
        add_configuration_path(program, directory, paths)?;
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
    filter: &SourceFilter,
) -> Result<(), RunError> {
    for name in registry.names() {
        let mutator = registry
            .get(name)
            .expect("registered mutator name must resolve to a mutator");
        add_mutator_candidates(candidates, workspace, source, name, mutator, text, filter)?;
    }
    Ok(())
}

fn add_mutator_candidates(
    candidates: &mut Vec<MutationCandidate>,
    workspace: &Workspace,
    source: &Path,
    name: &str,
    mutator: &dyn Mutator,
    text: &str,
    filter: &SourceFilter,
) -> Result<(), RunError> {
    for mutation in mutator.mutations(text) {
        let (range, _) = mutation.identity();
        if !filter.allows_mutation(name, &range) {
            continue;
        }
        let evidence = mutation_evidence(&workspace.source_root, source, name, &mutation, text)
            .map_err(run_error)?;
        candidates.push(MutationCandidate {
            workspace: workspace.clone(),
            source: source.to_path_buf(),
            mutator: name.to_owned(),
            mutation,
            evidence,
        });
    }
    Ok(())
}

fn test_candidate(
    candidate: MutationCandidate,
    timeout: Duration,
    execution: &TestExecution,
) -> MutationResult {
    let (state, error) = test_candidate_state(&candidate, timeout, execution)
        .unwrap_or_else(|error| (MutationState::Errored, Some(error.to_string())));
    MutationResult {
        source: candidate.evidence.source,
        stable_id: candidate.evidence.stable_id.into_string(),
        mutator: candidate.mutator,
        diff: candidate.evidence.diff,
        state,
        error,
    }
}

fn test_candidate_state(
    candidate: &MutationCandidate,
    timeout: Duration,
    execution: &TestExecution,
) -> Result<(MutationState, Option<String>), RunError> {
    stop_if_interrupted()?;
    let temporary = TemporaryWorkspace::create()?;
    let result = (|| {
        let workspace = copy_workspace(&candidate.workspace, temporary.path())?;
        match &execution.command {
            TestCommand::Cargo => {
                test_cargo_mutant(candidate, temporary.path(), &workspace, timeout)
            }
            TestCommand::Custom(command) => test_custom_mutant(
                candidate,
                temporary.path(),
                &workspace,
                command,
                execution,
                timeout,
            ),
        }
    })();
    temporary.finish(result)
}

fn test_cargo_mutant(
    candidate: &MutationCandidate,
    temporary: &Path,
    copied_workspace: &Path,
    timeout: Duration,
) -> Result<(MutationState, Option<String>), RunError> {
    write_mutant(&candidate.workspace, temporary, candidate)?;
    match run_cargo_command(
        temporary,
        copied_workspace,
        &candidate.workspace,
        CargoAction::Compile,
        timeout,
    )? {
        CargoOutcome::Passed => {
            test_compiled_mutant(temporary, copied_workspace, &candidate.workspace, timeout)
        }
        CargoOutcome::Failed(detail) => Ok((
            MutationState::Skipped,
            Some(format!("mutant did not compile: {}", cargo_detail(detail))),
        )),
        CargoOutcome::TimedOut => Ok((
            MutationState::Errored,
            Some(format!(
                "mutant compilation timed out after {} seconds",
                timeout.as_secs()
            )),
        )),
    }
}

fn test_custom_mutant(
    candidate: &MutationCandidate,
    temporary: &Path,
    copied_workspace: &Path,
    command: &CustomCommand,
    execution: &TestExecution,
    timeout: Duration,
) -> Result<(MutationState, Option<String>), RunError> {
    let sources = write_custom_mutant(&candidate.workspace, temporary, candidate)?;
    run_custom_command(
        command,
        copied_workspace,
        &candidate.workspace,
        &sources,
        execution,
        timeout,
    )
}

fn test_compiled_mutant(
    temporary: &Path,
    copied_workspace: &Path,
    workspace: &Workspace,
    timeout: Duration,
) -> Result<(MutationState, Option<String>), RunError> {
    match run_cargo_command(
        temporary,
        copied_workspace,
        workspace,
        CargoAction::Test,
        timeout,
    )? {
        CargoOutcome::Passed => Ok((MutationState::Escaped, None)),
        CargoOutcome::Failed(_) => Ok((MutationState::Killed, None)),
        CargoOutcome::TimedOut => Ok((
            MutationState::Errored,
            Some(format!(
                "cargo test timed out after {} seconds",
                timeout.as_secs()
            )),
        )),
    }
}

fn write_mutant(
    workspace: &Workspace,
    temporary: &Path,
    candidate: &MutationCandidate,
) -> Result<(), RunError> {
    let path = copied_path(workspace, temporary, &candidate.source)?;
    let source = fs::read_to_string(&path)
        .map_err(|error| run_error(format!("could not read {}: {error}", path.display())))?;
    let mutant = apply_candidate_mutation(candidate, &source)?;
    fs::write(&path, mutant)
        .map_err(|error| run_error(format!("could not write {}: {error}", path.display())))
}

struct CustomMutationSources {
    original: PathBuf,
    changed: PathBuf,
}

fn write_custom_mutant(
    workspace: &Workspace,
    temporary: &Path,
    candidate: &MutationCandidate,
) -> Result<CustomMutationSources, RunError> {
    let changed = copied_path(workspace, temporary, &candidate.source)?;
    let source = fs::read_to_string(&changed)
        .map_err(|error| run_error(format!("could not read {}: {error}", changed.display())))?;
    let relative = changed.strip_prefix(temporary).map_err(|_| {
        run_error(format!(
            "could not prepare custom command source {}",
            changed.display()
        ))
    })?;
    let original = temporary.join("original-source").join(relative);
    let parent = original
        .parent()
        .expect("original source must have a parent");
    fs::create_dir_all(parent)
        .map_err(|error| run_error(format!("could not create {}: {error}", parent.display())))?;
    fs::write(&original, &source)
        .map_err(|error| run_error(format!("could not write {}: {error}", original.display())))?;
    let mutant = apply_candidate_mutation(candidate, &source)?;
    fs::write(&changed, mutant)
        .map_err(|error| run_error(format!("could not write {}: {error}", changed.display())))?;
    Ok(CustomMutationSources { original, changed })
}

fn apply_candidate_mutation(
    candidate: &MutationCandidate,
    source: &str,
) -> Result<String, RunError> {
    candidate.mutation.apply(source).ok_or_else(|| {
        run_error(format!(
            "could not apply mutation to {}",
            candidate.source.display()
        ))
    })
}

#[derive(Clone, Copy)]
enum CargoAction {
    Compile,
    Test,
}

enum CargoOutcome {
    Passed,
    Failed(String),
    TimedOut,
}

enum ProcessOutcome {
    Exited(std::process::ExitStatus),
    TimedOut,
}

struct ProcessChild {
    process: std::process::Child,
    active: bool,
}

impl ProcessChild {
    fn new(process: std::process::Child) -> Self {
        Self {
            process,
            active: true,
        }
    }

    fn try_wait(&mut self) -> Result<Option<std::process::ExitStatus>, RunError> {
        match self.process.try_wait() {
            Ok(Some(status)) => {
                self.active = false;
                Ok(Some(status))
            }
            Ok(None) => Ok(None),
            Err(error) => {
                let operation = run_error(format!("could not wait for test command: {error}"));
                match self.stop_and_reap() {
                    Ok(()) => Err(operation),
                    Err(cleanup) => Err(run_error(format!("{operation}; {cleanup}"))),
                }
            }
        }
    }

    fn stop_and_reap(&mut self) -> Result<(), RunError> {
        let stop_error = stop_process(&mut self.process).err();
        if stop_error.is_some() {
            let _ = self.process.kill();
        }
        let wait = self
            .process
            .wait()
            .map_err(|error| run_error(format!("could not reap test command: {error}")));
        if wait.is_ok() {
            self.active = false;
        }
        match (stop_error, wait) {
            (None, Ok(_)) => Ok(()),
            (Some(stop), Ok(_)) => Err(stop),
            (None, Err(wait)) => Err(wait),
            (Some(stop), Err(wait)) => Err(run_error(format!("{stop}; {wait}"))),
        }
    }
}

impl Drop for ProcessChild {
    fn drop(&mut self) {
        if self.active {
            let _ = stop_process(&mut self.process);
            let _ = self.process.kill();
            let _ = self.process.wait();
        }
    }
}

fn run_cargo_command(
    temporary: &Path,
    copied_workspace: &Path,
    workspace: &Workspace,
    action: CargoAction,
    timeout: Duration,
) -> Result<CargoOutcome, RunError> {
    let manifest = copied_path(workspace, temporary, &workspace.manifest)?;
    let diagnostic_path = temporary.join("cargo-stderr");
    let diagnostics = fs::File::create(&diagnostic_path).map_err(|error| {
        run_error(format!(
            "could not create Cargo diagnostic output {}: {error}",
            diagnostic_path.display()
        ))
    })?;
    let diagnostic_stdout = diagnostics.try_clone().map_err(|error| {
        run_error(format!(
            "could not prepare Cargo diagnostic output {}: {error}",
            diagnostic_path.display()
        ))
    })?;
    let cargo = env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut command = Command::new(cargo);
    command
        .args(["test", "--manifest-path"])
        .arg(manifest)
        .args(["--target-dir"])
        .arg(temporary.join("target"))
        .current_dir(copied_workspace)
        .env_remove("CARGO_TARGET_DIR")
        .stdout(Stdio::from(diagnostic_stdout))
        .stderr(Stdio::from(diagnostics));
    if let Some(cargo_home) = &workspace.cargo_home {
        command.env("CARGO_HOME", copied_path(workspace, temporary, cargo_home)?);
    }
    if matches!(action, CargoAction::Compile) {
        command.arg("--no-run");
    }
    stop_if_interrupted()?;
    configure_process_group(&mut command);
    let mut child = ProcessChild::new(
        command
            .spawn()
            .map_err(|error| run_error(format!("could not run cargo test: {error}")))?,
    );
    let outcome = match wait_for_process(&mut child, timeout)? {
        ProcessOutcome::Exited(status) if status.success() => CargoOutcome::Passed,
        ProcessOutcome::Exited(_) => {
            CargoOutcome::Failed(fs::read_to_string(&diagnostic_path).unwrap_or_default())
        }
        ProcessOutcome::TimedOut => CargoOutcome::TimedOut,
    };
    Ok(outcome)
}

fn run_custom_command(
    command: &CustomCommand,
    copied_workspace: &Path,
    workspace: &Workspace,
    sources: &CustomMutationSources,
    execution: &TestExecution,
    timeout: Duration,
) -> Result<(MutationState, Option<String>), RunError> {
    let mut process = Command::new(&command.arguments[0]);
    process
        .args(&command.arguments[1..])
        .current_dir(copied_workspace)
        .env("MUTATE_ORIGINAL", &sources.original)
        .env("MUTATE_CHANGED", &sources.changed)
        .env("MUTATE_PACKAGE", &workspace.package_name)
        .env("MUTATE_TIMEOUT", timeout.as_secs().to_string())
        .env("TEST_RECURSIVE", execution.recursive.to_string())
        .env("MUTATE_VERBOSE", execution.verbose.to_string())
        .env("MUTATE_DEBUG", execution.debug.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    stop_if_interrupted()?;
    configure_process_group(&mut process);
    let mut child = ProcessChild::new(
        process
            .spawn()
            .map_err(|error| run_error(format!("could not run custom command: {error}")))?,
    );
    match wait_for_process(&mut child, timeout)? {
        ProcessOutcome::TimedOut => Ok((
            MutationState::Errored,
            Some(format!(
                "custom command timed out after {} seconds",
                timeout.as_secs()
            )),
        )),
        ProcessOutcome::Exited(status) => Ok(custom_command_result(status)),
    }
}

fn custom_command_result(status: std::process::ExitStatus) -> (MutationState, Option<String>) {
    match status.code() {
        Some(0) => (MutationState::Killed, None),
        Some(1) => (MutationState::Escaped, None),
        Some(2) => (MutationState::Skipped, None),
        Some(code) => (
            MutationState::Errored,
            Some(format!("custom command exited with status {code}")),
        ),
        None => (
            MutationState::Errored,
            Some("custom command stopped without an exit status".to_owned()),
        ),
    }
}

fn wait_for_process(
    child: &mut ProcessChild,
    timeout: Duration,
) -> Result<ProcessOutcome, RunError> {
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(ProcessOutcome::Exited(status));
        }
        if mutation_run_was_interrupted() {
            child.stop_and_reap()?;
            return Err(run_error("mutation run interrupted"));
        }
        if started.elapsed() >= timeout {
            child.stop_and_reap()?;
            return Ok(ProcessOutcome::TimedOut);
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn cargo_detail(detail: String) -> String {
    let detail = detail.trim();
    if detail.is_empty() {
        "Cargo exited with a failure status".to_owned()
    } else {
        detail.to_owned()
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
fn stop_process(child: &mut std::process::Child) -> Result<(), RunError> {
    let process_group = i32::try_from(child.id())
        .map_err(|error| run_error(format!("could not identify cargo test: {error}")))?;
    let result = unsafe { libc::kill(-process_group, libc::SIGKILL) };
    if result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
        Ok(())
    } else {
        Err(run_error(format!(
            "could not stop cargo test: {}",
            std::io::Error::last_os_error()
        )))
    }
}

#[cfg(windows)]
fn stop_process(child: &mut std::process::Child) -> Result<(), RunError> {
    let identifier = child.id().to_string();
    let status = Command::new("taskkill")
        .args(["/PID", &identifier, "/T", "/F"])
        .status()
        .map_err(|error| run_error(format!("could not stop cargo test: {error}")))?;
    if status.success() {
        Ok(())
    } else {
        Err(run_error("could not stop cargo test"))
    }
}

#[cfg(all(not(unix), not(windows)))]
fn stop_process(child: &mut std::process::Child) -> Result<(), RunError> {
    child
        .kill()
        .map_err(|error| run_error(format!("could not stop cargo test: {error}")))
}

fn copy_workspace(workspace: &Workspace, destination: &Path) -> Result<PathBuf, RunError> {
    for source in &workspace.copy_paths {
        if workspace.excluded_copy_roots.contains(source) {
            continue;
        }
        let copied = copied_path(workspace, destination, source)?;
        let parent = copied.parent().expect("copied path must have a parent");
        fs::create_dir_all(parent).map_err(|error| {
            run_error(format!("could not create {}: {error}", parent.display()))
        })?;
        if copied.exists() {
            copy_directory(workspace, source, &copied)?;
        } else {
            copy_entry(workspace, source, &copied)?;
        }
    }
    copy_cargo_configurations(workspace, destination)?;
    rewrite_cargo_configurations(workspace, destination)?;
    rewrite_cargo_manifests(workspace, destination)?;
    copied_path(workspace, destination, &workspace.root)
}

fn rewrite_cargo_manifests(workspace: &Workspace, destination: &Path) -> Result<(), RunError> {
    for original in &workspace.manifests {
        let manifest = copied_path(workspace, destination, original)?;
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
                if is_manifest_path_value(name, sections) {
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
                && !workspace.configurations.contains(&original)
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
    for configuration in &workspace.configurations {
        let copied = copied_path(workspace, destination, configuration)?;
        let parent = copied
            .parent()
            .expect("Cargo configuration must have a parent");
        fs::create_dir_all(parent).map_err(|error| {
            run_error(format!("could not create {}: {error}", parent.display()))
        })?;
        fs::copy(configuration, &copied).map_err(|error| {
            run_error(format!(
                "could not copy Cargo configuration {}: {error}",
                configuration.display()
            ))
        })?;
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
    rewrite_configuration_includes(table.get_mut("include"), workspace, destination)?;
    rewrite_configuration_values(table.get_mut("paths"), workspace, destination)?;
    if let Some(build) = table.get_mut("build").and_then(toml::Value::as_table_mut) {
        rewrite_configuration_values(build.get_mut("target"), workspace, destination)?;
    }
    if let Some(target) = table.get_mut("target").and_then(toml::Value::as_table_mut) {
        for (_, settings) in target.iter_mut() {
            if let Some(settings) = settings.as_table_mut() {
                rewrite_configuration_executable(
                    settings.get_mut("runner"),
                    workspace,
                    destination,
                )?;
                rewrite_configuration_executable(
                    settings.get_mut("linker"),
                    workspace,
                    destination,
                )?;
            }
        }
    }
    rewrite_configuration_table_paths(table.get_mut("patch"), workspace, destination)?;
    rewrite_configuration_table_paths(table.get_mut("replace"), workspace, destination)?;
    Ok(())
}

fn rewrite_configuration_table_paths(
    value: Option<&mut toml::Value>,
    workspace: &Workspace,
    destination: &Path,
) -> Result<(), RunError> {
    let Some(table) = value.and_then(toml::Value::as_table_mut) else {
        return Ok(());
    };
    rewrite_configuration_values(table.get_mut("path"), workspace, destination)?;
    for (_, child) in table.iter_mut() {
        rewrite_configuration_table_paths(Some(child), workspace, destination)?;
    }
    Ok(())
}

fn rewrite_configuration_includes(
    value: Option<&mut toml::Value>,
    workspace: &Workspace,
    destination: &Path,
) -> Result<(), RunError> {
    let Some(values) = value.and_then(toml::Value::as_array_mut) else {
        return Ok(());
    };
    for value in values {
        if value.is_str() {
            rewrite_configuration_include(value, false, workspace, destination)?;
        } else if let Some(table) = value.as_table_mut() {
            let optional = table
                .get("optional")
                .and_then(toml::Value::as_bool)
                .unwrap_or(false);
            if let Some(path) = table.get_mut("path") {
                rewrite_configuration_include(path, optional, workspace, destination)?;
            }
        }
    }
    Ok(())
}

fn rewrite_configuration_include(
    value: &mut toml::Value,
    optional: bool,
    workspace: &Workspace,
    destination: &Path,
) -> Result<(), RunError> {
    let Some(path) = value.as_str() else {
        return Ok(());
    };
    let path = Path::new(path);
    if optional && path.is_absolute() && !path.exists() {
        *value = toml::Value::String(
            destination
                .join(".mutarust-missing-configuration-include")
                .display()
                .to_string(),
        );
        return Ok(());
    }
    rewrite_absolute_path(value, workspace, destination)
}

fn rewrite_configuration_executable(
    value: Option<&mut toml::Value>,
    workspace: &Workspace,
    destination: &Path,
) -> Result<(), RunError> {
    let Some(value) = value else {
        return Ok(());
    };
    if let Some(command) = value.as_str() {
        let program_length = command.find(char::is_whitespace).unwrap_or(command.len());
        let (program, arguments) = command.split_at(program_length);
        let mut rewritten = toml::Value::String(program.to_owned());
        rewrite_absolute_path(&mut rewritten, workspace, destination)?;
        if let Some(program) = rewritten.as_str() {
            *value = toml::Value::String(format!("{program}{arguments}"));
        }
    } else if let Some(program) = value.as_array_mut().and_then(|values| values.first_mut()) {
        rewrite_absolute_path(program, workspace, destination)?;
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

fn copy_directory(
    workspace: &Workspace,
    source: &Path,
    destination: &Path,
) -> Result<(), RunError> {
    for entry in fs::read_dir(source)
        .map_err(|error| run_error(format!("could not read {}: {error}", source.display())))?
    {
        stop_if_interrupted()?;
        let entry =
            entry.map_err(|error| run_error(format!("could not read workspace entry: {error}")))?;
        let source = entry.path();
        if workspace.excluded_copy_roots.contains(&source) {
            continue;
        }
        copy_entry(workspace, &source, &destination.join(entry.file_name()))?;
    }
    Ok(())
}

fn copy_entry(workspace: &Workspace, source: &Path, destination: &Path) -> Result<(), RunError> {
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
        copy_directory(workspace, source, destination)
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

struct MutationPlan {
    workspaces: Vec<Workspace>,
    candidates: Vec<MutationCandidate>,
}

struct MutationCandidate {
    workspace: Workspace,
    source: PathBuf,
    mutator: String,
    mutation: Mutation,
    evidence: MutationEvidence,
}

#[derive(Clone)]
struct Workspace {
    root: PathBuf,
    source_root: PathBuf,
    manifest: PathBuf,
    package_name: String,
    layout_root: PathBuf,
    copy_paths: Vec<PathBuf>,
    configurations: Vec<PathBuf>,
    cargo_home: Option<PathBuf>,
    excluded_copy_roots: Vec<PathBuf>,
    manifests: Vec<PathBuf>,
}

struct TemporaryWorkspace {
    path: Option<PathBuf>,
}

impl TemporaryWorkspace {
    fn create() -> Result<Self, RunError> {
        for _ in 0..100 {
            let path = temporary_workspace_path();
            match create_private_directory(&path) {
                Ok(()) => return Ok(Self { path: Some(path) }),
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
        self.path
            .as_deref()
            .expect("temporary workspace path must exist before cleanup")
    }

    fn finish<T>(mut self, result: Result<T, RunError>) -> Result<T, RunError> {
        let path = self.path().to_path_buf();
        match fs::remove_dir_all(&path) {
            Ok(()) => {
                self.path.take();
                result
            }
            Err(error) => {
                let cleanup = format!(
                    "could not remove temporary workspace {}: {error}",
                    path.display()
                );
                match result {
                    Ok(_) => Err(run_error(cleanup)),
                    Err(operation) => Err(run_error(format!("{operation}; {cleanup}"))),
                }
            }
        }
    }
}

impl Drop for TemporaryWorkspace {
    fn drop(&mut self) {
        if let Some(path) = &self.path {
            let _ = fs::remove_dir_all(path);
        }
    }
}

fn temporary_workspace_path() -> PathBuf {
    let id = NEXT_TEMPORARY_WORKSPACE.fetch_add(1, Ordering::Relaxed);
    env::temp_dir().join(format!("mutarust-{}-{id}", std::process::id()))
}

#[cfg(unix)]
fn create_private_directory(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;

    let mut builder = fs::DirBuilder::new();
    builder.mode(0o700).create(path)
}

#[cfg(not(unix))]
fn create_private_directory(path: &Path) -> std::io::Result<()> {
    fs::create_dir(path)
}

fn run_error(message: impl Into<String>) -> RunError {
    RunError {
        message: message.into(),
    }
}
