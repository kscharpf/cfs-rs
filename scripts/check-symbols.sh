#!/bin/sh
# Checks that the built binaries take each replaced function from Rust. Runs in the container,
# after `cfs-build.sh rs`.
#
# For each name of modules/msg/replaced.cmake, the cFE executable and the MSG unit-test runner
# must define both `<name>` (Rust) and `<name>_c` (the renamed C implementation). A unit-test
# runner without `<name>` would be testing the C function or a stub.
set -eu

root="$(cd "$(dirname "$0")/.." && pwd)"
build="$root/build/rs"
names="$(sed -n 's/^ *\(CFE_MSG_[A-Za-z0-9_]*\) *$/\1/p' "$root/modules/msg/replaced.cmake")"
[ -n "$names" ] || { echo "no replaced functions listed" >&2; exit 1; }

status=0
for binary in \
    "$build/native/default_cpu1/cpu1/core-cpu1" \
    "$build/native/default_cpu1/msg/stock/ut-coverage/coverage-msg-ALL-testrunner"
do
    [ -f "$binary" ] || { echo "missing: $binary" >&2; exit 1; }
    defined="$(nm --defined-only "$binary" | awk '$2 == "T" { print $3 }')"
    for name in $names; do
        for symbol in "$name" "${name}_c"; do
            if ! printf '%s\n' "$defined" | grep -qx "$symbol"; then
                echo "FAIL: $(basename "$binary") does not define $symbol"
                status=1
            fi
        done
    done
done
[ "$status" -eq 0 ] && echo "symbols: ok ($(echo "$names" | wc -w) replaced)"
exit "$status"
