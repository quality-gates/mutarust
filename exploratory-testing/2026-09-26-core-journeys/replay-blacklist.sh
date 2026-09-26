#!/bin/sh
# Replay: the blacklist checksum is not available from any Mutarust output.
set -e
cd /tmp && rm -rf bl && mkdir -p bl/src && cd bl
printf '[package]\nname = "bl"\nversion = "0.1.0"\nedition = "2021"\n' > Cargo.toml
printf 'pub fn sum(a: u32, b: u32) -> u32 {\n    a + b\n}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn runs() {\n        let _ = super::sum(2, 1);\n    }\n}\n' > src/lib.rs
printf 'json_output: true\nhtml_output: true\n' > /tmp/bl.yml
mutarust --debug --config /tmp/bl.yml --logger-summary-json --logger-agentic-json --logger-gitlab --logger-github . > out.txt 2>&1 || true
ID=$(grep -m1 -A1 '^escaped' out.txt | sed -n 's/  ID: //p')
echo "first escaped ID: $ID"
echo "--- step 1: files that contain the word checksum:"
grep -l -i checksum out.txt report.json mutarust-report.html mutarust-agentic.json mutarust-gitlab.json mutarust-summary.json || echo "(none)"
echo "--- step 2: blacklist the printed ID"
echo "$ID" > ids.txt
mutarust --no-diffs --blacklist ids.txt . 2>&1 | grep -E "$ID|^Total"
echo "--- step 3: blacklist md5 of the changed lines (from src/evidence.rs)"
printf -- '-    a + b\n+    a - b\n' | md5sum | cut -c1-32 > sums.txt
cat sums.txt
mutarust --no-diffs --blacklist sums.txt . 2>&1 | grep -E "arithmetic/base|^Total"
echo "--- step 4: search every output for the working checksum value"
grep -l "$(cat sums.txt)" out.txt report.json mutarust-report.html mutarust-agentic.json mutarust-gitlab.json mutarust-summary.json || echo "(checksum value not in any output)"
