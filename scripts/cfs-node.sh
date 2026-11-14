#!/bin/sh
# Builds core-cpu1 with node_defs on stock and rs MSG, verifies bindings freshness,
# checks app symbol visibility, and verifies start-up EVS events.
# Runs inside the container (see Makefile).
set -eu

root="$(cd "$(dirname "$0")/.." && pwd)"

mkdir -p "$root/build/node-stock" "$root/build/node-rs"
chmod -R a+rwX "$root/build"

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

chmod -R a+rwX "$root/build"

# 3. Regenerate crates/cfe-sys bindings and verify freshness
echo "=== Checking cfe-sys bindings freshness ==="
tmp_bindings="$(mktemp)"
bindgen \
  "$root/crates/cfe-sys/wrapper.h" \
  --use-core \
  --no-layout-tests \
  --clang-macro-fallback \
  --allowlist-function "CFE_ES_.*|CFE_EVS_.*|CFE_SB_.*|CFE_MSG_.*|OS_Socket.*|OS_close" \
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

# 5. Run core-cpu1 on stock MSG and check EVS events
echo "=== Running core-cpu1 on stock MSG ==="
stock_log="$(mktemp)"
(
    cd "$root/build/node-stock/usr/local/cpu1"
    su nobody -s /bin/sh -c "timeout --kill-after=1s 3s ./core-cpu1" > "$stock_log" 2>&1 || true
)
cat "$stock_log"
grep -q "LINK_LAB App Initialized. Version 0.1.0" "$stock_log" || {
    echo "FAIL: LINK_LAB start-up event not found in node-stock log" >&2
    rm -f "$stock_log"
    exit 1
}
grep -q "STEP_LAB App Initialized. Version 0.1.0" "$stock_log" || {
    echo "FAIL: STEP_LAB start-up event not found in node-stock log" >&2
    rm -f "$stock_log"
    exit 1
}
rm -f "$stock_log"
echo "node-stock: start-up EVS events verified"

# 6. Run core-cpu1 on rs MSG and check EVS events
echo "=== Running core-cpu1 on rs MSG ==="
rs_log="$(mktemp)"
(
    cd "$root/build/node-rs/usr/local/cpu1"
    su nobody -s /bin/sh -c "timeout --kill-after=1s 3s ./core-cpu1" > "$rs_log" 2>&1 || true
)
cat "$rs_log"
grep -q "LINK_LAB App Initialized. Version 0.1.0" "$rs_log" || {
    echo "FAIL: LINK_LAB start-up event not found in node-rs log" >&2
    rm -f "$rs_log"
    exit 1
}
grep -q "STEP_LAB App Initialized. Version 0.1.0" "$rs_log" || {
    echo "FAIL: STEP_LAB start-up event not found in node-rs log" >&2
    rm -f "$rs_log"
    exit 1
}
rm -f "$rs_log"
echo "node-rs: start-up EVS events verified"

echo "=== node checks: ALL PASSED ==="
