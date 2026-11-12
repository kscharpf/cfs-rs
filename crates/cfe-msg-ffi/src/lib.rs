//! C exports of `cfe-msg` under the cFE MSG function names.
//!
//! The header is cFE's own `cfe_msg.h`; this crate generates none. Every function exported here
//! must also be listed in `modules/msg/replaced.cmake`, which renames the C implementation out
//! of the way at build time.
//!
//! All `unsafe` of the MSG replacement belongs in this crate.

#![cfg_attr(not(test), no_std)]

/// `CFE_MSG_SequenceCount_t` of `cfe_msg_api_typedefs.h`.
pub type CfeMsgSequenceCount = u16;

/// `CFE_MSG_GetNextSequenceCount` of `cfe_msg.h`.
#[unsafe(no_mangle)]
pub const extern "C" fn CFE_MSG_GetNextSequenceCount(
    seq_cnt: CfeMsgSequenceCount,
) -> CfeMsgSequenceCount {
    cfe_msg::next_sequence_count(seq_cnt)
}

#[cfg(not(test))]
unsafe extern "C" {
    /// `abort` of the C library.
    safe fn abort() -> !;
}

/// The crate is meant to be panic-free; if a panic is reached anyway, stop the process.
#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo<'_>) -> ! {
    abort()
}
