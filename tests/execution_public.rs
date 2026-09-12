#[cfg(any(unix, windows))]
use std::sync::atomic::{AtomicBool, Ordering};

static PUBLIC_RUN_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn public_run_guard() -> std::sync::MutexGuard<'static, ()> {
    PUBLIC_RUN_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

struct InvalidEdit;

impl mutarust::Mutator for InvalidEdit {
    fn name(&self) -> &str {
        "custom/invalid-edit"
    }

    fn mutations(&self, source: &str) -> Vec<mutarust::Mutation> {
        let mut mutations = (0..=source.len())
            .map(|offset| mutarust::Mutation::new(offset..offset, "}"))
            .collect::<Vec<_>>();
        let reversed_start = source.len().min(2);
        let reversed_end = reversed_start.saturating_sub(1);
        mutations.extend([
            mutarust::Mutation::new(0..1, ""),
            mutarust::Mutation::new(reversed_start..reversed_end, ""),
            mutarust::Mutation::new(source.len() + 1..source.len() + 1, ""),
            mutarust::Mutation::new(0..source.len(), "fn broken("),
        ]);
        mutations
    }
}

struct NoOpReplacement;

impl mutarust::Mutator for NoOpReplacement {
    fn name(&self) -> &str {
        "custom/no-op-replacement"
    }

    fn mutations(&self, _source: &str) -> Vec<mutarust::Mutation> {
        vec![mutarust::Mutation::new(0..0, "")]
    }
}

struct FixtureRoot(std::path::PathBuf);

impl Drop for FixtureRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct SlowReplacement;

impl mutarust::Mutator for SlowReplacement {
    fn name(&self) -> &str {
        "custom/slow-replacement"
    }

    fn mutations(&self, source: &str) -> Vec<mutarust::Mutation> {
        match source.find("false") {
            Some(offset) => vec![mutarust::Mutation::new(
                offset..offset + "false".len(),
                "true",
            )],
            None => Vec::new(),
        }
    }
}

struct WorkspaceSideEffect;

impl mutarust::Mutator for WorkspaceSideEffect {
    fn name(&self) -> &str {
        "custom/workspace-side-effect"
    }

    fn mutations(&self, source: &str) -> Vec<mutarust::Mutation> {
        let Some(first) = source.find("false") else {
            return Vec::new();
        };
        let second = source[first + "false".len()..]
            .find("false")
            .map(|offset| first + "false".len() + offset);
        let Some(second) = second else {
            return Vec::new();
        };
        [first, second]
            .into_iter()
            .map(|offset| mutarust::Mutation::new(offset..offset + "false".len(), "true"))
            .collect()
    }
}

fn unique_temporary_root(tag: &str) -> FixtureRoot {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time must follow the Unix epoch")
        .as_nanos();
    FixtureRoot(
        std::env::temp_dir().join(format!("mutarust-{tag}-{}-{unique}", std::process::id())),
    )
}

fn write_single_file_crate(root: &FixtureRoot, package: &str, source: &str) -> std::path::PathBuf {
    let source_path = root.0.join("src").join("lib.rs");
    std::fs::create_dir_all(source_path.parent().expect("source must have a parent"))
        .expect("fixture source directory must be created");
    std::fs::write(
        root.0.join("Cargo.toml"),
        format!("[package]\nname = \"{package}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n"),
    )
    .expect("fixture manifest must be written");
    std::fs::write(&source_path, source).expect("fixture source must be written");
    source_path
}

#[test]
fn dry_run_excludes_range_break_mutants_inside_cfg_test_modules() {
    let _run_guard = public_run_guard();
    let root = unique_temporary_root("range-filter");
    let source = write_single_file_crate(
        &root,
        "cfg-test-range-filter",
        "pub fn production() { for _ in 0..1 {} }\n#[cfg(test)]\nmod tests { fn helper() { for _ in 0..1 {} } }\n",
    );
    let mut registry = mutarust::Registry::builtins();
    registry.retain(|name| name == "loop/range_break");
    let names = registry.names().map(str::to_owned).collect::<Vec<_>>();
    let filters = mutarust::SourceFilters::with_policies(&[], &[], None, &names, false, false)
        .expect("source filters must accept the loop mutator");
    let controls = mutarust::ExecutionControls {
        dry_run: true,
        ..mutarust::ExecutionControls::default()
    };
    let execution = mutarust::TestExecution::custom("false", false, false, false)
        .expect("the dry-run command must parse");
    let run = mutarust::run_mutation_tests_with_controls(
        &[source.to_string_lossy().into_owned()],
        &registry,
        std::time::Duration::from_secs(1),
        None,
        &filters,
        &execution,
        &controls,
    )
    .expect("the dry run must complete");

    assert_eq!(run.results().len(), 1);
    assert_eq!(run.results()[0].line, 1);
}

#[test]
fn dry_run_excludes_range_break_mutants_inside_cfg_items() {
    let _run_guard = public_run_guard();
    let root = unique_temporary_root("range-filter");
    let source = write_single_file_crate(
        &root,
        "cfg-item-range-filter",
        "pub fn production() { for _ in 0..1 {} }\n#[cfg(feature = \"extra\")]\nfn gated() { for _ in 0..1 {} }\n",
    );
    let mut registry = mutarust::Registry::builtins();
    registry.retain(|name| name == "loop/range_break");
    let names = registry.names().map(str::to_owned).collect::<Vec<_>>();
    let filters = mutarust::SourceFilters::with_policies(&[], &[], None, &names, false, true)
        .expect("source filters must accept the loop mutator");
    let controls = mutarust::ExecutionControls {
        dry_run: true,
        ..mutarust::ExecutionControls::default()
    };
    let execution = mutarust::TestExecution::custom("false", false, false, false)
        .expect("the dry-run command must parse");
    let run = mutarust::run_mutation_tests_with_controls(
        &[source.to_string_lossy().into_owned()],
        &registry,
        std::time::Duration::from_secs(1),
        None,
        &filters,
        &execution,
        &controls,
    )
    .expect("the dry run must complete");

    assert_eq!(run.results().len(), 1);
    assert_eq!(run.results()[0].line, 1);
}

#[test]
fn timeout_coefficient_extends_the_clean_suite_budget() {
    let _run_guard = public_run_guard();
    let root = unique_temporary_root("slow-suite");
    let source = write_single_file_crate(
        &root,
        "slow-suite-fixture",
        "pub fn answer() -> bool {\n    false\n}\n",
    );
    std::fs::create_dir_all(root.0.join("tests")).expect("fixture tests directory must be created");
    std::fs::write(
        root.0.join("tests").join("suite.rs"),
        "#[test]\nfn detects_answer_change() {\n    std::thread::sleep(std::time::Duration::from_secs(4));\n    assert!(!slow_suite_fixture::answer());\n}\n",
    )
    .expect("fixture test must be written");

    let registry = mutarust::RegistryBuilder::new()
        .register(SlowReplacement)
        .expect("custom mutator must register")
        .build();
    let names = registry.names().map(str::to_owned).collect::<Vec<_>>();
    let filters = mutarust::SourceFilters::new(&[], &[], None, &names)
        .expect("source filters must accept the custom mutator");
    let controls = mutarust::ExecutionControls {
        timeout_coefficient: Some(8.0),
        ..mutarust::ExecutionControls::default()
    };
    let run = mutarust::run_mutation_tests_with_controls(
        &[source.to_string_lossy().into_owned()],
        &registry,
        std::time::Duration::from_secs(2),
        None,
        &filters,
        &mutarust::TestExecution::cargo(),
        &controls,
    )
    .expect("the clean suite must finish inside the coefficient budget");

    assert_eq!(run.results().len(), 1);
    assert_eq!(
        run.results()[0].state,
        mutarust::MutationState::Killed,
        "the mutant must be killed by the adaptive timeout, not the base timeout: {:?}",
        run.results()[0].diff
    );
}

#[cfg(unix)]
#[test]
fn timeout_coefficient_extends_the_coverage_budget() {
    use std::os::unix::fs::PermissionsExt;

    const COVERAGE_ENV: &str = "MUTARUST_SLOW_COVERAGE_SOURCE";
    if let Some(source) = std::env::var_os(COVERAGE_ENV) {
        let _run_guard = public_run_guard();
        let mut registry = mutarust::Registry::builtins();
        registry.retain(|name| name == "conditional/bool-literal");
        let names = registry.names().map(str::to_owned).collect::<Vec<_>>();
        let filters = mutarust::SourceFilters::new(&[], &[], None, &names)
            .expect("source filters must accept the boolean literal mutator");
        let controls = mutarust::ExecutionControls {
            timeout_coefficient: Some(4.0),
            coverage: mutarust::CoverageControls {
                enabled: true,
                per_test: false,
            },
            ..mutarust::ExecutionControls::default()
        };
        let run = mutarust::run_mutation_tests_with_controls(
            &[source.to_string_lossy().into_owned()],
            &registry,
            std::time::Duration::from_secs(2),
            None,
            &filters,
            &mutarust::TestExecution::cargo(),
            &controls,
        )
        .expect("coverage and the clean suite must finish inside the coefficient budget");

        assert!(
            run.has_coverage(),
            "the coverage command must collect a profile inside the coefficient budget"
        );
        let killed = run
            .results()
            .iter()
            .all(|result| result.state == mutarust::MutationState::Killed);
        assert!(
            killed,
            "mutant runs must finish inside the adaptive timeout: {:?}",
            run.results()
                .iter()
                .map(|result| (&result.state, &result.diff))
                .collect::<Vec<_>>()
        );
        return;
    }

    let _run_guard = public_run_guard();
    let root = unique_temporary_root("slow-coverage");
    let source = write_single_file_crate(
        &root,
        "slow-coverage-fixture",
        "pub fn detected() -> bool {\n    let value = false;\n    value\n}\n",
    );
    let fake_cargo = root.0.join("slow-coverage-cargo");
    std::fs::write(
        &fake_cargo,
        "#!/bin/sh\nif [ \"$1\" = \"metadata\" ]; then\n  exec \"$MUTARUST_REAL_CARGO\" \"$@\"\nfi\nif [ \"$1\" = \"llvm-cov\" ]; then\n  output=\n  while [ \"$#\" -gt 0 ]; do\n    if [ \"$1\" = \"--output-path\" ]; then\n      output=$2\n      break\n    fi\n    shift\n  done\n  sleep 3\n  printf 'SF:%s\\nDA:1,1\\nDA:2,1\\nend_of_record\\n' \"$MUTARUST_SLOW_COVERAGE_SOURCE\" > \"$output\"\n  exit 0\nfi\nif grep -q 'let value = false;' src/lib.rs 2>/dev/null; then\n  sleep 3\n  exit 0\nfi\nexit 1\n",
    )
    .expect("slow coverage Cargo command must be written");
    std::fs::set_permissions(&fake_cargo, std::fs::Permissions::from_mode(0o755))
        .expect("slow coverage Cargo command must be executable");

    let output =
        std::process::Command::new(std::env::current_exe().expect("test command must resolve"))
            .args(["--exact", "timeout_coefficient_extends_the_coverage_budget"])
            .env("CARGO", &fake_cargo)
            .env("MUTARUST_REAL_CARGO", env!("CARGO"))
            .env(COVERAGE_ENV, &source)
            .output()
            .expect("the coverage child run must start");
    assert!(
        output.status.success(),
        "coverage and the clean suite must finish inside the coefficient budget: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn invalid_diff_fuzz_corpus_does_not_become_mutation_results() {
    let _run_guard = public_run_guard();
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time must follow the Unix epoch")
        .as_nanos();
    let root = FixtureRoot(std::env::temp_dir().join(format!(
        "mutarust-invalid-edit-{}-{unique}",
        std::process::id()
    )));
    let source = root.0.join("src").join("lib.rs");
    std::fs::create_dir_all(source.parent().expect("source must have a parent"))
        .expect("fixture source directory must be created");
    std::fs::write(
        root.0.join("Cargo.toml"),
        "[package]\nname = \"invalid-edit-fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .expect("fixture manifest must be written");
    std::fs::write(&source, "pub fn café() -> i32 { 1 }\n")
        .expect("fixture source must be written");

    let registry = mutarust::RegistryBuilder::new()
        .register(InvalidEdit)
        .expect("custom mutator must register")
        .build();
    let names = registry.names().map(str::to_owned).collect::<Vec<_>>();
    let filters = mutarust::SourceFilters::new(&[], &[], None, &names)
        .expect("source filters must accept the custom mutator");
    let controls = mutarust::ExecutionControls {
        dry_run: true,
        ..mutarust::ExecutionControls::default()
    };
    let run = mutarust::run_mutation_tests_with_controls(
        &[source.to_string_lossy().into_owned()],
        &registry,
        std::time::Duration::from_secs(1),
        None,
        &filters,
        &mutarust::TestExecution::cargo(),
        &controls,
    )
    .expect("invalid edits must not fail the run");

    assert!(
        run.results().is_empty(),
        "the invalid diff fuzz corpus must not become results"
    );
}

#[cfg(any(unix, windows))]
static HOST_INTERRUPT_SEEN: AtomicBool = AtomicBool::new(false);

#[cfg(unix)]
extern "C" fn host_interrupt_handler(_: libc::c_int) {
    HOST_INTERRUPT_SEEN.store(true, Ordering::SeqCst);
}

#[cfg(windows)]
unsafe extern "system" fn host_interrupt_handler(control_type: u32) -> i32 {
    use windows_sys::Win32::System::Console::{CTRL_BREAK_EVENT, CTRL_C_EVENT};

    if control_type == CTRL_C_EVENT || control_type == CTRL_BREAK_EVENT {
        HOST_INTERRUPT_SEEN.store(true, Ordering::SeqCst);
        1
    } else {
        0
    }
}

#[cfg(unix)]
#[test]
fn mutation_run_restores_the_host_interrupt_handler() {
    let _run_guard = public_run_guard();
    HOST_INTERRUPT_SEEN.store(false, Ordering::SeqCst);
    let handler = host_interrupt_handler as *const () as usize;
    let previous = unsafe { libc::signal(libc::SIGINT, handler) };
    assert_ne!(previous, libc::SIG_ERR, "host signal handler must install");

    let result = mutarust::run_mutation_tests(
        &["mutarust-test-target-that-does-not-exist".to_owned()],
        &mutarust::Registry::builtins(),
    );
    assert!(result.is_err(), "the invalid target must fail");
    unsafe {
        libc::raise(libc::SIGINT);
    }
    assert!(
        HOST_INTERRUPT_SEEN.load(Ordering::SeqCst),
        "Mutarust must restore the host signal handler"
    );

    unsafe {
        libc::signal(libc::SIGINT, previous);
    }
}

#[test]
fn adaptive_timeout_requires_the_public_cargo_execution() {
    let execution = mutarust::TestExecution::custom("true", false, false, false)
        .expect("custom command must parse");
    let filters = mutarust::SourceFilters::new(&[], &[], None, &[])
        .expect("empty source filters must be valid");
    let controls = mutarust::ExecutionControls {
        timeout_coefficient: Some(1.5),
        ..mutarust::ExecutionControls::default()
    };

    let result = mutarust::run_mutation_tests_with_controls(
        &[],
        &mutarust::Registry::builtins(),
        std::time::Duration::from_secs(1),
        None,
        &filters,
        &execution,
        &controls,
    );
    let error = match result {
        Err(error) => error,
        Ok(_) => panic!("adaptive timeout must reject a custom command"),
    };

    assert_eq!(
        error.to_string(),
        "adaptive timeout requires the Cargo test command"
    );
}

#[test]
fn worker_limit_requires_a_positive_value() {
    assert!(mutarust::WorkerLimit::new(0).is_none());
    let workers = mutarust::WorkerLimit::new(2).expect("two workers must be valid");
    assert_eq!(workers.get(), 2);
}

#[test]
fn expression_remove_does_not_plan_a_no_op_mutant_for_tautological_operands() {
    let _run_guard = public_run_guard();
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time must follow the Unix epoch")
        .as_nanos();
    let root = FixtureRoot(std::env::temp_dir().join(format!(
        "mutarust-no-op-remove-{}-{unique}",
        std::process::id()
    )));
    let source = root.0.join("src").join("lib.rs");
    std::fs::create_dir_all(source.parent().expect("source must have a parent"))
        .expect("fixture source directory must be created");
    std::fs::write(
        root.0.join("Cargo.toml"),
        "[package]\nname = \"no-op-remove-fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .expect("fixture manifest must be written");
    std::fs::write(
        &source,
        "pub fn check(a: bool) -> bool {\n    a && true\n}\n\npub fn other(x: bool) -> bool {\n    x || false\n}\n",
    )
    .expect("fixture source must be written");

    let mut registry = mutarust::Registry::builtins();
    registry.retain(|name| name == "expression/remove");
    let execution = mutarust::TestExecution::custom("false", false, false, false)
        .expect("custom command must parse");
    let filters = mutarust::SourceFilters::new(&[], &[], None, &[])
        .expect("empty source filters must be valid");
    let controls = mutarust::ExecutionControls::default();
    let run = mutarust::run_mutation_tests_with_controls(
        &[source.to_string_lossy().into_owned()],
        &registry,
        std::time::Duration::from_secs(1),
        None,
        &filters,
        &execution,
        &controls,
    )
    .expect("the run must complete");
    let results = run.results();

    let useful = [
        "    true && true\n",   // left operand of a && true
        "    false || false\n", // left operand of x || false
    ];
    for expected in useful {
        assert!(
            results.iter().any(|result| result.diff.contains(expected)),
            "the useful operand mutant must stay planned: {expected}"
        );
    }
    let phantoms: Vec<_> = results
        .iter()
        .filter(|result| {
            !result.diff.lines().any(|line| {
                (line.starts_with('-') && !line.starts_with("---"))
                    || (line.starts_with('+') && !line.starts_with("+++"))
            })
        })
        .collect();
    assert!(
        phantoms.is_empty(),
        "a mutant whose applied text equals the source text must not be planned: {:?}",
        phantoms
            .iter()
            .map(|result| result.diff.as_str())
            .collect::<Vec<_>>()
    );
    let ids: std::collections::BTreeSet<&str> = results
        .iter()
        .map(|result| result.stable_id.as_str())
        .collect();
    assert_eq!(
        ids.len(),
        results.len(),
        "each planned mutant must keep a distinct stable mutant ID"
    );
}

#[test]
fn adjacent_equal_statement_removals_keep_distinct_stable_ids() {
    let _run_guard = public_run_guard();
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time must follow the Unix epoch")
        .as_nanos();
    let root = FixtureRoot(std::env::temp_dir().join(format!(
        "mutarust-adjacent-equal-statements-{}-{unique}",
        std::process::id()
    )));
    let source = root.0.join("src").join("lib.rs");
    std::fs::create_dir_all(source.parent().expect("source must have a parent"))
        .expect("fixture source directory must be created");
    std::fs::write(
        root.0.join("Cargo.toml"),
        "[package]\nname = \"adjacent-equal-statements\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .expect("fixture manifest must be written");
    std::fs::write(
        &source,
        "pub fn check() {\n    println!(\"same\");\n    println!(\"same\");\n}\n",
    )
    .expect("fixture source must be written");

    let mut registry = mutarust::Registry::builtins();
    registry.retain(|name| name == "statement/remove");
    let names = registry.names().map(str::to_owned).collect::<Vec<_>>();
    let filters = mutarust::SourceFilters::new(&[], &[], None, &names)
        .expect("source filters must accept the statement mutator");
    let controls = mutarust::ExecutionControls {
        dry_run: true,
        ..mutarust::ExecutionControls::default()
    };
    let run = mutarust::run_mutation_tests_with_controls(
        &[source.to_string_lossy().into_owned()],
        &registry,
        std::time::Duration::from_secs(1),
        None,
        &filters,
        &mutarust::TestExecution::cargo(),
        &controls,
    )
    .expect("the dry run must complete");

    assert_eq!(
        run.results().len(),
        2,
        "both statement removals must be planned"
    );
    let ids: std::collections::BTreeSet<&str> = run
        .results()
        .iter()
        .map(|result| result.stable_id.as_str())
        .collect();
    assert_eq!(
        ids.len(),
        2,
        "adjacent equal statements must have distinct stable IDs: {:?}",
        run.results()
            .iter()
            .map(|result| (result.stable_id.as_str(), result.line))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        run.results()
            .iter()
            .map(|result| result.line)
            .collect::<Vec<_>>(),
        vec![2, 3],
        "each mutation must retain its source line"
    );

    let first_id = run
        .results()
        .iter()
        .find(|result| result.line == 2)
        .expect("the first statement mutant must exist")
        .stable_id
        .clone();
    let second_id = run
        .results()
        .iter()
        .find(|result| result.line == 3)
        .expect("the second statement mutant must exist")
        .stable_id
        .clone();
    let execution = mutarust::TestExecution::custom("false", false, false, false)
        .expect("the escaping test command must parse");
    for id in [&first_id, &second_id] {
        let selected = mutarust::run_mutation_tests_with_controls(
            &[source.to_string_lossy().into_owned()],
            &registry,
            std::time::Duration::from_secs(1),
            Some(id.as_str()),
            &filters,
            &execution,
            &mutarust::ExecutionControls::default(),
        )
        .expect("a distinct stable ID must select one mutant");
        assert_eq!(selected.results().len(), 1);
        assert_eq!(selected.results()[0].stable_id, id.as_str());
        assert_eq!(
            selected.results()[0].state,
            mutarust::MutationState::Escaped
        );
    }

    let blacklist = root.0.join("blacklist.txt");
    std::fs::write(
        &blacklist,
        format!("{:x}\n", md5::compute("-    println!(\"same\");\n+    \n")),
    )
    .expect("blacklist must be written");
    let mut blacklist_controls = controls.clone();
    blacklist_controls.blacklist_files.push(blacklist);
    let unblacklisted = mutarust::run_mutation_tests_with_controls(
        &[source.to_string_lossy().into_owned()],
        &registry,
        std::time::Duration::from_secs(1),
        None,
        &filters,
        &mutarust::TestExecution::cargo(),
        &blacklist_controls,
    )
    .expect("the blacklist run must complete");
    assert_eq!(
        unblacklisted.results().len(),
        1,
        "the first blacklist checksum must apply to one mutant: {:?}",
        unblacklisted
            .results()
            .iter()
            .map(|result| (&result.stable_id, &result.diff))
            .collect::<Vec<_>>()
    );
    assert_eq!(unblacklisted.results()[0].stable_id, second_id);

    let baseline_path = root.0.join("baseline.json");
    std::fs::write(
        &baseline_path,
        format!(
            "{{\"version\":1,\"mutants\":[{{\"id\":\"{first_id}\",\"file\":\"src/lib.rs\",\"mutator\":\"statement/remove\",\"line\":2}}]}}\n"
        ),
    )
    .expect("baseline must be written");
    let baseline = mutarust::Baseline::load(&baseline_path).expect("baseline must load");
    let escaped = mutarust::run_mutation_tests_with_controls(
        &[source.to_string_lossy().into_owned()],
        &registry,
        std::time::Duration::from_secs(1),
        None,
        &filters,
        &execution,
        &mutarust::ExecutionControls::default(),
    )
    .expect("the escaped run must complete");
    assert_eq!(baseline.new_escaped_count(&escaped), 1);
}

#[test]
fn custom_mutator_no_op_replacements_are_not_planned() {
    let _run_guard = public_run_guard();
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time must follow the Unix epoch")
        .as_nanos();
    let root = FixtureRoot(std::env::temp_dir().join(format!(
        "mutarust-custom-no-op-{}-{unique}",
        std::process::id()
    )));
    let source = root.0.join("src").join("lib.rs");
    std::fs::create_dir_all(source.parent().expect("source must have a parent"))
        .expect("fixture source directory must be created");
    std::fs::write(
        root.0.join("Cargo.toml"),
        "[package]\nname = \"custom-no-op-fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .expect("fixture manifest must be written");
    std::fs::write(
        &source,
        "pub fn check(a: bool) -> bool {\n    a && true\n}\n",
    )
    .expect("fixture source must be written");

    let registry = mutarust::RegistryBuilder::new()
        .register(NoOpReplacement)
        .expect("custom mutator must register")
        .build();
    let names = registry.names().map(str::to_owned).collect::<Vec<_>>();
    let execution = mutarust::TestExecution::custom("false", false, false, false)
        .expect("custom command must parse");
    let filters = mutarust::SourceFilters::new(&[], &[], None, &names)
        .expect("source filters must accept the custom mutator");
    let controls = mutarust::ExecutionControls::default();
    let run = mutarust::run_mutation_tests_with_controls(
        &[source.to_string_lossy().into_owned()],
        &registry,
        std::time::Duration::from_secs(1),
        None,
        &filters,
        &execution,
        &controls,
    )
    .expect("the run must complete");

    assert!(
        run.results().is_empty(),
        "a mutant whose applied text equals the source text must not be planned"
    );
}

#[cfg(windows)]
#[test]
fn mutation_run_restores_the_host_interrupt_handler() {
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    use windows_sys::Win32::System::Console::{
        CTRL_BREAK_EVENT, GenerateConsoleCtrlEvent, SetConsoleCtrlHandler,
    };
    use windows_sys::Win32::System::Threading::CREATE_NEW_PROCESS_GROUP;

    struct HelperGuard {
        child: std::process::Child,
        marker: Option<std::path::PathBuf>,
        active: bool,
    }

    impl Drop for HelperGuard {
        fn drop(&mut self) {
            if self.active {
                let _ = self.child.kill();
                let _ = self.child.wait();
            }
            if let Some(marker) = self.marker.take() {
                let _ = std::fs::remove_file(marker);
            }
        }
    }

    if let Some(marker) = std::env::var_os("MUTARUST_INTERRUPT_HELPER") {
        HOST_INTERRUPT_SEEN.store(false, Ordering::SeqCst);
        let installed = unsafe { SetConsoleCtrlHandler(Some(host_interrupt_handler), 1) };
        assert_ne!(installed, 0, "host console handler must install");

        let result = mutarust::run_mutation_tests(
            &["mutarust-test-target-that-does-not-exist".to_owned()],
            &mutarust::Registry::builtins(),
        );
        assert!(result.is_err(), "the invalid target must fail");
        std::fs::write(marker, b"ready").expect("interrupt helper marker must be written");
        for _ in 0..100 {
            if HOST_INTERRUPT_SEEN.load(Ordering::SeqCst) {
                unsafe {
                    SetConsoleCtrlHandler(Some(host_interrupt_handler), 0);
                }
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("Mutarust must remove its console handler and keep the host handler");
    }

    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time must follow the Unix epoch")
        .as_nanos();
    let marker = std::env::temp_dir().join(format!(
        "mutarust-interrupt-helper-{}-{unique}",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&marker);
    let child = Command::new(std::env::current_exe().expect("test command must resolve"))
        .args([
            "--exact",
            "mutation_run_restores_the_host_interrupt_handler",
        ])
        .env("MUTARUST_INTERRUPT_HELPER", &marker)
        .creation_flags(CREATE_NEW_PROCESS_GROUP)
        .spawn()
        .expect("interrupt helper must start");
    let mut helper = HelperGuard {
        child,
        marker: Some(marker.clone()),
        active: true,
    };
    for _ in 0..1000 {
        if marker.is_file() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(marker.is_file(), "interrupt helper must become ready");
    let generated = unsafe { GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, helper.child.id()) };
    assert_ne!(generated, 0, "console interrupt must be generated");
    let status = helper.child.wait().expect("interrupt helper must stop");
    helper.active = false;
    std::fs::remove_file(&marker).expect("interrupt helper marker must be removed");
    helper.marker = None;
    assert!(
        status.success(),
        "the restored host console handler must receive the targeted interrupt"
    );
}

#[test]
fn two_workspaces_sharing_layout_root_complete_clean_suite_and_run_mutants() {
    let _run_guard = public_run_guard();
    let parent = unique_temporary_root("shared-layout");
    std::fs::create_dir_all(parent.0.join(".cargo")).expect("cargo config dir must be created");
    std::fs::write(
        parent.0.join(".cargo").join("config.toml"),
        "# shared cargo config\n",
    )
    .expect("cargo config must be written");

    let crate_a = parent.0.join("crate_a");
    std::fs::create_dir_all(crate_a.join("src")).expect("crate_a src dir must be created");
    std::fs::write(
        crate_a.join("Cargo.toml"),
        "[package]\nname = \"crate-a\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .expect("crate_a manifest must be written");
    let source_a = crate_a.join("src").join("lib.rs");
    std::fs::write(
        &source_a,
        "pub fn answer_a() -> bool {\n    let value_a = false;\n    value_a\n}\n",
    )
    .expect("crate_a source must be written");
    std::fs::create_dir_all(crate_a.join("tests")).expect("crate_a tests dir must be created");
    std::fs::write(
        crate_a.join("tests").join("suite.rs"),
        "#[test]\nfn test_a() {\n    assert!(!crate_a::answer_a());\n}\n",
    )
    .expect("crate_a test must be written");

    let crate_b = parent.0.join("crate_b");
    std::fs::create_dir_all(crate_b.join("src")).expect("crate_b src dir must be created");
    std::fs::write(
        crate_b.join("Cargo.toml"),
        "[package]\nname = \"crate-b\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .expect("crate_b manifest must be written");
    let source_b = crate_b.join("src").join("lib.rs");
    std::fs::write(
        &source_b,
        "pub fn answer_b() -> bool {\n    let value_b = false;\n    value_b\n}\n",
    )
    .expect("crate_b source must be written");
    std::fs::create_dir_all(crate_b.join("tests")).expect("crate_b tests dir must be created");
    std::fs::write(
        crate_b.join("tests").join("suite.rs"),
        "#[test]\nfn test_b() {\n    assert!(!crate_b::answer_b());\n}\n",
    )
    .expect("crate_b test must be written");

    let run = mutarust::run_mutation_tests(
        &[
            source_a.to_string_lossy().into_owned(),
            source_b.to_string_lossy().into_owned(),
        ],
        &mutarust::Registry::builtins(),
    )
    .expect("the mutation run across two shared-layout workspaces must succeed");

    assert_eq!(run.results().len(), 2);
    let has_crate_a = run
        .results()
        .iter()
        .any(|r| r.diff.contains("value_a") && r.state == mutarust::MutationState::Killed);
    let has_crate_b = run
        .results()
        .iter()
        .any(|r| r.diff.contains("value_b") && r.state == mutarust::MutationState::Killed);
    assert!(has_crate_a, "mutants must run and be killed for crate_a");
    assert!(has_crate_b, "mutants must run and be killed for crate_b");
}

#[test]
fn two_workspaces_sharing_layout_root_complete_clean_suite_and_run_mutants_in_parallel() {
    let _run_guard = public_run_guard();
    let parent = unique_temporary_root("shared-layout-parallel");
    std::fs::create_dir_all(parent.0.join(".cargo")).expect("cargo config dir must be created");
    std::fs::write(
        parent.0.join(".cargo").join("config.toml"),
        "# shared cargo config\n",
    )
    .expect("cargo config must be written");

    let crate_a = parent.0.join("crate_a");
    std::fs::create_dir_all(crate_a.join("src")).expect("crate_a src dir must be created");
    std::fs::write(
        crate_a.join("Cargo.toml"),
        "[package]\nname = \"crate-a\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .expect("crate_a manifest must be written");
    let source_a = crate_a.join("src").join("lib.rs");
    std::fs::write(
        &source_a,
        "pub fn answer_a() -> bool {\n    let value_a = false;\n    value_a\n}\n",
    )
    .expect("crate_a source must be written");
    std::fs::create_dir_all(crate_a.join("tests")).expect("crate_a tests dir must be created");
    std::fs::write(
        crate_a.join("tests").join("suite.rs"),
        "#[test]\nfn test_a() {\n    assert!(!crate_a::answer_a());\n}\n",
    )
    .expect("crate_a test must be written");

    let crate_b = parent.0.join("crate_b");
    std::fs::create_dir_all(crate_b.join("src")).expect("crate_b src dir must be created");
    std::fs::write(
        crate_b.join("Cargo.toml"),
        "[package]\nname = \"crate-b\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .expect("crate_b manifest must be written");
    let source_b = crate_b.join("src").join("lib.rs");
    std::fs::write(
        &source_b,
        "pub fn answer_b() -> bool {\n    let value_b = false;\n    value_b\n}\n",
    )
    .expect("crate_b source must be written");
    std::fs::create_dir_all(crate_b.join("tests")).expect("crate_b tests dir must be created");
    std::fs::write(
        crate_b.join("tests").join("suite.rs"),
        "#[test]\nfn test_b() {\n    assert!(!crate_b::answer_b());\n}\n",
    )
    .expect("crate_b test must be written");

    let registry = mutarust::Registry::builtins();
    let names = registry.names().map(str::to_owned).collect::<Vec<_>>();
    let filters = mutarust::SourceFilters::new(&[], &[], None, &names)
        .expect("source filters must accept builtins");
    let controls = mutarust::ExecutionControls {
        workers: mutarust::WorkerLimit::new(2).expect("worker count must be valid"),
        ..Default::default()
    };
    let run = mutarust::run_mutation_tests_with_controls(
        &[
            source_a.to_string_lossy().into_owned(),
            source_b.to_string_lossy().into_owned(),
        ],
        &registry,
        std::time::Duration::from_secs(30),
        None,
        &filters,
        &mutarust::TestExecution::cargo(),
        &controls,
    )
    .expect("parallel mutation run across two shared-layout workspaces must succeed");

    assert_eq!(run.results().len(), 2);
    let has_crate_a = run
        .results()
        .iter()
        .any(|r| r.diff.contains("value_a") && r.state == mutarust::MutationState::Killed);
    let has_crate_b = run
        .results()
        .iter()
        .any(|r| r.diff.contains("value_b") && r.state == mutarust::MutationState::Killed);
    assert!(
        has_crate_a,
        "mutants must run and be killed for crate_a in parallel"
    );
    assert!(
        has_crate_b,
        "mutants must run and be killed for crate_b in parallel"
    );
}

#[test]
fn cargo_worker_scratch_removes_test_files_between_mutants() {
    let _run_guard = public_run_guard();
    let root = unique_temporary_root("worker-side-effect");
    let source = write_single_file_crate(
        &root,
        "workspace-side-effect",
        "pub fn create_marker() -> bool {\n    false\n}\n\npub fn unused_value() -> bool {\n    false\n}\n",
    );
    std::fs::create_dir_all(root.0.join("tests")).expect("fixture tests directory must be created");
    std::fs::write(
        root.0.join("tests").join("suite.rs"),
        "#[test]\nfn workspace_starts_clean() {\n    let marker = std::path::Path::new(env!(\"CARGO_MANIFEST_DIR\"))\n        .join(\"mutarust-side-effect.txt\");\n    if workspace_side_effect::create_marker() {\n        std::fs::write(&marker, \"created\").expect(\"marker must be written\");\n    }\n    assert!(!marker.exists(), \"a previous mutant left a workspace file\");\n}\n",
    )
    .expect("fixture test must be written");

    let registry = mutarust::RegistryBuilder::new()
        .register(WorkspaceSideEffect)
        .expect("custom mutator must register")
        .build();
    let names = registry.names().map(str::to_owned).collect::<Vec<_>>();
    let filters = mutarust::SourceFilters::new(&[], &[], None, &names)
        .expect("source filters must accept the custom mutator");
    let execution = mutarust::TestExecution::cargo();
    let run = |workers| {
        let controls = mutarust::ExecutionControls {
            workers: mutarust::WorkerLimit::new(workers).expect("worker count must be valid"),
            ..Default::default()
        };
        mutarust::run_mutation_tests_with_controls(
            &[source.to_string_lossy().into_owned()],
            &registry,
            std::time::Duration::from_secs(30),
            None,
            &filters,
            &execution,
            &controls,
        )
        .expect("mutation run must succeed")
    };

    let sequential = run(1);
    let parallel = run(2);
    for mutation_run in [&sequential, &parallel] {
        let created_marker = mutation_run
            .results()
            .iter()
            .find(|result| result.line == 2)
            .expect("create_marker mutant must be present");
        let unused_value = mutation_run
            .results()
            .iter()
            .find(|result| result.line == 6)
            .expect("unused_value mutant must be present");
        assert_eq!(
            created_marker.state,
            mutarust::MutationState::Killed,
            "the marker mutant must be killed by the workspace assertion"
        );
        assert_eq!(
            unused_value.state,
            mutarust::MutationState::Escaped,
            "the unused mutant must escape when each run starts with a clean workspace"
        );
    }
}
