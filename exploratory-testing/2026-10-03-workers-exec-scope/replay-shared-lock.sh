#!/bin/sh
# Replay: parallel workers share one Cargo build lock.
# Run on the host. Needs image mutarust-et:<tag> built from et.Dockerfile.
set -eu
IMG=${IMG:-mutarust-et:e212c49}
W=$(mktemp -d)
mkdir -p "$W/p/src"
cat > "$W/p/Cargo.toml" <<'T'
[package]
name = "p"
version = "0.1.0"
edition = "2021"
T
cat > "$W/p/src/lib.rs" <<'T'
pub fn f(x: u32) -> u32 { if x > 1 { x + 1 } else { x } }
#[cfg(test)]
mod tests { #[test] fn t() { assert_eq!(super::f(2), 3); } }
T
docker run --rm --cpus=2 --memory=1g --network=none -v "$W":/w -w /w/p "$IMG" sh -c '
  (for i in $(seq 1 40); do ls -l /tmp/mutarust-*/target/debug/.cargo-lock 2>/dev/null; sleep 0.25; done | sort -u > /tmp/locks) &
  mutarust --workers 2 . > /tmp/out 2>&1; echo "mutarust exit: $?"; wait
  echo "--- .cargo-lock entries seen during the run:"; sed "s/.*root //" /tmp/locks
  echo "--- result lines with Cargo lock waits: $(grep -c "Blocking waiting for file lock on build directory" /tmp/out)"'
rm -rf "$W"
