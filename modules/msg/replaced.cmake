# The CFE_MSG functions that the Rust library implements.
#
# For each name, the C implementation in cFE is compiled as `<name>_c`, so the Rust symbol is
# the one the rest of cFE links to. The C version stays in the library for differential tests.
# Keep this list equal to the `#[unsafe(no_mangle)]` functions of `crates/cfe-msg-ffi`.
set(CFE_MSG_RS_REPLACED
    CFE_MSG_GetNextSequenceCount
    CFE_MSG_GetSize
    CFE_MSG_SetSize
    CFE_MSG_GetType
    CFE_MSG_SetType
    CFE_MSG_GetHasSecondaryHeader
    CFE_MSG_SetHasSecondaryHeader
    CFE_MSG_GetHeaderVersion
    CFE_MSG_SetHeaderVersion
    CFE_MSG_GetApId
    CFE_MSG_SetApId
    CFE_MSG_GetSegmentationFlag
    CFE_MSG_SetSegmentationFlag
    CFE_MSG_GetSequenceCount
    CFE_MSG_SetSequenceCount
    CFE_MSG_GetFcnCode
    CFE_MSG_SetFcnCode
)
