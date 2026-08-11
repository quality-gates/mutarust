#!/bin/sh
set -eu

binary=${1:-target/release/mutarust}
case "$binary" in
    /*) ;;
    *) binary=$(cd "$(dirname "$binary")" && pwd)/$(basename "$binary") ;;
esac
runs=${RUNS:-5}
root=${TMPDIR:-/tmp}/mutarust-planning-benchmark
rm -rf "$root"
mkdir -p "$root/src"
printf '[package]\nname = "planning-benchmark"\nversion = "0.0.0"\nedition = "2024"\n' > "$root/Cargo.toml"
: > "$root/src/lib.rs"
index=0
while [ "$index" -lt 256 ]; do
    printf 'pub mod module_%s;\n' "$index" >> "$root/src/lib.rs"
    cat > "$root/src/module_$index.rs" <<EOF
pub fn enabled_$index(input: bool) -> bool {
    if input { true } else { false }
}

pub fn calculate_$index(left: i32, right: i32) -> i32 {
    let total = left + right;
    if total > $index { total - 1 } else { total + 1 }
}
EOF
    index=$((index + 1))
done

source_hash() {
    find "$root/src" -type f -exec sha256sum {} + | sort | sha256sum | cut -d ' ' -f 1
}

before=$(source_hash)
expected='Total: 3840 mutation(s) would be generated. No files written, no tests run.'
run_once() {
    output=$(cd "$root" && "$binary" --dry-run .)
    [ "$output" = "$expected" ] || {
        printf 'unexpected output:\n%s\n' "$output" >&2
        return 1
    }
}

run_once
i=0
values=''
while [ "$i" -lt "$runs" ]; do
    start=$(date +%s%N)
    run_once
    end=$(date +%s%N)
    elapsed=$(((end - start) / 1000000))
    values="$values $elapsed"
    i=$((i + 1))
done
after=$(source_hash)
[ "$before" = "$after" ] || {
    printf 'source tree changed\n' >&2
    exit 1
}
printf 'milliseconds:%s\n' "$values"
printf 'mutants: 3840\nsource_sha256: %s\n' "$after"
