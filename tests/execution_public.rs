#[cfg(any(unix, windows))]
use std::sync::atomic::{AtomicBool, Ordering};

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

struct FixtureRoot(std::path::PathBuf);

impl Drop for FixtureRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn invalid_diff_fuzz_corpus_does_not_become_mutation_results() {
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
