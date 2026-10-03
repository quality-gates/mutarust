#!/bin/sh
# Replay: an exclude_dirs entry that starts with "./" excludes nothing.
set -eu
IMG=${IMG:-mutarust-et:e212c49}
W=$(mktemp -d)
mkdir -p "$W/p/src/gen"
printf '[package]\nname = "p"\nversion = "0.1.0"\nedition = "2021"\n' > "$W/p/Cargo.toml"
printf 'pub mod gen;\npub fn a(x: i32) -> bool { x > 1 }\n' > "$W/p/src/lib.rs"
printf 'pub fn b(x: i32) -> bool { x > 2 }\n' > "$W/p/src/gen/mod.rs"
docker run --rm --cpus=1 --memory=512m --network=none -v "$W":/w -w /w/p "$IMG" sh -c '
  for p in src/gen ./src/gen; do
    printf "exclude_dirs: [\"%s\"]\n" "$p" > /tmp/c.yml
    printf "%-10s " "$p"; mutarust --config /tmp/c.yml --exec false --no-diffs . | grep -E "^escaped" | cut -d" " -f2 | sort | uniq -c | tr "\n" " "; echo
  done'
rm -rf "$W"
