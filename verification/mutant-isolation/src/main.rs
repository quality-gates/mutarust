//! Bounded check that a Cargo mutant run stays inside the temporary workspace.
//!
//! The public seam is `run_mutation_tests_with_controls`. Each layout is one
//! package. The test command is Cargo, replaced by an observer that reads
//! `src/lib.rs` the way rustc opens that path: it follows symbolic links.

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use mutarust::{
    run_mutation_tests_with_controls, ExecutionControls, Mutation, Mutator, RegistryBuilder,
    SourceFilters, TestExecution, WorkerLimit,
};

const ORIGINAL: &str = "pub fn ready() -> bool { true }\n";
const MUTANT: &str = "pub fn ready() -> bool { false }\n";
const MANIFEST: &str = "[package]\nname = \"ready\"\nversion = \"0.1.0\"\nedition = \"2021\"\n";

#[derive(Clone, Copy)]
enum Shape {
    Regular,
    FileRelative,
    FileAbsolute,
    DirRelative,
    DirAbsolute,
}

impl Shape {
    fn all() -> [Self; 5] {
        [
            Self::Regular,
            Self::FileRelative,
            Self::FileAbsolute,
            Self::DirRelative,
            Self::DirAbsolute,
        ]
    }

    fn name(self) -> &'static str {
        match self {
            Self::Regular => "regular",
            Self::FileRelative => "file-relative",
            Self::FileAbsolute => "file-absolute",
            Self::DirRelative => "dir-relative",
            Self::DirAbsolute => "dir-absolute",
        }
    }
}

#[derive(Clone, Eq, PartialEq)]
enum Node {
    File(Vec<u8>),
    Link(PathBuf),
    Dir,
}

impl std::fmt::Display for Node {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::File(bytes) => write!(formatter, "file {} bytes", bytes.len()),
            Self::Link(target) => write!(formatter, "link -> {}", target.display()),
            Self::Dir => formatter.write_str("directory"),
        }
    }
}

struct Read {
    kind: String,
    resolved: PathBuf,
}

struct ShapeReport {
    passed: bool,
    lines: Vec<String>,
}

struct Report {
    text: String,
    passed: bool,
}

fn main() {
    match run() {
        Ok(report) => {
            print!("{}", report.text);
            std::process::exit(if report.passed { 0 } else { 1 });
        }
        Err(error) => {
            eprintln!("mutant isolation check could not run: {error}");
            std::process::exit(2);
        }
    }
}

fn run() -> Result<Report, String> {
    check_mutator()?;
    let cargo = toolchain_cargo()?;
    let root = env::temp_dir().join(format!("mutant-isolation-check-{}", std::process::id()));
    fs::create_dir_all(&root)
        .map_err(|error| format!("could not create {}: {error}", root.display()))?;
    let original = root.join("original.rs");
    let mutant = root.join("mutant.rs");
    fs::write(&original, ORIGINAL).map_err(|error| format!("could not write oracle: {error}"))?;
    fs::write(&mutant, MUTANT).map_err(|error| format!("could not write oracle: {error}"))?;
    let observer = root.join("observe.sh");
    write_observer(&observer, &cargo, &original, &mutant)?;
    env::set_var("CARGO", &observer);

    let mut sections = Vec::new();
    sections.push(
        "mutant isolation check\n\
         bound: 5 layouts, 1 worker, 1 mutant, Cargo test command\n\
         property: each Cargo read stays outside the user package, each mutant read is the mutant, each result is killed, and the user source tree is unchanged\n"
            .to_owned(),
    );
    let mut passed = true;
    for shape in Shape::all() {
        let report = check_shape(shape, &root)?;
        passed &= report.passed;
        sections.push(report.lines.join("\n"));
        sections.push(String::new());
    }
    let _ = fs::remove_dir_all(&root);
    Ok(Report {
        text: format!("{}\n", sections.join("\n")),
        passed,
    })
}

fn check_mutator() -> Result<(), String> {
    let mutations = BoolFlip.mutations(ORIGINAL);
    if mutations.len() != 1 {
        return Err(format!(
            "the check mutant must be one replacement, found {}",
            mutations.len()
        ));
    }
    let applied = mutations[0]
        .apply(ORIGINAL)
        .ok_or_else(|| "the check mutant must apply".to_owned())?;
    if applied != MUTANT {
        return Err("the check mutant text does not match the oracle".to_owned());
    }
    Ok(())
}

fn toolchain_cargo() -> Result<PathBuf, String> {
    let output = Command::new("rustc")
        .args(["--print", "sysroot"])
        .output()
        .map_err(|error| format!("could not run rustc: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "could not read the rustc sysroot: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let sysroot =
        String::from_utf8(output.stdout).map_err(|_| "rustc sysroot is not UTF-8".to_owned())?;
    let cargo = PathBuf::from(sysroot.trim()).join("bin").join("cargo");
    if !cargo.is_file() {
        return Err(format!("toolchain cargo is missing: {}", cargo.display()));
    }
    Ok(cargo)
}

fn write_observer(
    script: &Path,
    cargo: &Path,
    original: &Path,
    mutant: &Path,
) -> Result<(), String> {
    let body = format!(
        "#!/bin/sh\n\
         set -u\n\
         cargo={cargo}\n\
         original={original}\n\
         mutant={mutant}\n\
         if [ \"${{1:-}}\" != \"test\" ]; then\n\
           exec env -u CARGO \"$cargo\" \"$@\"\n\
         fi\n\
         log=\"${{MUTARUST_ISOLATION_LOG:?}}\"\n\
         manifest=\"\"\n\
         previous=\"\"\n\
         for argument in \"$@\"; do\n\
           if [ \"$previous\" = \"--manifest-path\" ]; then\n\
             manifest=$argument\n\
           fi\n\
           previous=$argument\n\
         done\n\
         if [ -z \"$manifest\" ]; then\n\
           printf '%s\\n' 'missing-manifest -' >> \"$log\"\n\
           exit 0\n\
         fi\n\
         source_file=\"$(dirname \"$manifest\")/src/lib.rs\"\n\
         if [ ! -e \"$source_file\" ]; then\n\
           printf '%s\\n' 'missing-source -' >> \"$log\"\n\
           exit 0\n\
         fi\n\
         resolved=$(readlink -f \"$source_file\")\n\
         if cmp -s \"$source_file\" \"$original\"; then\n\
           kind=original\n\
         elif cmp -s \"$source_file\" \"$mutant\"; then\n\
           kind=mutant\n\
         else\n\
           kind=other\n\
         fi\n\
         printf '%s %s\\n' \"$kind\" \"$resolved\" >> \"$log\"\n\
         if [ \"$kind\" = mutant ]; then\n\
           exit 1\n\
         fi\n\
         exit 0\n",
        cargo = shell_single(cargo),
        original = shell_single(original),
        mutant = shell_single(mutant),
    );
    fs::write(script, body).map_err(|error| format!("could not write observer: {error}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(script, fs::Permissions::from_mode(0o755))
            .map_err(|error| format!("could not mark the observer executable: {error}"))?;
    }
    Ok(())
}

fn shell_single(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', "'\\''"))
}

fn check_shape(shape: Shape, root: &Path) -> Result<ShapeReport, String> {
    let package = root.join(shape.name());
    let _ = fs::remove_dir_all(&package);
    materialize(shape, &package)?;
    let package = fs::canonicalize(&package)
        .map_err(|error| format!("could not resolve {}: {error}", package.display()))?;
    let before = snapshot(&package)?;
    let log = root.join(format!("{}.log", shape.name()));
    let _ = fs::remove_file(&log);
    fs::write(&log, "").map_err(|error| format!("could not create the read log: {error}"))?;
    env::set_var("MUTARUST_ISOLATION_LOG", &log);

    let registry = RegistryBuilder::new()
        .register(BoolFlip)
        .map_err(|error| format!("could not register the check mutator: {error}"))?
        .build();
    let filters = SourceFilters::new(&[], &[], None, &["verify/bool-flip".to_owned()])
        .map_err(|error| format!("could not build source filters: {error}"))?;
    let controls = ExecutionControls {
        workers: WorkerLimit::new(1).ok_or_else(|| "worker count 1 must be valid".to_owned())?,
        ..ExecutionControls::default()
    };
    let targets = vec![package.display().to_string()];
    let run = run_mutation_tests_with_controls(
        &targets,
        &registry,
        Duration::from_secs(30),
        None,
        &filters,
        &TestExecution::cargo(),
        &controls,
    );
    let after = snapshot(&package)?;
    let reads = parse_reads(&log)?;
    let diff = tree_diff(&before, &after);
    Ok(judge(shape, &package, run, reads, diff))
}

fn materialize(shape: Shape, package: &Path) -> Result<(), String> {
    fs::create_dir_all(package)
        .map_err(|error| format!("could not create {}: {error}", package.display()))?;
    fs::write(package.join("Cargo.toml"), MANIFEST)
        .map_err(|error| format!("could not write the manifest: {error}"))?;
    match shape {
        Shape::Regular => write_source(&package.join("src"), "lib.rs", ORIGINAL)?,
        Shape::FileRelative => {
            write_source(&package.join("src"), "included.rs", ORIGINAL)?;
            link("included.rs", &package.join("src").join("lib.rs"))?;
        }
        Shape::FileAbsolute => {
            let included = package.join("src").join("included.rs");
            write_source(&package.join("src"), "included.rs", ORIGINAL)?;
            link(&included, &package.join("src").join("lib.rs"))?;
        }
        Shape::DirRelative => {
            write_source(&package.join("libsrc"), "lib.rs", ORIGINAL)?;
            link("libsrc", &package.join("src"))?;
        }
        Shape::DirAbsolute => {
            let libsrc = package.join("libsrc");
            write_source(&libsrc, "lib.rs", ORIGINAL)?;
            link(&libsrc, &package.join("src"))?;
        }
    }
    Ok(())
}

fn write_source(directory: &Path, name: &str, text: &str) -> Result<(), String> {
    fs::create_dir_all(directory)
        .map_err(|error| format!("could not create {}: {error}", directory.display()))?;
    fs::write(directory.join(name), text)
        .map_err(|error| format!("could not write {name}: {error}"))
}

fn link(target: impl AsRef<Path>, link_path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target.as_ref(), link_path)
            .map_err(|error| format!("could not link {}: {error}", link_path.display()))
    }
    #[cfg(not(unix))]
    {
        let _ = (target, link_path);
        Err("this check uses Unix symbolic links".to_owned())
    }
}

fn snapshot(root: &Path) -> Result<BTreeMap<PathBuf, Node>, String> {
    let mut nodes = BTreeMap::new();
    walk(root, root, &mut nodes)?;
    Ok(nodes)
}

fn walk(root: &Path, directory: &Path, nodes: &mut BTreeMap<PathBuf, Node>) -> Result<(), String> {
    for entry in fs::read_dir(directory)
        .map_err(|error| format!("could not read {}: {error}", directory.display()))?
    {
        let entry = entry.map_err(|error| format!("could not read an entry: {error}"))?;
        let path = entry.path();
        let relative = path
            .strip_prefix(root)
            .map_err(|_| format!("path left the package: {}", path.display()))?
            .to_path_buf();
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| format!("could not inspect {}: {error}", path.display()))?;
        if metadata.file_type().is_symlink() {
            let target = fs::read_link(&path)
                .map_err(|error| format!("could not read link {}: {error}", path.display()))?;
            nodes.insert(relative, Node::Link(target));
        } else if metadata.is_dir() {
            nodes.insert(relative, Node::Dir);
            walk(root, &path, nodes)?;
        } else if metadata.is_file() {
            let bytes = fs::read(&path)
                .map_err(|error| format!("could not read {}: {error}", path.display()))?;
            nodes.insert(relative, Node::File(bytes));
        }
    }
    Ok(())
}

fn parse_reads(log: &Path) -> Result<Vec<Read>, String> {
    let text = fs::read_to_string(log)
        .map_err(|error| format!("could not read the Cargo log: {error}"))?;
    let mut reads = Vec::new();
    for line in text.lines() {
        let Some((kind, resolved)) = line.split_once(' ') else {
            return Err(format!("Cargo read log has a bad line: {line}"));
        };
        reads.push(Read {
            kind: kind.to_owned(),
            resolved: PathBuf::from(resolved),
        });
    }
    Ok(reads)
}

fn tree_diff(before: &BTreeMap<PathBuf, Node>, after: &BTreeMap<PathBuf, Node>) -> Vec<String> {
    let keys = before
        .keys()
        .chain(after.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut lines = Vec::new();
    for key in keys {
        match (before.get(&key), after.get(&key)) {
            (Some(left), Some(right)) if left == right => {}
            (Some(left), Some(right)) => {
                lines.push(format!("changed {}: {} -> {}", key.display(), left, right));
            }
            (None, Some(right)) => lines.push(format!("added {}: {}", key.display(), right)),
            (Some(left), None) => lines.push(format!("removed {}: {}", key.display(), left)),
            (None, None) => {}
        }
    }
    lines
}

fn judge(
    shape: Shape,
    package: &Path,
    run: Result<mutarust::MutationRun, mutarust::RunError>,
    reads: Vec<Read>,
    diff: Vec<String>,
) -> ShapeReport {
    let name = shape.name();
    let mut failures = Vec::new();
    let states = match &run {
        Ok(run) => run
            .results()
            .iter()
            .map(|result| result.state.to_string())
            .collect::<Vec<_>>(),
        Err(error) => {
            failures.push(format!("mutation run failed: {error}"));
            Vec::new()
        }
    };
    if !diff.is_empty() {
        failures.push("user source tree changed".to_owned());
        failures.extend(diff);
    }
    if states.is_empty() {
        failures.push("the run reported no mutant".to_owned());
    } else if states.iter().any(|state| state != "killed") {
        failures.push(format!("result: {}", states.join(", ")));
    }
    let mutant_reads = reads.iter().filter(|read| read.kind == "mutant").count();
    if reads.first().is_none_or(|read| read.kind != "original") || mutant_reads != states.len() {
        failures.push(
            "Cargo reads do not match one clean original and one mutant per result".to_owned(),
        );
    }
    for read in &reads {
        if read.resolved.starts_with(package) {
            failures.push(format!(
                "Cargo read the user package: {} {}",
                read.kind,
                read.resolved.display()
            ));
        }
    }
    let passed = failures.is_empty();
    let mut lines = vec![format!("{} {name}", if passed { "PASS" } else { "FAIL" })];
    if !passed {
        lines.extend(failures);
        if !reads.is_empty() {
            lines.push("reads:".to_owned());
            for read in &reads {
                lines.push(format!("  {} {}", read.kind, read.resolved.display()));
            }
        }
        if let Ok(run) = &run {
            for result in run.results() {
                if let Some(error) = &result.error {
                    lines.push(format!("detail: {error}"));
                }
            }
        }
    }
    ShapeReport { passed, lines }
}

struct BoolFlip;

impl Mutator for BoolFlip {
    fn name(&self) -> &str {
        "verify/bool-flip"
    }

    fn mutations(&self, source: &str) -> Vec<Mutation> {
        let Some(start) = source.find("true") else {
            return Vec::new();
        };
        vec![Mutation::new(start..start + "true".len(), "false")]
    }
}
