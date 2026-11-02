#!/bin/sh
set -eu
cd /work
export CARGO_NET_OFFLINE=true
mkdir -p /work/.scratch
out=$(mktemp -d /work/.scratch/replay.XXXXXX)
export TMPDIR="$out/tmp"
export CARGO_TARGET_DIR="$out/consumer-target"
bin=/work/.scratch/target/debug/mutarust
pkg="$out/pricing"
mkdir -p "$pkg/src" "$TMPDIR"
printf 'Replay output: %s\n' "$out"
printf '[package]\nname = "pricing"\nversion = "0.1.0"\nedition = "2021"\n' > "$pkg/Cargo.toml"
printf 'pub fn checked() -> bool { let value = true; value }\npub fn unchecked() -> bool { let value = true; value }\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn checked_works() { assert!(super::checked()); }\n}\n' > "$pkg/src/lib.rs"
cd "$pkg"
cargo generate-lockfile --offline
cat > mutarust.yml <<'YAML'
json_output: true
html_output: true
enable_mutators:
  - conditional/bool-literal
YAML
git init -q
git config user.email test@example.invalid
git config user.name Explorer
git add Cargo.toml Cargo.lock src/lib.rs mutarust.yml
git commit -qm initial
run() {
  label=$1; shift
  printf '\n$ %s\n' "$*"
  set +e
  "$@"
  status=$?
  set -e
  printf 'exit=%s\n' "$status"
}
{
  echo 'Journey 1: reports and score gate'
  sha256sum src/lib.rs
  run normal "$bin" --workers 1 --config mutarust.yml --logger-summary-json --logger-agentic-json --logger-gitlab .
  for f in report.json mutarust-summary.json mutarust-agentic.json mutarust-gitlab.json; do echo "--- $f"; cat "$f"; done
  echo '--- HTML evidence'; grep -o 'No escaped mutants\|checked/src/lib.rs\|pricing/src/lib.rs\|unchecked\|Escaped' mutarust-report.html | sort | uniq -c || true
  run gate "$bin" --workers 1 --config mutarust.yml --min-msi 100 .
  sha256sum src/lib.rs
} > "$out/journey1.txt" 2>&1
{
  echo 'Journey 2: accepted baseline'
  rm -f mutarust-baseline.json
  run update "$bin" --workers 1 --enable conditional/bool-literal --update-baseline .
  echo '--- baseline'; cat mutarust-baseline.json
  run accepted "$bin" --workers 1 --enable conditional/bool-literal --baseline mutarust-baseline.json --fail-on-escaped .
  printf '\npub fn new_unchecked() -> bool { let value = false; value }\n' >> src/lib.rs
  run new_escape "$bin" --workers 1 --enable conditional/bool-literal --baseline mutarust-baseline.json --fail-on-escaped .
  echo '--- baseline after new source'; cat mutarust-baseline.json
} > "$out/journey2.txt" 2>&1
{
  echo 'Journey 3: changed-line selection'
  git checkout -- src/lib.rs
  run unchanged "$bin" --git-diff-lines --git-diff-base HEAD --enable conditional/bool-literal --dry-run .
  printf '\npub fn new_unchecked() -> bool { let value = false; value }\n' >> src/lib.rs
  git diff -- src/lib.rs
  run changed "$bin" --git-diff-lines --git-diff-base HEAD --enable conditional/bool-literal --workers 1 --logger-summary-json .
  echo '--- changed summary'; cat mutarust-summary.json
  echo '--- source after run'; git diff -- src/lib.rs
} > "$out/journey3.txt" 2>&1
