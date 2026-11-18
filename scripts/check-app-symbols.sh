#!/bin/sh
# Verifies that app shared libraries:
# 1. Export only their designated entry point.
# 2. Contain no heap allocation symbols.
# 3. Contain no panic/unwind symbols.
set -eu

cf_dir="${1:?usage: check-app-symbols.sh <path-to-cf-dir> [app.so:EntrySym ...]}"
shift

status=0

check_app() {
    app_so="$1"
    expected_sym="$2"
    base="$(basename "$app_so")"

    if [ ! -f "$app_so" ]; then
        echo "FAIL: missing $app_so" >&2
        status=1
        return
    fi

    # 1. Export visibility: In ELF, nm -D --defined-only output format is: <addr> <type> <name>
    syms="$(nm -D --defined-only "$app_so" | awk '{ print $3 }')"

    if ! printf '%s\n' "$syms" | grep -Fqx "$expected_sym"; then
        echo "FAIL: $base does not export $expected_sym" >&2
        status=1
        return
    fi

    other_syms="$(printf '%s\n' "$syms" | grep -Fxv "$expected_sym" || true)"
    if [ -n "$other_syms" ]; then
        echo "FAIL: $base exports unexpected symbols:" >&2
        printf '%s\n' "$other_syms" >&2
        status=1
        return
    fi

    # 2. Heap-freedom: verify .symtab is present and check .symtab + .dynsym for C/Rust allocators
    if ! readelf -SW "$app_so" | grep -qE '[[:space:]]\.symtab[[:space:]]'; then
        echo "FAIL: $base has no .symtab (stripped binary cannot be verified)" >&2
        status=1
        return
    fi

    all_demangled="$(printf '%s\n%s\n' "$(nm -C "$app_so")" "$(nm -CD "$app_so")")"
    heap_syms="$(printf '%s\n' "$all_demangled" | grep -E '(^|[[:space:]])(malloc|calloc|realloc|free)(@.*)?$|__rust_alloc|__rust_dealloc|__rust_realloc|__rust_alloc_zeroed|(^|[^[:alnum:]_:])alloc::' || true)"
    if [ -n "$heap_syms" ]; then
        echo "FAIL: $base contains heap allocation symbols:" >&2
        printf '%s\n' "$heap_syms" >&2
        status=1
        return
    fi

    # 3. Panic-freedom: check for Rust panic/unwind symbols
    panic_syms="$(printf '%s\n' "$all_demangled" | grep -E 'core::panicking|rust_begin_unwind|__rust_start_panic|panic_fmt' || true)"
    if [ -n "$panic_syms" ]; then
        echo "FAIL: $base contains panic/unwind symbols:" >&2
        printf '%s\n' "$panic_syms" >&2
        status=1
        return
    fi

    echo "app symbols ok: $base exports only $expected_sym, heap-free, panic-free"
}

if [ "$#" -gt 0 ]; then
    for spec in "$@"; do
        app_name="${spec%%:*}"
        entry_sym="${spec#*:}"
        if [ -z "$app_name" ] || [ -z "$entry_sym" ] || [ "$app_name" = "$spec" ]; then
            echo "FAIL: invalid app spec '$spec' (expected app.so:EntrySym)" >&2
            exit 1
        fi
        check_app "$cf_dir/$app_name" "$entry_sym"
    done
else
    check_app "$cf_dir/link_lab.so" "LINK_LAB_AppMain"
    check_app "$cf_dir/step_lab.so" "STEP_LAB_AppMain"
    check_app "$cf_dir/cmd_lab.so" "CMD_LAB_AppMain"
    check_app "$cf_dir/hk_lab.so" "HK_LAB_AppMain"
    check_app "$cf_dir/pl_lab.so" "PL_LAB_AppMain"
fi

exit "$status"
