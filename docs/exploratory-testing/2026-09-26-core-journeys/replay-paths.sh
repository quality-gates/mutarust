#!/bin/sh
# Replay: report paths are relative to the Cargo workspace, not the Git repository.
set -e
cd /tmp && rm -rf repo && mkdir -p repo/rust/calc/src && cd repo
printf '[package]\nname = "calc"\nversion = "0.1.0"\nedition = "2021"\n' > rust/calc/Cargo.toml
printf 'pub fn sum(a: u32, b: u32) -> u32 {\n    a + b\n}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn runs() {\n        let _ = super::sum(2, 1);\n    }\n}\n' > rust/calc/src/lib.rs
git init -q && git add rust && git commit -qm init
echo "Git repository root: $(git rev-parse --show-toplevel)"
mutarust --quiet --no-diffs --logger-github --logger-gitlab --logger-agentic-json rust/calc 2>&1 | grep '::warning' || true
python3 -c "import json;print('gitlab location.path:', [m['location']['path'] for m in json.load(open('mutarust-gitlab.json'))]);print('agentic file:', [m['file'] for m in json.load(open('mutarust-agentic.json'))['mutants']])"
for p in src/lib.rs rust/calc/src/lib.rs; do [ -f "$p" ] && echo "exists at repo root: $p" || echo "missing at repo root: $p"; done
