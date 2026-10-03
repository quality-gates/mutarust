#!/bin/sh
# Run "$@" and sample concurrent rustc and blocked cargo counts every 0.5 s.
"$@" & pid=$!; maxr=0; samples=""
while kill -0 $pid 2>/dev/null; do
  r=$(ps -eo comm | grep -c '^rustc$'); c=$(ps -eo args | grep -c '^[^ ]*cargo test')
  [ "$r" -gt "$maxr" ] && maxr=$r; samples="$samples $r/$c"
  sleep 0.5
done
wait $pid; echo "MAX_CONCURRENT_RUSTC=$maxr"; echo "SAMPLES(rustc/cargo-test):$samples"
