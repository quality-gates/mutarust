use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use mutarust::{Blacklist, Mutation, MutationChecksum, StableMutantId, mutation_evidence};

struct Fixture(PathBuf);

impl Fixture {
    fn new(tag: &str) -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "mutarust-evidence-{tag}-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        Self(root)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn pinned_evidence_and_identity_parsers() {
    let text = "pub fn unchecked() -> bool { true }\n";
    let evidence = mutation_evidence(
        Path::new("workspace"),
        Path::new("workspace/checked/src/lib.rs"),
        "conditional/bool-literal",
        &Mutation::new(29..33, "false"),
        text,
        false,
    )
    .unwrap();
    assert_eq!(evidence.source, Path::new("checked/src/lib.rs"));
    assert_eq!(evidence.line, 1);
    assert_eq!(
        evidence.stable_id.as_str(),
        "7e44f5b6649ca4de087acb260e75a287"
    );
    assert_eq!(
        evidence.blacklist_checksum.as_str(),
        "c5b09ed00204d4cc02955047cba77a84"
    );
    assert_eq!(
        evidence.diff,
        "--- checked/src/lib.rs\n+++ checked/src/lib.rs\n@@ -1,1 +1,1 @@\n-pub fn unchecked() -> bool { true }\n+pub fn unchecked() -> bool { false }\n"
    );
    let valid = "0123456789abcdef0123456789abcdef";
    assert_eq!(StableMutantId::parse(valid).unwrap().as_str(), valid);
    assert_eq!(MutationChecksum::parse(valid).unwrap().as_str(), valid);
    for invalid in [
        "a".repeat(31),
        "a".repeat(33),
        "A".repeat(32),
        format!("{}é", "a".repeat(30)),
        format!("{} ", "a".repeat(31)),
        "g".repeat(32),
    ] {
        assert!(StableMutantId::parse(&invalid).is_none(), "{invalid:?}");
        assert!(MutationChecksum::parse(&invalid).is_none(), "{invalid:?}");
    }
}

#[test]
fn blacklist_files_and_errors() {
    let fixture = Fixture::new("blacklist");
    let first = fixture.0.join("one");
    let second = fixture.0.join("two");
    let evidence = mutation_evidence(
        Path::new("root"),
        Path::new("root/a.rs"),
        "m",
        &Mutation::new(0..1, "b"),
        "a",
        false,
    )
    .unwrap();
    let checksum = evidence.blacklist_checksum;
    std::fs::write(
        &first,
        format!("\r\n{}\r\n{}\r\n", checksum.as_str(), checksum.as_str()),
    )
    .unwrap();
    std::fs::write(&second, "0123456789abcdef0123456789abcdef\n\n").unwrap();
    let blacklist = Blacklist::load(&[first.clone(), second.clone()]).unwrap();
    assert!(blacklist.contains(&checksum));
    assert!(
        blacklist.contains(&MutationChecksum::parse("0123456789abcdef0123456789abcdef").unwrap())
    );
    assert!(
        !blacklist.contains(&MutationChecksum::parse("ffffffffffffffffffffffffffffffff").unwrap())
    );
    std::fs::write(&second, "\nINVALID\n").unwrap();
    assert_eq!(
        Blacklist::load(std::slice::from_ref(&second)).unwrap_err(),
        format!(
            "blacklist {} line 2 must be a 32-character lower-case hexadecimal checksum",
            second.display()
        )
    );
    let missing = fixture.0.join("missing");
    let error = Blacklist::load(std::slice::from_ref(&missing)).unwrap_err();
    let io_error = std::fs::read_to_string(&missing).unwrap_err();
    assert_eq!(
        error,
        format!("could not read blacklist {}: {io_error}", missing.display())
    );
}

#[test]
fn invalid_ranges_root_crlf_and_unterminated_line() {
    let root = Path::new("root");
    let source = Path::new("root/a.rs");
    for range in [4..5, std::ops::Range { start: 2, end: 1 }, 1..2, 0..1] {
        assert_eq!(
            mutation_evidence(root, source, "m", &Mutation::new(range, ""), "éx", false)
                .unwrap_err(),
            "mutation range is invalid for source text"
        );
    }
    assert_eq!(
        mutation_evidence(
            root,
            Path::new("other/a.rs"),
            "m",
            &Mutation::new(0..1, "b"),
            "a",
            false
        )
        .unwrap_err(),
        "path is outside the stable source root: other/a.rs"
    );
    let no_op =
        mutation_evidence(root, source, "m", &Mutation::new(0..1, "a"), "a", false).unwrap();
    assert_eq!(no_op.diff, "--- a.rs\n+++ a.rs\n@@ -1,0 +1,0 @@\n");
    let text = "first\r\nlast";
    let evidence =
        mutation_evidence(root, source, "m", &Mutation::new(7..11, "end"), text, false).unwrap();
    assert_eq!(evidence.line, 2);
    assert_eq!(
        evidence.diff,
        "--- a.rs\n+++ a.rs\n@@ -2,1 +2,1 @@\n-last\n+end\n"
    );
    let crlf = mutation_evidence(
        root,
        source,
        "m",
        &Mutation::new(0..5, "other"),
        text,
        false,
    )
    .unwrap();
    assert_eq!(
        crlf.diff,
        "--- a.rs\n+++ a.rs\n@@ -1,1 +1,1 @@\n-first\r\n+other\r\n"
    );
    let lf = mutation_evidence(
        root,
        source,
        "m",
        &Mutation::new(0..5, "other"),
        "first\nlast",
        false,
    )
    .unwrap();
    assert_ne!(crlf.stable_id, lf.stable_id);
    assert_ne!(crlf.blacklist_checksum, lf.blacklist_checksum);
}

#[test]
fn repeated_evidence_matches_planner() {
    let fixture = Fixture::new("repeated");
    let source = fixture.0.join("src/lib.rs");
    std::fs::create_dir_all(source.parent().unwrap()).unwrap();
    std::fs::write(
        fixture.0.join("Cargo.toml"),
        "[package]\nname = \"evidence-check\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    let text = "pub fn check() {\n    println!(\"same\");\n    println!(\"same\");\n}\n";
    std::fs::write(&source, text).unwrap();
    let mut registry = mutarust::Registry::builtins();
    registry.retain(|name| name == "statement/remove");
    let names = registry.names().map(str::to_owned).collect::<Vec<_>>();
    let filters = mutarust::SourceFilters::new(&[], &[], None, &names).unwrap();
    let controls = mutarust::ExecutionControls {
        dry_run: true,
        ..Default::default()
    };
    let run = mutarust::run_mutation_tests_with_controls(
        &[source.to_string_lossy().into_owned()],
        &registry,
        Duration::from_secs(1),
        None,
        &filters,
        &mutarust::TestExecution::cargo(),
        &controls,
    )
    .unwrap();
    assert_eq!(run.results().len(), 2);
    let statement = "println!(\"same\");";
    let first = text.find(statement).unwrap();
    let second = text.rfind(statement).unwrap();
    let build = |start, repeated| {
        mutation_evidence(
            &fixture.0,
            &source,
            "statement/remove",
            &Mutation::new(start..start + statement.len(), ""),
            text,
            repeated,
        )
        .unwrap()
    };
    let one = build(first, false);
    let base_two = build(second, false);
    let two = build(second, true);
    assert_eq!(one.stable_id, base_two.stable_id);
    assert_ne!(one.stable_id, two.stable_id);
    for (evidence, result) in [one, two].iter().zip(run.results()) {
        assert_eq!(evidence.stable_id.as_str(), result.stable_id);
        assert_eq!(
            evidence.blacklist_checksum.as_str(),
            result.blacklist_checksum
        );
        assert_eq!(evidence.line, result.line);
        assert_eq!(evidence.diff, result.diff);
    }
}
