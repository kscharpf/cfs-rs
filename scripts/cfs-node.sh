#!/bin/sh
# Builds core-cpu1 with node_defs on stock and rs MSG, verifies bindings freshness,
# checks app symbol visibility, and verifies start-up EVS events.
# Runs inside the container (see Makefile).
set -eu

root="$(cd "$(dirname "$0")/.." && pwd)"

mkdir -p "$root/build/node-stock" "$root/build/node-rs"
chmod -R a+rwX "$root/build" 2>/dev/null || true

# 1. Build core-cpu1 with node_defs on stock MSG
echo "=== Building node-stock ==="
export CFS_APP_PATH="$root/apps"
cmake -S "$root/cfs/cfe" -B "$root/build/node-stock" \
    -DMISSIONCONFIG=node \
    -DMISSION_DEFS="$root/node_defs" \
    -DSIMULATION=native \
    -DCFE_EDS_ENABLED=OFF \
    -DCMAKE_BUILD_TYPE=debug
make -C "$root/build/node-stock" -j"$(nproc)" mission-install DESTDIR="$root/build/node-stock"

# 2. Build core-cpu1 with node_defs on rs MSG
echo "=== Building node-rs ==="
export CFS_APP_PATH="$root/modules:$root/apps"
cmake -S "$root/cfs/cfe" -B "$root/build/node-rs" \
    -DMISSIONCONFIG=node \
    -DMISSION_DEFS="$root/node_defs" \
    -DSIMULATION=native \
    -DCFE_EDS_ENABLED=OFF \
    -DCMAKE_BUILD_TYPE=debug
make -C "$root/build/node-rs" -j"$(nproc)" mission-install DESTDIR="$root/build/node-rs"

chmod -R a+rwX "$root/build" 2>/dev/null || true

# 3. Regenerate crates/cfe-sys bindings and verify freshness
echo "=== Checking cfe-sys bindings freshness ==="
tmp_bindings="$(mktemp)"
bindgen \
  "$root/crates/cfe-sys/wrapper.h" \
  --use-core \
  --no-layout-tests \
  --clang-macro-fallback \
  --allowlist-function "CFE_ES_.*|CFE_EVS_.*|CFE_SB_.*|CFE_MSG_.*|OS_Socket.*|OS_Queue.*|OS_close|OS_read|OS_write|OS_TimedRead|OS_TimedWrite" \
  --allowlist-type "CFE_ES_RunStatus.*|CFE_EVS_.*|CFE_SB_.*|CFE_MSG_.*|OS_.*|osal_id_t|CFE_Status_t" \
  --allowlist-var "OS_.*|CFE_.*|NODE_.*" \
  -o "$tmp_bindings" \
  -- \
  -I"$root/node_defs" \
  -I"$root/build/node-stock/inc" \
  -I"$root/build/node-stock/native/default_cpu1/inc" \
  -I"$root/cfs/cfe/modules/core_api/fsw/inc" \
  -I"$root/cfs/cfe/modules/config/fsw/inc" \
  -I"$root/cfs/cfe/modules/es/fsw/inc" \
  -I"$root/cfs/cfe/modules/evs/fsw/inc" \
  -I"$root/cfs/cfe/modules/fs/fsw/inc" \
  -I"$root/cfs/cfe/modules/sb/fsw/inc" \
  -I"$root/cfs/cfe/modules/tbl/fsw/inc" \
  -I"$root/cfs/cfe/modules/time/fsw/inc" \
  -I"$root/cfs/osal/src/os/inc" \
  -I"$root/build/node-stock/native/default_cpu1/osal/inc" \
  -I"$root/cfs/psp/fsw/inc" \
  -I"$root/build/node-stock/native/default_cpu1/psp/inc" \
  -I"$root/cfs/psp/fsw/modules/iodriver/inc" \
  -I"$root/cfs/cfe/modules/msg/fsw/inc" \
  -I"$root/cfs/cfe/modules/resourceid/fsw/inc"

rustfmt --edition 2024 "$tmp_bindings"

if ! diff -u "$root/crates/cfe-sys/src/bindings.rs" "$tmp_bindings"; then
    echo "FAIL: crates/cfe-sys/src/bindings.rs is out of date with node headers" >&2
    rm -f "$tmp_bindings"
    exit 1
fi
rm -f "$tmp_bindings"
echo "cfe-sys bindings: up to date"

# 4. Check app symbol visibility on both builds
echo "=== Checking app symbol visibility (stock) ==="
"$root/scripts/check-app-symbols.sh" "$root/build/node-stock/usr/local/cpu1/cf"

echo "=== Checking app symbol visibility (rs) ==="
"$root/scripts/check-app-symbols.sh" "$root/build/node-rs/usr/local/cpu1/cf"

# 5. Fetch Aldis repository at pinned revision
aldis_rev="4852ee687ab64b22041dfb556c33ef0dc8666e83"
aldis_dir="$root/build/aldis-$aldis_rev"
if [ ! -d "$aldis_dir/.git" ]; then
    echo "=== Fetching Aldis at $aldis_rev ==="
    git clone https://github.com/kscharpf/aldis.git "$aldis_dir"
fi
(cd "$aldis_dir" && git checkout "$aldis_rev")

# 6. Run a0-empty via refapp-harness against core-cpu1 (twice per build)
run_a0() {
    build_type="$1"
    run_num="$2"
    staging="$root/build/$build_type/usr/local/cpu1"
    out_dir="$root/build/runs-$build_type-run$run_num"
    echo "=== Running a0-empty against $build_type (run $run_num) ==="
    cargo run --manifest-path "$aldis_dir/refapp/Cargo.toml" -p refapp-harness -- \
        run-node a0-empty --out "$out_dir" cfs "$staging" 5010

    # Verify byte-for-byte equality with golden trace
    if ! cmp -s "$out_dir/a0-empty.jsonl" "$aldis_dir/refapp/golden/a0-empty.jsonl"; then
        echo "FAIL: $out_dir/a0-empty.jsonl does not match golden trace" >&2
        diff -u "$aldis_dir/refapp/golden/a0-empty.jsonl" "$out_dir/a0-empty.jsonl" >&2 || true
        exit 1
    fi

    # Verify start-up EVS events in the node's console log
    grep -q "LINK_LAB App Initialized. Version 0.1.0" "$out_dir/a0-empty.node.log" || {
        echo "FAIL: LINK_LAB start-up event not found in $out_dir/a0-empty.node.log" >&2
        exit 1
    }
    grep -q "STEP_LAB App Initialized. Version 0.1.0" "$out_dir/a0-empty.node.log" || {
        echo "FAIL: STEP_LAB start-up event not found in $out_dir/a0-empty.node.log" >&2
        exit 1
    }

    echo "$build_type (run $run_num): a0-empty passed and matched golden trace byte-for-byte"
}

run_a0 "node-stock" 1
run_a0 "node-stock" 2
run_a0 "node-rs" 1
run_a0 "node-rs" 2

echo "=== node checks: ALL PASSED ==="
