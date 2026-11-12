# The CFE_MSG functions that the Rust library implements.
#
# For each name, the C implementation in cFE is compiled as `<name>_c`, so the Rust symbol is
# the one the rest of cFE links to. The C version stays in the library for differential tests.
# Keep this list equal to the `#[unsafe(no_mangle)]` functions of `crates/cfe-msg-ffi`.
set(CFE_MSG_RS_REPLACED
    CFE_MSG_GetNextSequenceCount
)
