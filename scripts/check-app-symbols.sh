#!/bin/sh
# Verifies that app shared libraries export only their designated entry point.
set -eu

cf_dir="${1:?usage: check-app-symbols.sh <path-to-cf-dir>}"

status=0

check_app() {
    app_so="$1"
    expected_sym="$2"

    if [ ! -f "$app_so" ]; then
        echo "FAIL: missing $app_so" >&2
        status=1
        return
    fi

    # In ELF, nm -D --defined-only output format is: <addr> <type> <name>
    syms="$(nm -D --defined-only "$app_so" | awk '{ print $3 }')"

    # Check that expected_sym is present
    if ! printf '%s\n' "$syms" | grep -qx "$expected_sym"; then
        echo "FAIL: $(basename "$app_so") does not export $expected_sym" >&2
        status=1
        return
    fi

    # Check that NO other symbols are exported
    other_syms="$(printf '%s\n' "$syms" | grep -v "^${expected_sym}$" || true)"
    if [ -n "$other_syms" ]; then
        echo "FAIL: $(basename "$app_so") exports unexpected symbols:" >&2
        printf '%s\n' "$other_syms" >&2
        status=1
        return
    fi

    echo "app symbols ok: $(basename "$app_so") exports only $expected_sym"
}

check_app "$cf_dir/link_lab.so" "LINK_LAB_AppMain"
check_app "$cf_dir/step_lab.so" "STEP_LAB_AppMain"
check_app "$cf_dir/scn_lab.so" "SCN_LAB_AppMain"

exit "$status"
