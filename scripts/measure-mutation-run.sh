#!/bin/sh
# Times one release mutarust process for a mutation run.
# The default case produces 100 mutations from one Rust source file.
# Container start is outside the timed region.
set -eu

case_name=${CASE:-hundred}
runs=${RUNS:-5}
root=${WORKLOAD_ROOT:-/opt/hill-workload}
container=${MEASURE_CONTAINER:-mutarust-measure}
binary=${MUTARUST_BIN:-/workspace/target/release/mutarust}
image=${MEASURE_IMAGE:-mutarust-dev}

case "$case_name" in
    hundred) expected=100 ;;
    few) expected=24 ;;
    *)
        printf 'unknown CASE %s\n' "$case_name" >&2
        exit 2
        ;;
esac

write_hundred() {
    crate=$root/hundred
    mkdir -p "$crate/src"
    cat > "$crate/Cargo.toml" <<'EOF'
[package]
name = "hundred"
version = "0.1.0"
edition = "2021"
EOF
    cat > "$crate/src/lib.rs" <<'EOF'
pub fn accumulate(left: i32, right: i32) -> i32 {
    let mut total = 0;
    if left > 0 && right < 10 {
        total += left + 1;
    } else {
        total -= 1;
    }
    if left > 0 && right < 10 {
        total += left + 1;
    } else {
        total -= 1;
    }
    if left > 0 && right < 10 {
        total += left + 1;
    } else {
        total -= 1;
    }
    if left > 0 && right < 10 {
        total += left + 1;
    } else {
        total -= 1;
    }
    if left > 0 { total += 1; }
    let flag = true;
    let _ = flag;
    total
}

#[cfg(test)]
mod checks {
    use super::accumulate;

    #[test]
    fn known_total() {
        assert_eq!(accumulate(2, 3), 13);
    }
}
EOF
}

write_few() {
    crate=$root/few
    mkdir -p "$crate/src"
    cat > "$crate/Cargo.toml" <<'EOF'
[package]
name = "few"
version = "0.1.0"
edition = "2021"
EOF
    cat > "$crate/src/lib.rs" <<'EOF'
pub fn accumulate(left: i32, right: i32) -> i32 {
    let mut total = 0;
    if left > 0 && right < 10 {
        total += left + 1;
    } else {
        total -= 1;
    }
    total
}

#[cfg(test)]
mod checks {
    use super::accumulate;

    #[test]
    fn known_total() {
        assert_eq!(accumulate(2, 3), 3);
    }
}
EOF
}

source_hash() {
    find "$1/src" "$1/Cargo.toml" -type f -exec sha256sum {} + | sort | sha256sum | cut -d ' ' -f 1
}

ensure_container() {
    if ! docker image inspect "$image" >/dev/null 2>&1; then
        printf 'missing image %s\n' "$image" >&2
        exit 2
    fi
    if ! docker container inspect "$container" >/dev/null 2>&1; then
        mkdir -p /var/tmp/mutarust-areas
        docker run -d --name "$container" \
            --cpus=1 --memory=512m --network=none \
            -v /workspace:/workspace \
            -v "$root:$root" \
            -v /var/tmp/mutarust-areas:/tmp \
            -e TMPDIR=/tmp \
            -w "$root" \
            "$image" sleep infinity >/dev/null
    fi
    running=$(docker inspect -f '{{.State.Running}}' "$container")
    if [ "$running" != "true" ]; then
        docker start "$container" >/dev/null
    fi
    limits=$(docker inspect -f '{{.HostConfig.NanoCpus}} {{.HostConfig.Memory}} {{.HostConfig.NetworkMode}}' "$container")
    if [ "$limits" != "1000000000 536870912 none" ]; then
        printf 'container %s limits are %s\n' "$container" "$limits" >&2
        exit 2
    fi
}

if [ ! -d /workspace ] || [ "$(pwd)" = "" ]; then
    :
fi
mkdir -p "$root"
write_hundred
write_few
ensure_container

crate=$root/$case_name
source=$crate/src/lib.rs
before=$(source_hash "$crate")

samples=""
i=0
while [ "$i" -lt "$runs" ]; do
    # Time only the mutarust process. The shell around it stays inside the
    # already running container.
    result=$(docker exec -e TMPDIR=/tmp "$container" bash -c "
        set -euo pipefail
        out=\$(mktemp)
        start=\$EPOCHREALTIME
        set +e
        \"$binary\" \"$source\" >\"\$out\" 2>\"\$out.err\"
        code=\$?
        set -e
        end=\$EPOCHREALTIME
        errored=\$(awk '/^Errored: / { print \$2; found=1 } END { if (!found) print \"missing\" }' \"\$out\")
        total=\$(awk '/^Total: / { print \$2; found=1 } END { if (!found) print \"missing\" }' \"\$out\")
        awk -v s=\"\$start\" -v e=\"\$end\" -v c=\"\$code\" -v err=\"\$errored\" -v tot=\"\$total\" 'BEGIN {
            printf \"seconds=%.6f exit=%s errors=%s mutations=%s\\n\", e - s, c, err, tot
        }'
        rm -f \"\$out\" \"\$out.err\"
    ")
    printf '%s\n' "$result" >&2
    seconds=$(printf '%s\n' "$result" | awk -F'[= ]' '/^seconds=/{print $2}')
    exit_code=$(printf '%s\n' "$result" | awk -F'[= ]' '/exit=/{print $4}')
    errors=$(printf '%s\n' "$result" | awk -F'[= ]' '/errors=/{print $6}')
    mutations=$(printf '%s\n' "$result" | awk -F'[= ]' '/mutations=/{print $8}')
    if [ "$exit_code" != "0" ] || [ "$errors" != "0" ] || [ "$mutations" != "$expected" ]; then
        printf 'run failed: %s\n' "$result" >&2
        exit 1
    fi
    samples="$samples $seconds"
    i=$((i + 1))
done

after=$(source_hash "$crate")
if [ "$before" != "$after" ]; then
    printf 'source tree changed\n' >&2
    exit 1
fi

summary=$(printf '%s\n' $samples | awk '
    { v[NR] = $1 }
    END {
        n = NR
        for (i = 1; i <= n; i++) {
            for (j = i + 1; j <= n; j++) {
                if (v[j] + 0 < v[i] + 0) {
                    t = v[i]; v[i] = v[j]; v[j] = t
                }
            }
        }
        if (n % 2 == 1) {
            med = v[(n + 1) / 2]
        } else {
            med = (v[n / 2] + v[n / 2 + 1]) / 2
        }
        printf "errors=0 mutations=%s seconds=%.6f runs=%d min=%.6f max=%.6f\n", "'"$expected"'", med, n, v[1], v[n]
    }
')
printf '%s\n' "$summary"
