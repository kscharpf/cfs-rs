#!/bin/sh
# Builds core-cpu1 with realtime_defs on rs MSG, checks app symbols, launches core-cpu1,
# and runs ground UDP TM/TC frame verification against it.
# Runs inside the container (see Makefile).
set -eu

root="$(cd "$(dirname "$0")/.." && pwd)"
build_dir="$root/build/realtime-rs"

mkdir -p "$build_dir"
chmod -R a+rwX "$root/build" 2>/dev/null || true

echo "=== Building realtime-rs ==="
export CFS_APP_PATH="$root/modules:$root/apps"
cmake -S "$root/cfs/cfe" -B "$build_dir" \
    -DMISSIONCONFIG=realtime \
    -DMISSION_DEFS="$root/realtime_defs" \
    -DSIMULATION=native \
    -DCFE_EDS_ENABLED=OFF \
    -DCMAKE_BUILD_TYPE=debug
make -C "$build_dir" -j"$(nproc)" mission-install DESTDIR="$build_dir"

chmod -R a+rwX "$root/build" 2>/dev/null || true

staging="$build_dir/usr/local/cpu1"
log_file="$build_dir/core-cpu1.log"

echo "=== Checking app symbol visibility (realtime-rs) ==="
"$root/scripts/check-app-symbols.sh" "$staging/cf" "link_lab.so:LINK_LAB_AppMain"

if [ ! -x "$staging/core-cpu1" ]; then
    echo "FAIL: missing $staging/core-cpu1" >&2
    exit 1
fi

echo "=== Starting core-cpu1 in NativeRealtime mode ==="
cfs_pid=""
cleanup() {
    if [ -n "$cfs_pid" ] && kill -0 "$cfs_pid" 2>/dev/null; then
        kill "$cfs_pid" 2>/dev/null || true
        wait "$cfs_pid" 2>/dev/null || true
    fi
}
trap cleanup EXIT INT TERM

(cd "$staging" && ./core-cpu1 > "$log_file" 2>&1) &
cfs_pid=$!

echo "=== Running ground UDP TM/TC verification ==="
if ! cargo test --locked --manifest-path "$root/Cargo.toml" \
    -p link-lab-core --test realtime_ground -- --ignored --nocapture; then
    echo "=== core-cpu1 log ===" >&2
    cat "$log_file" >&2 || true
    exit 1
fi

grep -q "LINK_LAB App Initialized. Version 0.1.0" "$log_file" || {
    echo "FAIL: LINK_LAB start-up event not found in $log_file" >&2
    cat "$log_file" >&2 || true
    exit 1
}

echo "=== realtime check: ALL PASSED ==="
