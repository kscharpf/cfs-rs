//! C exports of `cfe-msg` under the cFE MSG function names.
//!
//! The header is cFE's own `cfe_msg.h`; this crate generates none. Every function exported here
//! must also be listed in `modules/msg/replaced.cmake`, which renames the C implementation out
//! of the way at build time.
//!
//! All `unsafe` of the MSG replacement belongs in this crate.

#![cfg_attr(not(test), no_std)]

use cfe_msg::{PRIMARY_HEADER_LEN, PacketType};

/// `CFE_Status_t` of `cfe_error.h`.
pub type CfeStatus = i32;

/// `CFE_SUCCESS` of `cfe_error.h`.
const CFE_SUCCESS: CfeStatus = 0;

/// `CFE_MSG_BAD_ARGUMENT` of `cfe_error.h`.
#[expect(
    clippy::cast_possible_wrap,
    reason = "CFE_MSG_BAD_ARGUMENT is 0xCA000003 as a signed 32-bit CFE_Status_t"
)]
const CFE_MSG_BAD_ARGUMENT: CfeStatus = 0xca00_0003_u32 as i32;

/// `CFE_MSG_SequenceCount_t` of `cfe_msg_api_typedefs.h`.
pub type CfeMsgSequenceCount = u16;

/// `CFE_MSG_Size_t` of `cfe_msg_api_typedefs.h`.
pub type CfeMsgSize = usize;

/// `CFE_MSG_HeaderVersion_t` of `cfe_msg_api_typedefs.h`.
pub type CfeMsgHeaderVersion = u16;

/// `CFE_MSG_Type_t` of `cfe_msg_api_typedefs.h`.
pub type CfeMsgType = core::ffi::c_uint;

/// `CFE_MSG_Type_Cmd` of `cfe_msg_api_typedefs.h`.
const CFE_MSG_TYPE_CMD: CfeMsgType = 1;

/// `CFE_MSG_Type_Tlm` of `cfe_msg_api_typedefs.h`.
const CFE_MSG_TYPE_TLM: CfeMsgType = 2;

/// `CFE_MSG_Message_t` of `cfe_msg_api_typedefs.h` / `default_cfe_msg_hdr_pri.h`.
#[repr(C)]
pub struct CfeMsgMessage {
    /// CCSDS Primary Header bytes (`CCSDS_PrimaryHeader_t`).
    pri: [u8; PRIMARY_HEADER_LEN],
}

/// `CFE_MSG_GetNextSequenceCount` of `cfe_msg.h`.
#[unsafe(no_mangle)]
pub const extern "C" fn CFE_MSG_GetNextSequenceCount(
    seq_cnt: CfeMsgSequenceCount,
) -> CfeMsgSequenceCount {
    cfe_msg::next_sequence_count(seq_cnt)
}

/// `CFE_MSG_GetSize` of `cfe_msg.h`.
///
/// # Safety
/// If non-null, `msg_ptr` must point to a valid `CFE_MSG_Message_t` and `size` must point to
/// writable `CFE_MSG_Size_t` storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn CFE_MSG_GetSize(
    msg_ptr: *const CfeMsgMessage,
    size: *mut CfeMsgSize,
) -> CfeStatus {
    if msg_ptr.is_null() || size.is_null() {
        return CFE_MSG_BAD_ARGUMENT;
    }
    // SAFETY: `msg_ptr` and `size` are non-null, and the caller guarantees they point to valid
    // `CFE_MSG_Message_t` and `CFE_MSG_Size_t` objects for the duration of this call.
    unsafe {
        *size = cfe_msg::get_size(&(*msg_ptr).pri);
    }
    CFE_SUCCESS
}

/// `CFE_MSG_SetSize` of `cfe_msg.h`.
///
/// # Safety
/// If non-null, `msg_ptr` must point to a valid mutable `CFE_MSG_Message_t`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn CFE_MSG_SetSize(
    msg_ptr: *mut CfeMsgMessage,
    size: CfeMsgSize,
) -> CfeStatus {
    if msg_ptr.is_null() {
        return CFE_MSG_BAD_ARGUMENT;
    }
    // SAFETY: `msg_ptr` is non-null, and the caller guarantees it points to a valid mutable
    // `CFE_MSG_Message_t` for the duration of this call.
    let res = unsafe { cfe_msg::set_size(&mut (*msg_ptr).pri, size) };
    if res.is_ok() {
        CFE_SUCCESS
    } else {
        CFE_MSG_BAD_ARGUMENT
    }
}

/// `CFE_MSG_GetType` of `cfe_msg.h`.
///
/// # Safety
/// If non-null, `msg_ptr` must point to a valid `CFE_MSG_Message_t` and `msg_type` must point to
/// writable `CFE_MSG_Type_t` storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn CFE_MSG_GetType(
    msg_ptr: *const CfeMsgMessage,
    msg_type: *mut CfeMsgType,
) -> CfeStatus {
    if msg_ptr.is_null() || msg_type.is_null() {
        return CFE_MSG_BAD_ARGUMENT;
    }
    // SAFETY: `msg_ptr` and `msg_type` are non-null, and the caller guarantees they point to
    // valid `CFE_MSG_Message_t` and `CFE_MSG_Type_t` objects for the duration of this call.
    unsafe {
        *msg_type = match cfe_msg::get_type(&(*msg_ptr).pri) {
            PacketType::Telecommand => CFE_MSG_TYPE_CMD,
            PacketType::Telemetry => CFE_MSG_TYPE_TLM,
        };
    }
    CFE_SUCCESS
}

/// `CFE_MSG_SetType` of `cfe_msg.h`.
///
/// # Safety
/// If non-null, `msg_ptr` must point to a valid mutable `CFE_MSG_Message_t`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn CFE_MSG_SetType(
    msg_ptr: *mut CfeMsgMessage,
    msg_type: CfeMsgType,
) -> CfeStatus {
    if msg_ptr.is_null() {
        return CFE_MSG_BAD_ARGUMENT;
    }
    let packet_type = match msg_type {
        CFE_MSG_TYPE_CMD => PacketType::Telecommand,
        CFE_MSG_TYPE_TLM => PacketType::Telemetry,
        _ => return CFE_MSG_BAD_ARGUMENT,
    };
    // SAFETY: `msg_ptr` is non-null, and the caller guarantees it points to a valid mutable
    // `CFE_MSG_Message_t` for the duration of this call.
    unsafe {
        cfe_msg::set_type(&mut (*msg_ptr).pri, packet_type);
    }
    CFE_SUCCESS
}

/// `CFE_MSG_GetHasSecondaryHeader` of `cfe_msg.h`.
///
/// # Safety
/// If non-null, `msg_ptr` must point to a valid `CFE_MSG_Message_t` and `has_secondary` must
/// point to writable `bool` storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn CFE_MSG_GetHasSecondaryHeader(
    msg_ptr: *const CfeMsgMessage,
    has_secondary: *mut bool,
) -> CfeStatus {
    if msg_ptr.is_null() || has_secondary.is_null() {
        return CFE_MSG_BAD_ARGUMENT;
    }
    // SAFETY: `msg_ptr` and `has_secondary` are non-null, and the caller guarantees they point to
    // valid `CFE_MSG_Message_t` and `bool` objects for the duration of this call.
    unsafe {
        *has_secondary = cfe_msg::get_has_secondary_header(&(*msg_ptr).pri);
    }
    CFE_SUCCESS
}

/// `CFE_MSG_SetHasSecondaryHeader` of `cfe_msg.h`.
///
/// # Safety
/// If non-null, `msg_ptr` must point to a valid mutable `CFE_MSG_Message_t`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn CFE_MSG_SetHasSecondaryHeader(
    msg_ptr: *mut CfeMsgMessage,
    has_secondary: bool,
) -> CfeStatus {
    if msg_ptr.is_null() {
        return CFE_MSG_BAD_ARGUMENT;
    }
    // SAFETY: `msg_ptr` is non-null, and the caller guarantees it points to a valid mutable
    // `CFE_MSG_Message_t` for the duration of this call.
    unsafe {
        cfe_msg::set_has_secondary_header(&mut (*msg_ptr).pri, has_secondary);
    }
    CFE_SUCCESS
}

/// `CFE_MSG_GetHeaderVersion` of `cfe_msg.h`.
///
/// # Safety
/// If non-null, `msg_ptr` must point to a valid `CFE_MSG_Message_t` and `version` must point to
/// writable `CFE_MSG_HeaderVersion_t` storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn CFE_MSG_GetHeaderVersion(
    msg_ptr: *const CfeMsgMessage,
    version: *mut CfeMsgHeaderVersion,
) -> CfeStatus {
    if msg_ptr.is_null() || version.is_null() {
        return CFE_MSG_BAD_ARGUMENT;
    }
    // SAFETY: `msg_ptr` and `version` are non-null, and the caller guarantees they point to valid
    // `CFE_MSG_Message_t` and `CFE_MSG_HeaderVersion_t` objects for the duration of this call.
    unsafe {
        *version = cfe_msg::get_header_version(&(*msg_ptr).pri);
    }
    CFE_SUCCESS
}

/// `CFE_MSG_SetHeaderVersion` of `cfe_msg.h`.
///
/// # Safety
/// If non-null, `msg_ptr` must point to a valid mutable `CFE_MSG_Message_t`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn CFE_MSG_SetHeaderVersion(
    msg_ptr: *mut CfeMsgMessage,
    version: CfeMsgHeaderVersion,
) -> CfeStatus {
    if msg_ptr.is_null() {
        return CFE_MSG_BAD_ARGUMENT;
    }
    // SAFETY: `msg_ptr` is non-null, and the caller guarantees it points to a valid mutable
    // `CFE_MSG_Message_t` for the duration of this call.
    let res = unsafe { cfe_msg::set_header_version(&mut (*msg_ptr).pri, version) };
    if res.is_ok() {
        CFE_SUCCESS
    } else {
        CFE_MSG_BAD_ARGUMENT
    }
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
