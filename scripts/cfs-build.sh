#!/bin/sh
# Builds cFS and runs the MSG unit tests. Runs inside the container (see Makefile).
#
#   cfs-build.sh stock   cFS as released
#   cfs-build.sh rs      cFS with modules/msg of this repository in place of cfe/modules/msg
set -eu

variant="${1:?usage: cfs-build.sh stock|rs}"
root="$(cd "$(dirname "$0")/.." && pwd)"
build="$root/build/$variant"

case "$variant" in
    stock) unset CFS_APP_PATH ;;
    rs)    export CFS_APP_PATH="$root/modules" ;;
    *)     echo "unknown variant: $variant" >&2; exit 2 ;;
esac

cmake -S "$root/cfs/cfe" -B "$build" \
    -DMISSIONCONFIG=sample \
    -DSIMULATION=native \
    -DCFE_EDS_ENABLED=OFF \
    -DENABLE_UNIT_TESTS=TRUE \
    -DCMAKE_BUILD_TYPE=debug

make -C "$build" -j"$(nproc)" mission-install DESTDIR="$build"

cd "$build/native/default_cpu1"
ctest --output-on-failure -R 'coverage-msg'

if [ "$variant" = rs ]; then
    "$root/scripts/check-symbols.sh"
fi
