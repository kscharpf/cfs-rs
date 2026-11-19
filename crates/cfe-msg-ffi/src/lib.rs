//! C exports of `cfe-msg` under the cFE MSG function names.
//!
//! The header is cFE's own `cfe_msg.h`; this crate generates none. Every function exported here
//! must also be listed in `modules/msg/replaced.cmake`, which renames the C implementation out
//! of the way at build time.
//!
//! All `unsafe` of the MSG replacement belongs in this crate.

#![cfg_attr(not(test), no_std)]
#![expect(unsafe_code, reason = "FFI crate implements C ABI exports")]

use cfe_msg::{
    BadArgument, COMMAND_HEADER_LEN, MsgError, MsgId, PRIMARY_HEADER_LEN, PacketType,
    SequenceFlags, SysTime, TELEMETRY_HEADER_LEN,
};

/// `CFE_Status_t` of `cfe_error.h`.
pub type CfeStatus = i32;

/// `CFE_SUCCESS` of `cfe_error.h`.
const CFE_SUCCESS: CfeStatus = 0;

/// `CFE_MSG_BAD_ARGUMENT` of `cfe_error.h`.
const CFE_MSG_BAD_ARGUMENT: CfeStatus = 0xca00_0003_u32.cast_signed();

/// `CFE_MSG_WRONG_MSG_TYPE` (`CFE_SB_WRONG_MSG_TYPE`) of `cfe_error.h`.
const CFE_MSG_WRONG_MSG_TYPE: CfeStatus = 0xca00_000d_u32.cast_signed();

/// `CFE_MSG_ApId_t` of `cfe_msg_api_typedefs.h`.
pub type CfeMsgApId = u16;

/// `CFE_MSG_FcnCode_t` of `cfe_msg_api_typedefs.h`.
pub type CfeMsgFcnCode = u16;

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

/// `CFE_MSG_SegmentationFlag_t` of `cfe_msg_api_typedefs.h`.
pub type CfeMsgSegmentationFlag = core::ffi::c_uint;

/// `CFE_MSG_SegFlag_Continue` of `cfe_msg_api_typedefs.h`.
const CFE_MSG_SEG_FLAG_CONTINUE: CfeMsgSegmentationFlag = 1;

/// `CFE_MSG_SegFlag_First` of `cfe_msg_api_typedefs.h`.
const CFE_MSG_SEG_FLAG_FIRST: CfeMsgSegmentationFlag = 2;

/// `CFE_MSG_SegFlag_Last` of `cfe_msg_api_typedefs.h`.
const CFE_MSG_SEG_FLAG_LAST: CfeMsgSegmentationFlag = 3;

/// `CFE_MSG_SegFlag_Unsegmented` of `cfe_msg_api_typedefs.h`.
const CFE_MSG_SEG_FLAG_UNSEGMENTED: CfeMsgSegmentationFlag = 4;

/// Maps a fallible core operation to `CFE_SUCCESS` or `CFE_MSG_BAD_ARGUMENT`.
const fn status_from_result(res: Result<(), BadArgument>) -> CfeStatus {
    if res.is_ok() {
        CFE_SUCCESS
    } else {
        CFE_MSG_BAD_ARGUMENT
    }
}

/// `CFE_MSG_Message_t` of `cfe_msg_api_typedefs.h` / `default_cfe_msg_hdr_pri.h`.
#[repr(C)]
pub struct CfeMsgMessage {
    /// CCSDS Primary Header bytes (`CCSDS_PrimaryHeader_t`).
    pri: [u8; PRIMARY_HEADER_LEN],
}

/// `CFE_MSG_CommandSecondaryHeader_t` of `default_cfe_msg_sechdr.h`.
#[repr(C)]
pub struct CfeMsgCommandSecondaryHeader {
    /// Command function code (bits 0..=6) and reserved bit 7.
    function_code: u8,
    /// Command checksum.
    checksum: u8,
}

/// `CFE_MSG_CommandHeader_t` of `cfe_msg_api_typedefs.h` / `default_cfe_msg_hdr_pri.h`.
#[repr(C)]
pub struct CfeMsgCommandHeader {
    /// Base message (`CFE_MSG_Message_t`).
    msg: CfeMsgMessage,
    /// Command secondary header (`CFE_MSG_CommandSecondaryHeader_t`).
    sec: CfeMsgCommandSecondaryHeader,
}

/// `CFE_TIME_SysTime_t` of `cfe_time_extern_typedefs.h` / `default_cfe_time_extern_typedefs.h`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CfeTimeSysTime {
    /// Number of seconds since epoch.
    seconds: u32,
    /// Number of subseconds since epoch (LSB = 2^(-32) seconds).
    subseconds: u32,
}

/// `CFE_SB_MsgId_Atom_t` of `default_cfe_sb_extern_typedefs.h`.
pub type CfeSbMsgIdAtom = u32;

/// `CFE_SB_MsgId_t` of `default_cfe_sb_extern_typedefs.h`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CfeSbMsgId {
    /// Underlying message ID value.
    value: CfeSbMsgIdAtom,
}

impl CfeSbMsgId {
    /// Extracts the low 16-bit CCSDS message ID from `self`.
    #[must_use]
    const fn to_msg_id(self) -> MsgId {
        let [b0, b1, _, _] = self.value.to_le_bytes();
        MsgId::new(u16::from_le_bytes([b0, b1]))
    }
}

/// `CFE_MSG_TelemetrySecondaryHeader_t` of `default_cfe_msg_sechdr.h`.
#[repr(C)]
pub struct CfeMsgTelemetrySecondaryHeader {
    /// Time, big endian: 4 byte seconds, 2 byte subseconds.
    time: [u8; 6],
}

/// `CFE_MSG_TelemetryHeader_t` of `cfe_msg_api_typedefs.h` / `default_cfe_msg_hdr_pri.h`.
#[repr(C)]
pub struct CfeMsgTelemetryHeader {
    /// Base message (`CFE_MSG_Message_t`).
    msg: CfeMsgMessage,
    /// Telemetry secondary header (`CFE_MSG_TelemetrySecondaryHeader_t`).
    sec: CfeMsgTelemetrySecondaryHeader,
    /// Pad to avoid compiler padding if payload requires 64-bit alignment.
    spare: [u8; 4],
}

const _: () = {
    use core::mem::{align_of, size_of};
    assert!(size_of::<CfeMsgMessage>() == PRIMARY_HEADER_LEN && align_of::<CfeMsgMessage>() == 1);
    assert!(
        size_of::<CfeMsgCommandSecondaryHeader>() == 2
            && align_of::<CfeMsgCommandSecondaryHeader>() == 1
    );
    assert!(
        size_of::<CfeMsgCommandHeader>() == COMMAND_HEADER_LEN
            && align_of::<CfeMsgCommandHeader>() == 1
    );
    assert!(size_of::<CfeTimeSysTime>() == 8 && align_of::<CfeTimeSysTime>() == 4);
    assert!(size_of::<CfeSbMsgId>() == 4 && align_of::<CfeSbMsgId>() == 4);
    assert!(
        size_of::<CfeMsgTelemetrySecondaryHeader>() == 6
            && align_of::<CfeMsgTelemetrySecondaryHeader>() == 1
    );
    assert!(
        size_of::<CfeMsgTelemetryHeader>() == TELEMETRY_HEADER_LEN
            && align_of::<CfeMsgTelemetryHeader>() == 1
    );
};

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
    status_from_result(unsafe { cfe_msg::set_size(&mut (*msg_ptr).pri, size) })
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
    status_from_result(unsafe { cfe_msg::set_header_version(&mut (*msg_ptr).pri, version) })
}

/// `CFE_MSG_GetApId` of `cfe_msg.h`.
///
/// # Safety
/// If non-null, `msg_ptr` must point to a valid `CFE_MSG_Message_t` and `ap_id` must point to
/// writable `CFE_MSG_ApId_t` storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn CFE_MSG_GetApId(
    msg_ptr: *const CfeMsgMessage,
    ap_id: *mut CfeMsgApId,
) -> CfeStatus {
    if msg_ptr.is_null() || ap_id.is_null() {
        return CFE_MSG_BAD_ARGUMENT;
    }
    // SAFETY: `msg_ptr` and `ap_id` are non-null, and the caller guarantees they point to valid
    // `CFE_MSG_Message_t` and `CFE_MSG_ApId_t` objects for the duration of this call.
    unsafe {
        *ap_id = cfe_msg::get_apid(&(*msg_ptr).pri);
    }
    CFE_SUCCESS
}

/// `CFE_MSG_SetApId` of `cfe_msg.h`.
///
/// # Safety
/// If non-null, `msg_ptr` must point to a valid mutable `CFE_MSG_Message_t`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn CFE_MSG_SetApId(
    msg_ptr: *mut CfeMsgMessage,
    ap_id: CfeMsgApId,
) -> CfeStatus {
    if msg_ptr.is_null() {
        return CFE_MSG_BAD_ARGUMENT;
    }
    // SAFETY: `msg_ptr` is non-null, and the caller guarantees it points to a valid mutable
    // `CFE_MSG_Message_t` for the duration of this call.
    status_from_result(unsafe { cfe_msg::set_apid(&mut (*msg_ptr).pri, ap_id) })
}

/// `CFE_MSG_GetSegmentationFlag` of `cfe_msg.h`.
///
/// # Safety
/// If non-null, `msg_ptr` must point to a valid `CFE_MSG_Message_t` and `seg_flag` must point to
/// writable `CFE_MSG_SegmentationFlag_t` storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn CFE_MSG_GetSegmentationFlag(
    msg_ptr: *const CfeMsgMessage,
    seg_flag: *mut CfeMsgSegmentationFlag,
) -> CfeStatus {
    if msg_ptr.is_null() || seg_flag.is_null() {
        return CFE_MSG_BAD_ARGUMENT;
    }
    // SAFETY: `msg_ptr` and `seg_flag` are non-null, and the caller guarantees they point to valid
    // `CFE_MSG_Message_t` and `CFE_MSG_SegmentationFlag_t` objects for the duration of this call.
    unsafe {
        *seg_flag = match cfe_msg::get_segmentation_flag(&(*msg_ptr).pri) {
            SequenceFlags::Continuation => CFE_MSG_SEG_FLAG_CONTINUE,
            SequenceFlags::First => CFE_MSG_SEG_FLAG_FIRST,
            SequenceFlags::Last => CFE_MSG_SEG_FLAG_LAST,
            SequenceFlags::Unsegmented => CFE_MSG_SEG_FLAG_UNSEGMENTED,
        };
    }
    CFE_SUCCESS
}

/// `CFE_MSG_SetSegmentationFlag` of `cfe_msg.h`.
///
/// # Safety
/// If non-null, `msg_ptr` must point to a valid mutable `CFE_MSG_Message_t`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn CFE_MSG_SetSegmentationFlag(
    msg_ptr: *mut CfeMsgMessage,
    seg_flag: CfeMsgSegmentationFlag,
) -> CfeStatus {
    if msg_ptr.is_null() {
        return CFE_MSG_BAD_ARGUMENT;
    }
    let flag = match seg_flag {
        CFE_MSG_SEG_FLAG_CONTINUE => SequenceFlags::Continuation,
        CFE_MSG_SEG_FLAG_FIRST => SequenceFlags::First,
        CFE_MSG_SEG_FLAG_LAST => SequenceFlags::Last,
        CFE_MSG_SEG_FLAG_UNSEGMENTED => SequenceFlags::Unsegmented,
        _ => return CFE_MSG_BAD_ARGUMENT,
    };
    // SAFETY: `msg_ptr` is non-null, and the caller guarantees it points to a valid mutable
    // `CFE_MSG_Message_t` for the duration of this call.
    unsafe {
        cfe_msg::set_segmentation_flag(&mut (*msg_ptr).pri, flag);
    }
    CFE_SUCCESS
}

/// `CFE_MSG_GetSequenceCount` of `cfe_msg.h`.
///
/// # Safety
/// If non-null, `msg_ptr` must point to a valid `CFE_MSG_Message_t` and `seq_cnt` must point to
/// writable `CFE_MSG_SequenceCount_t` storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn CFE_MSG_GetSequenceCount(
    msg_ptr: *const CfeMsgMessage,
    seq_cnt: *mut CfeMsgSequenceCount,
) -> CfeStatus {
    if msg_ptr.is_null() || seq_cnt.is_null() {
        return CFE_MSG_BAD_ARGUMENT;
    }
    // SAFETY: `msg_ptr` and `seq_cnt` are non-null, and the caller guarantees they point to valid
    // `CFE_MSG_Message_t` and `CFE_MSG_SequenceCount_t` objects for the duration of this call.
    unsafe {
        *seq_cnt = cfe_msg::get_sequence_count(&(*msg_ptr).pri);
    }
    CFE_SUCCESS
}

/// `CFE_MSG_SetSequenceCount` of `cfe_msg.h`.
///
/// # Safety
/// If non-null, `msg_ptr` must point to a valid mutable `CFE_MSG_Message_t`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn CFE_MSG_SetSequenceCount(
    msg_ptr: *mut CfeMsgMessage,
    seq_cnt: CfeMsgSequenceCount,
) -> CfeStatus {
    if msg_ptr.is_null() {
        return CFE_MSG_BAD_ARGUMENT;
    }
    // SAFETY: `msg_ptr` is non-null, and the caller guarantees it points to a valid mutable
    // `CFE_MSG_Message_t` for the duration of this call.
    status_from_result(unsafe { cfe_msg::set_sequence_count(&mut (*msg_ptr).pri, seq_cnt) })
}

/// `CFE_MSG_GetFcnCode` of `cfe_msg.h`.
///
/// # Safety
/// If non-null, `msg_ptr` must point to a valid `CFE_MSG_Message_t` (and, when it is a
/// CCSDS v1 command packet with a secondary header, a valid `CFE_MSG_CommandHeader_t`), and
/// `fcn_code` must point to writable `CFE_MSG_FcnCode_t` storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn CFE_MSG_GetFcnCode(
    msg_ptr: *const CfeMsgMessage,
    fcn_code: *mut CfeMsgFcnCode,
) -> CfeStatus {
    if msg_ptr.is_null() || fcn_code.is_null() {
        return CFE_MSG_BAD_ARGUMENT;
    }
    // SAFETY: `msg_ptr` is non-null, and the caller guarantees it points to a valid
    // `CFE_MSG_Message_t` for the duration of this call.
    let pri = unsafe { &(*msg_ptr).pri };
    if !cfe_msg::is_command_with_secondary_header(pri) {
        // SAFETY: `fcn_code` is non-null, and the caller guarantees it points to writable
        // `CFE_MSG_FcnCode_t` storage.
        unsafe {
            *fcn_code = 0;
        }
        return CFE_MSG_WRONG_MSG_TYPE;
    }
    // SAFETY: `msg_ptr` points to a valid command message with a secondary header, so it is a
    // `CFE_MSG_CommandHeader_t` (`CfeMsgCommandHeader`, align 1, size 8). `fcn_code` is non-null
    // and writable.
    unsafe {
        let cmd = &*msg_ptr.cast::<CfeMsgCommandHeader>();
        let [b0, b1, b2, b3, b4, b5] = cmd.msg.pri;
        let hdr = [
            b0,
            b1,
            b2,
            b3,
            b4,
            b5,
            cmd.sec.function_code,
            cmd.sec.checksum,
        ];
        match cfe_msg::get_fcn_code(&hdr) {
            Ok(code) => {
                *fcn_code = code;
                CFE_SUCCESS
            }
            Err(MsgError::WrongMsgType) => {
                *fcn_code = 0;
                CFE_MSG_WRONG_MSG_TYPE
            }
            Err(MsgError::BadArgument) => CFE_MSG_BAD_ARGUMENT,
        }
    }
}

/// `CFE_MSG_SetFcnCode` of `cfe_msg.h`.
///
/// # Safety
/// If non-null, `msg_ptr` must point to a valid mutable `CFE_MSG_Message_t` (and, when it is a
/// CCSDS v1 command packet with a secondary header, a valid mutable `CFE_MSG_CommandHeader_t`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn CFE_MSG_SetFcnCode(
    msg_ptr: *mut CfeMsgMessage,
    fcn_code: CfeMsgFcnCode,
) -> CfeStatus {
    if msg_ptr.is_null() || fcn_code > cfe_msg::MAX_FCN_CODE {
        return CFE_MSG_BAD_ARGUMENT;
    }
    // SAFETY: `msg_ptr` is non-null, and the caller guarantees it points to a valid mutable
    // `CFE_MSG_Message_t` for the duration of this call.
    let pri = unsafe { &(*msg_ptr).pri };
    if !cfe_msg::is_command_with_secondary_header(pri) {
        return CFE_MSG_WRONG_MSG_TYPE;
    }
    // SAFETY: `msg_ptr` points to a valid mutable command message with a secondary header, so it
    // is a `CFE_MSG_CommandHeader_t` (`CfeMsgCommandHeader`, align 1, size 8).
    unsafe {
        let cmd = &mut *msg_ptr.cast::<CfeMsgCommandHeader>();
        let [b0, b1, b2, b3, b4, b5] = cmd.msg.pri;
        let mut hdr = [
            b0,
            b1,
            b2,
            b3,
            b4,
            b5,
            cmd.sec.function_code,
            cmd.sec.checksum,
        ];
        match cfe_msg::set_fcn_code(&mut hdr, fcn_code) {
            Ok(()) => {
                let [.., fc, _] = hdr;
                cmd.sec.function_code = fc;
                CFE_SUCCESS
            }
            Err(MsgError::BadArgument) => CFE_MSG_BAD_ARGUMENT,
            Err(MsgError::WrongMsgType) => CFE_MSG_WRONG_MSG_TYPE,
        }
    }
}

/// `CFE_MSG_GenerateChecksum` of `cfe_msg.h`.
///
/// # Safety
/// If non-null, `msg_ptr` must point to a valid mutable `CFE_MSG_Message_t` (and, when it is a
/// CCSDS v1 command packet with a secondary header, a valid mutable `CFE_MSG_CommandHeader_t`
/// whose backing buffer is at least
/// `max(CFE_MSG_GetSize(msg_ptr), sizeof(CFE_MSG_CommandHeader_t))` bytes).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn CFE_MSG_GenerateChecksum(msg_ptr: *mut CfeMsgMessage) -> CfeStatus {
    if msg_ptr.is_null() {
        return CFE_MSG_BAD_ARGUMENT;
    }
    // SAFETY: `msg_ptr` is non-null, and the caller guarantees it points to a valid mutable
    // `CFE_MSG_Message_t` for the duration of this call.
    let pri = unsafe { &(*msg_ptr).pri };
    if !cfe_msg::is_command_with_secondary_header(pri) {
        return CFE_MSG_WRONG_MSG_TYPE;
    }
    let len = cfe_msg::get_size(pri).max(cfe_msg::COMMAND_HEADER_LEN);
    // SAFETY: `msg_ptr` points to a valid mutable command message with a secondary header, so
    // its backing buffer is at least `max(size, COMMAND_HEADER_LEN)` bytes (at most 65542 <=
    // `isize::MAX`) and `u8` has alignment 1.
    let pkt = unsafe { core::slice::from_raw_parts_mut(msg_ptr.cast::<u8>(), len) };
    match cfe_msg::generate_checksum(pkt) {
        Ok(()) => CFE_SUCCESS,
        Err(MsgError::BadArgument) => CFE_MSG_BAD_ARGUMENT,
        Err(MsgError::WrongMsgType) => CFE_MSG_WRONG_MSG_TYPE,
    }
}

/// `CFE_MSG_ValidateChecksum` of `cfe_msg.h`.
///
/// # Safety
/// If non-null, `msg_ptr` must point to a valid `CFE_MSG_Message_t` (and, when it is a
/// CCSDS v1 command packet with a secondary header, a buffer of at least `CFE_MSG_GetSize(msg_ptr)`
/// bytes), and `is_valid` must point to writable `bool` storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn CFE_MSG_ValidateChecksum(
    msg_ptr: *const CfeMsgMessage,
    is_valid: *mut bool,
) -> CfeStatus {
    if msg_ptr.is_null() || is_valid.is_null() {
        return CFE_MSG_BAD_ARGUMENT;
    }
    // SAFETY: `msg_ptr` is non-null, and the caller guarantees it points to a valid
    // `CFE_MSG_Message_t` for the duration of this call.
    let pri = unsafe { &(*msg_ptr).pri };
    if !cfe_msg::is_command_with_secondary_header(pri) {
        return CFE_MSG_WRONG_MSG_TYPE;
    }
    let size = cfe_msg::get_size(pri);
    // SAFETY: `msg_ptr` points to a valid command message with a secondary header, so its
    // backing buffer is at least `size` bytes (at most 65542 <= `isize::MAX`) and `u8` has
    // alignment 1.
    let pkt = unsafe { core::slice::from_raw_parts(msg_ptr.cast::<u8>(), size) };
    match cfe_msg::validate_checksum(pkt) {
        Ok(valid) => {
            // SAFETY: `is_valid` is non-null, and the caller guarantees it points to writable
            // `bool` storage.
            unsafe {
                *is_valid = valid;
            }
            CFE_SUCCESS
        }
        Err(MsgError::BadArgument) => CFE_MSG_BAD_ARGUMENT,
        Err(MsgError::WrongMsgType) => CFE_MSG_WRONG_MSG_TYPE,
    }
}

/// `CFE_MSG_GetMsgTime` of `cfe_msg.h`.
///
/// # Safety
/// If non-null, `msg_ptr` must point to a valid `CFE_MSG_Message_t` (and, when it is a
/// CCSDS v1 telemetry packet with a secondary header, a valid `CFE_MSG_TelemetryHeader_t`), and
/// `time` must point to writable `CFE_TIME_SysTime_t` storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn CFE_MSG_GetMsgTime(
    msg_ptr: *const CfeMsgMessage,
    time: *mut CfeTimeSysTime,
) -> CfeStatus {
    if msg_ptr.is_null() || time.is_null() {
        return CFE_MSG_BAD_ARGUMENT;
    }
    // SAFETY: `msg_ptr` is non-null, and the caller guarantees it points to a valid
    // `CFE_MSG_Message_t` for the duration of this call.
    let pri = unsafe { &(*msg_ptr).pri };
    if !cfe_msg::is_telemetry_with_secondary_header(pri) {
        // SAFETY: `time` is non-null, and the caller guarantees it points to writable
        // `CFE_TIME_SysTime_t` storage.
        unsafe {
            *time = CfeTimeSysTime {
                seconds: 0,
                subseconds: 0,
            };
        }
        return CFE_MSG_WRONG_MSG_TYPE;
    }
    // SAFETY: `msg_ptr` points to a valid telemetry message with a secondary header, so it is a
    // `CFE_MSG_TelemetryHeader_t` (`CfeMsgTelemetryHeader`, align 1, size 16). `time` is non-null
    // and writable.
    unsafe {
        let tlm = &*msg_ptr.cast::<CfeMsgTelemetryHeader>();
        let [b0, b1, b2, b3, b4, b5] = tlm.msg.pri;
        let [t0, t1, t2, t3, t4, t5] = tlm.sec.time;
        let [sp0, sp1, sp2, sp3] = tlm.spare;
        let hdr = [
            b0, b1, b2, b3, b4, b5, t0, t1, t2, t3, t4, t5, sp0, sp1, sp2, sp3,
        ];
        match cfe_msg::get_msg_time(&hdr) {
            Ok(t) => {
                *time = CfeTimeSysTime {
                    seconds: t.seconds,
                    subseconds: t.subseconds,
                };
                CFE_SUCCESS
            }
            Err(MsgError::WrongMsgType) => {
                *time = CfeTimeSysTime {
                    seconds: 0,
                    subseconds: 0,
                };
                CFE_MSG_WRONG_MSG_TYPE
            }
            Err(MsgError::BadArgument) => CFE_MSG_BAD_ARGUMENT,
        }
    }
}

/// `CFE_MSG_SetMsgTime` of `cfe_msg.h`.
///
/// # Safety
/// If non-null, `msg_ptr` must point to a valid mutable `CFE_MSG_Message_t` (and, when it is a
/// CCSDS v1 telemetry packet with a secondary header, a valid mutable
/// `CFE_MSG_TelemetryHeader_t`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn CFE_MSG_SetMsgTime(
    msg_ptr: *mut CfeMsgMessage,
    new_time: CfeTimeSysTime,
) -> CfeStatus {
    if msg_ptr.is_null() {
        return CFE_MSG_BAD_ARGUMENT;
    }
    // SAFETY: `msg_ptr` is non-null, and the caller guarantees it points to a valid mutable
    // `CFE_MSG_Message_t` for the duration of this call.
    let pri = unsafe { &(*msg_ptr).pri };
    if !cfe_msg::is_telemetry_with_secondary_header(pri) {
        return CFE_MSG_WRONG_MSG_TYPE;
    }
    // SAFETY: `msg_ptr` points to a valid mutable telemetry message with a secondary header, so
    // it is a `CFE_MSG_TelemetryHeader_t` (`CfeMsgTelemetryHeader`, align 1, size 16).
    unsafe {
        let tlm = &mut *msg_ptr.cast::<CfeMsgTelemetryHeader>();
        let [b0, b1, b2, b3, b4, b5] = tlm.msg.pri;
        let [t0, t1, t2, t3, t4, t5] = tlm.sec.time;
        let [sp0, sp1, sp2, sp3] = tlm.spare;
        let mut hdr = [
            b0, b1, b2, b3, b4, b5, t0, t1, t2, t3, t4, t5, sp0, sp1, sp2, sp3,
        ];
        let sys_time = SysTime {
            seconds: new_time.seconds,
            subseconds: new_time.subseconds,
        };
        match cfe_msg::set_msg_time(&mut hdr, sys_time) {
            Ok(()) => {
                let [_, _, _, _, _, _, r0, r1, r2, r3, r4, r5, ..] = hdr;
                tlm.sec.time = [r0, r1, r2, r3, r4, r5];
                CFE_SUCCESS
            }
            Err(MsgError::BadArgument) => CFE_MSG_BAD_ARGUMENT,
            Err(MsgError::WrongMsgType) => CFE_MSG_WRONG_MSG_TYPE,
        }
    }
}

unsafe extern "C" {
    /// `CFE_SB_IsValidMsgId` of `cfe_sb.h`.
    safe fn CFE_SB_IsValidMsgId(msg_id: CfeSbMsgId) -> bool;
}

/// `CFE_MSG_GetMsgId` of `cfe_msg.h`.
///
/// # Safety
/// If non-null, `msg_ptr` must point to a valid `CFE_MSG_Message_t` and `msg_id` must point to
/// writable `CFE_SB_MsgId_t` storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn CFE_MSG_GetMsgId(
    msg_ptr: *const CfeMsgMessage,
    msg_id: *mut CfeSbMsgId,
) -> CfeStatus {
    if msg_ptr.is_null() || msg_id.is_null() {
        return CFE_MSG_BAD_ARGUMENT;
    }
    // SAFETY: `msg_ptr` and `msg_id` are non-null, and the caller guarantees they point to valid
    // `CFE_MSG_Message_t` and `CFE_SB_MsgId_t` objects for the duration of this call.
    unsafe {
        *msg_id = CfeSbMsgId {
            value: u32::from(cfe_msg::get_msg_id(&(*msg_ptr).pri).get()),
        };
    }
    CFE_SUCCESS
}

/// `CFE_MSG_SetMsgId` of `cfe_msg.h`.
///
/// # Safety
/// If non-null, `msg_ptr` must point to a valid mutable `CFE_MSG_Message_t`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn CFE_MSG_SetMsgId(
    msg_ptr: *mut CfeMsgMessage,
    msg_id: CfeSbMsgId,
) -> CfeStatus {
    if msg_ptr.is_null() || !CFE_SB_IsValidMsgId(msg_id) {
        return CFE_MSG_BAD_ARGUMENT;
    }
    let id = msg_id.to_msg_id();
    // SAFETY: `msg_ptr` is non-null, and the caller guarantees it points to a valid mutable
    // `CFE_MSG_Message_t` for the duration of this call.
    unsafe {
        cfe_msg::set_msg_id(&mut (*msg_ptr).pri, id);
    }
    CFE_SUCCESS
}

/// `CFE_MSG_GetTypeFromMsgId` of `cfe_msg.h`.
///
/// # Safety
/// If non-null, `msg_type` must point to writable `CFE_MSG_Type_t` storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn CFE_MSG_GetTypeFromMsgId(
    msg_id: CfeSbMsgId,
    msg_type: *mut CfeMsgType,
) -> CfeStatus {
    if msg_type.is_null() || !CFE_SB_IsValidMsgId(msg_id) {
        return CFE_MSG_BAD_ARGUMENT;
    }
    let id = msg_id.to_msg_id();
    let ty = match cfe_msg::get_type_from_msg_id(id) {
        PacketType::Telecommand => CFE_MSG_TYPE_CMD,
        PacketType::Telemetry => CFE_MSG_TYPE_TLM,
    };
    // SAFETY: `msg_type` is non-null, and the caller guarantees it points to writable
    // `CFE_MSG_Type_t` storage.
    unsafe {
        *msg_type = ty;
    }
    CFE_SUCCESS
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

#[cfg(test)]
mod tests {
    use super::*;
    use core::ptr;

    #[unsafe(no_mangle)]
    extern "C" fn CFE_SB_IsValidMsgId(msg_id: CfeSbMsgId) -> bool {
        msg_id.value != 0 && msg_id.value <= 0x1FFF
    }

    #[test]
    fn primary_header_null_pointers_return_bad_argument() {
        let mut size: CfeMsgSize = 0;
        let mut msg_type: CfeMsgType = 0;
        let mut has_sec = false;
        let mut version: CfeMsgHeaderVersion = 0;
        let mut apid: CfeMsgApId = 0;
        let mut seg_flag: CfeMsgSegmentationFlag = 0;
        let mut seq_cnt: CfeMsgSequenceCount = 0;
        let msg = CfeMsgMessage {
            pri: [0; PRIMARY_HEADER_LEN],
        };

        // SAFETY: Every call below passes null pointers specifically to verify null-guard paths.
        unsafe {
            assert_eq!(
                CFE_MSG_GetSize(ptr::null(), &raw mut size),
                CFE_MSG_BAD_ARGUMENT
            );
            assert_eq!(
                CFE_MSG_GetSize(&raw const msg, ptr::null_mut()),
                CFE_MSG_BAD_ARGUMENT
            );
            assert_eq!(CFE_MSG_SetSize(ptr::null_mut(), 8), CFE_MSG_BAD_ARGUMENT);
            assert_eq!(
                CFE_MSG_GetType(ptr::null(), &raw mut msg_type),
                CFE_MSG_BAD_ARGUMENT
            );
            assert_eq!(
                CFE_MSG_GetType(&raw const msg, ptr::null_mut()),
                CFE_MSG_BAD_ARGUMENT
            );
            assert_eq!(
                CFE_MSG_SetType(ptr::null_mut(), CFE_MSG_TYPE_CMD),
                CFE_MSG_BAD_ARGUMENT
            );
            assert_eq!(
                CFE_MSG_GetHasSecondaryHeader(ptr::null(), &raw mut has_sec),
                CFE_MSG_BAD_ARGUMENT
            );
            assert_eq!(
                CFE_MSG_GetHasSecondaryHeader(&raw const msg, ptr::null_mut()),
                CFE_MSG_BAD_ARGUMENT
            );
            assert_eq!(
                CFE_MSG_SetHasSecondaryHeader(ptr::null_mut(), true),
                CFE_MSG_BAD_ARGUMENT
            );
            assert_eq!(
                CFE_MSG_GetHeaderVersion(ptr::null(), &raw mut version),
                CFE_MSG_BAD_ARGUMENT
            );
            assert_eq!(
                CFE_MSG_GetHeaderVersion(&raw const msg, ptr::null_mut()),
                CFE_MSG_BAD_ARGUMENT
            );
            assert_eq!(
                CFE_MSG_SetHeaderVersion(ptr::null_mut(), 0),
                CFE_MSG_BAD_ARGUMENT
            );
            assert_eq!(
                CFE_MSG_GetApId(ptr::null(), &raw mut apid),
                CFE_MSG_BAD_ARGUMENT
            );
            assert_eq!(
                CFE_MSG_GetApId(&raw const msg, ptr::null_mut()),
                CFE_MSG_BAD_ARGUMENT
            );
            assert_eq!(CFE_MSG_SetApId(ptr::null_mut(), 1), CFE_MSG_BAD_ARGUMENT);
            assert_eq!(
                CFE_MSG_GetSegmentationFlag(ptr::null(), &raw mut seg_flag),
                CFE_MSG_BAD_ARGUMENT
            );
            assert_eq!(
                CFE_MSG_GetSegmentationFlag(&raw const msg, ptr::null_mut()),
                CFE_MSG_BAD_ARGUMENT
            );
            assert_eq!(
                CFE_MSG_SetSegmentationFlag(ptr::null_mut(), CFE_MSG_SEG_FLAG_UNSEGMENTED),
                CFE_MSG_BAD_ARGUMENT
            );
            assert_eq!(
                CFE_MSG_GetSequenceCount(ptr::null(), &raw mut seq_cnt),
                CFE_MSG_BAD_ARGUMENT
            );
            assert_eq!(
                CFE_MSG_GetSequenceCount(&raw const msg, ptr::null_mut()),
                CFE_MSG_BAD_ARGUMENT
            );
            assert_eq!(
                CFE_MSG_SetSequenceCount(ptr::null_mut(), 0),
                CFE_MSG_BAD_ARGUMENT
            );
        }
    }

    #[test]
    fn secondary_header_and_msgid_null_pointers_return_bad_argument() {
        let mut msg_type: CfeMsgType = 0;
        let mut fcn: CfeMsgFcnCode = 0;
        let mut valid = false;
        let mut time = CfeTimeSysTime {
            seconds: 0,
            subseconds: 0,
        };
        let mut mid = CfeSbMsgId { value: 0 };
        let mut msg = CfeMsgMessage {
            pri: [0; PRIMARY_HEADER_LEN],
        };

        // SAFETY: Every call below passes null pointers specifically to verify null-guard paths.
        unsafe {
            assert_eq!(
                CFE_MSG_GetFcnCode(ptr::null(), &raw mut fcn),
                CFE_MSG_BAD_ARGUMENT
            );
            assert_eq!(
                CFE_MSG_GetFcnCode(&raw const msg, ptr::null_mut()),
                CFE_MSG_BAD_ARGUMENT
            );
            assert_eq!(CFE_MSG_SetFcnCode(ptr::null_mut(), 0), CFE_MSG_BAD_ARGUMENT);
            assert_eq!(
                CFE_MSG_GenerateChecksum(ptr::null_mut()),
                CFE_MSG_BAD_ARGUMENT
            );
            assert_eq!(
                CFE_MSG_ValidateChecksum(ptr::null(), &raw mut valid),
                CFE_MSG_BAD_ARGUMENT
            );
            assert_eq!(
                CFE_MSG_ValidateChecksum(&raw const msg, ptr::null_mut()),
                CFE_MSG_BAD_ARGUMENT
            );
            assert_eq!(
                CFE_MSG_GetMsgTime(ptr::null(), &raw mut time),
                CFE_MSG_BAD_ARGUMENT
            );
            assert_eq!(
                CFE_MSG_GetMsgTime(&raw const msg, ptr::null_mut()),
                CFE_MSG_BAD_ARGUMENT
            );
            assert_eq!(
                CFE_MSG_SetMsgTime(ptr::null_mut(), time),
                CFE_MSG_BAD_ARGUMENT
            );
            assert_eq!(
                CFE_MSG_GetMsgId(ptr::null(), &raw mut mid),
                CFE_MSG_BAD_ARGUMENT
            );
            assert_eq!(
                CFE_MSG_GetMsgId(&raw const msg, ptr::null_mut()),
                CFE_MSG_BAD_ARGUMENT
            );
            assert_eq!(
                CFE_MSG_SetMsgId(ptr::null_mut(), CfeSbMsgId { value: 0x1801 }),
                CFE_MSG_BAD_ARGUMENT
            );
            assert_eq!(
                CFE_MSG_SetMsgId(&raw mut msg, CfeSbMsgId { value: 0 }),
                CFE_MSG_BAD_ARGUMENT
            );
            assert_eq!(
                CFE_MSG_GetTypeFromMsgId(CfeSbMsgId { value: 0x1801 }, ptr::null_mut()),
                CFE_MSG_BAD_ARGUMENT
            );
            assert_eq!(
                CFE_MSG_GetTypeFromMsgId(CfeSbMsgId { value: 0 }, &raw mut msg_type),
                CFE_MSG_BAD_ARGUMENT
            );
        }
    }

    #[test]
    fn primary_header_and_msgid_round_trip() {
        assert_eq!(CFE_MSG_GetNextSequenceCount(0x3FFF), 0);
        assert_eq!(CFE_MSG_GetNextSequenceCount(41), 42);

        let mut msg = CfeMsgMessage {
            pri: [0; PRIMARY_HEADER_LEN],
        };
        let mut size: CfeMsgSize = 0;
        let mut msg_type: CfeMsgType = 0;
        let mut has_sec = false;
        let mut version: CfeMsgHeaderVersion = 0;
        let mut apid: CfeMsgApId = 0;
        let mut seg_flag: CfeMsgSegmentationFlag = 0;
        let mut seq_cnt: CfeMsgSequenceCount = 0;
        let mut mid = CfeSbMsgId { value: 0 };

        // SAFETY: All pointers reference valid local variables for the duration of each call.
        unsafe {
            assert_eq!(CFE_MSG_SetSize(&raw mut msg, 16), CFE_SUCCESS);
            assert_eq!(CFE_MSG_GetSize(&raw const msg, &raw mut size), CFE_SUCCESS);
            assert_eq!(size, 16);

            assert_eq!(CFE_MSG_SetType(&raw mut msg, CFE_MSG_TYPE_CMD), CFE_SUCCESS);
            assert_eq!(
                CFE_MSG_GetType(&raw const msg, &raw mut msg_type),
                CFE_SUCCESS
            );
            assert_eq!(msg_type, CFE_MSG_TYPE_CMD);
            assert_eq!(CFE_MSG_SetType(&raw mut msg, 99), CFE_MSG_BAD_ARGUMENT);

            assert_eq!(
                CFE_MSG_SetHasSecondaryHeader(&raw mut msg, true),
                CFE_SUCCESS
            );
            assert_eq!(
                CFE_MSG_GetHasSecondaryHeader(&raw const msg, &raw mut has_sec),
                CFE_SUCCESS
            );
            assert!(has_sec);

            assert_eq!(CFE_MSG_SetHeaderVersion(&raw mut msg, 0), CFE_SUCCESS);
            assert_eq!(
                CFE_MSG_GetHeaderVersion(&raw const msg, &raw mut version),
                CFE_SUCCESS
            );
            assert_eq!(version, 0);

            assert_eq!(CFE_MSG_SetApId(&raw mut msg, 0x0123), CFE_SUCCESS);
            assert_eq!(CFE_MSG_GetApId(&raw const msg, &raw mut apid), CFE_SUCCESS);
            assert_eq!(apid, 0x0123);

            assert_eq!(
                CFE_MSG_SetSegmentationFlag(&raw mut msg, CFE_MSG_SEG_FLAG_UNSEGMENTED),
                CFE_SUCCESS
            );
            assert_eq!(
                CFE_MSG_GetSegmentationFlag(&raw const msg, &raw mut seg_flag),
                CFE_SUCCESS
            );
            assert_eq!(seg_flag, CFE_MSG_SEG_FLAG_UNSEGMENTED);
            assert_eq!(
                CFE_MSG_SetSegmentationFlag(&raw mut msg, 99),
                CFE_MSG_BAD_ARGUMENT
            );

            assert_eq!(CFE_MSG_SetSequenceCount(&raw mut msg, 0x1234), CFE_SUCCESS);
            assert_eq!(
                CFE_MSG_GetSequenceCount(&raw const msg, &raw mut seq_cnt),
                CFE_SUCCESS
            );
            assert_eq!(seq_cnt, 0x1234);

            assert_eq!(
                CFE_MSG_SetMsgId(&raw mut msg, CfeSbMsgId { value: 0x1888 }),
                CFE_SUCCESS
            );
            assert_eq!(CFE_MSG_GetMsgId(&raw const msg, &raw mut mid), CFE_SUCCESS);
            assert_eq!(mid.value, 0x1888);

            assert_eq!(
                CFE_MSG_GetTypeFromMsgId(CfeSbMsgId { value: 0x1888 }, &raw mut msg_type),
                CFE_SUCCESS
            );
            assert_eq!(msg_type, CFE_MSG_TYPE_CMD);
            assert_eq!(
                CFE_MSG_GetTypeFromMsgId(CfeSbMsgId { value: 0x0888 }, &raw mut msg_type),
                CFE_SUCCESS
            );
            assert_eq!(msg_type, CFE_MSG_TYPE_TLM);
        }
    }

    #[test]
    fn command_and_telemetry_secondary_headers_and_checksum() {
        #[repr(C)]
        struct CmdPacket {
            hdr: CfeMsgCommandHeader,
            payload: [u8; 4],
        }

        let mut cmd_pkt = CmdPacket {
            hdr: CfeMsgCommandHeader {
                msg: CfeMsgMessage {
                    pri: [0x18, 0x01, 0xC0, 0x00, 0x00, 0x05], // size = 12
                },
                sec: CfeMsgCommandSecondaryHeader {
                    function_code: 0,
                    checksum: 0,
                },
            },
            payload: [0xAA, 0x55, 0x12, 0x34],
        };
        let cmd_ptr = core::ptr::from_mut(&mut cmd_pkt).cast::<CfeMsgMessage>();
        let mut fcn: CfeMsgFcnCode = 99;
        let mut valid = false;

        // SAFETY: `cmd_ptr` points to a valid 12-byte `CmdPacket` starting with `CfeMsgCommandHeader`.
        unsafe {
            assert_eq!(CFE_MSG_SetFcnCode(cmd_ptr, 7), CFE_SUCCESS);
            assert_eq!(CFE_MSG_GetFcnCode(cmd_ptr, &raw mut fcn), CFE_SUCCESS);
            assert_eq!(fcn, 7);

            assert_eq!(CFE_MSG_GenerateChecksum(cmd_ptr), CFE_SUCCESS);
            assert_eq!(
                CFE_MSG_ValidateChecksum(cmd_ptr, &raw mut valid),
                CFE_SUCCESS
            );
            assert!(valid);

            // Telemetry time fails with WRONG_MSG_TYPE and zeroes `time` out-param.
            let mut time = CfeTimeSysTime {
                seconds: 99,
                subseconds: 88,
            };
            assert_eq!(
                CFE_MSG_GetMsgTime(cmd_ptr, &raw mut time),
                CFE_MSG_WRONG_MSG_TYPE
            );
            assert_eq!(time.seconds, 0);
            assert_eq!(time.subseconds, 0);
            assert_eq!(CFE_MSG_SetMsgTime(cmd_ptr, time), CFE_MSG_WRONG_MSG_TYPE);
        }

        let mut tlm_hdr = CfeMsgTelemetryHeader {
            msg: CfeMsgMessage {
                pri: [0x08, 0x01, 0xC0, 0x00, 0x00, 0x09], // size = 16
            },
            sec: CfeMsgTelemetrySecondaryHeader { time: [0; 6] },
            spare: [0; 4],
        };
        let tlm_ptr = core::ptr::from_mut(&mut tlm_hdr).cast::<CfeMsgMessage>();
        let set_time = CfeTimeSysTime {
            seconds: 0x1122_3344,
            subseconds: 0x5566_0000,
        };
        let mut got_time = CfeTimeSysTime {
            seconds: 0,
            subseconds: 0,
        };

        // SAFETY: `tlm_ptr` points to a valid 16-byte `CfeMsgTelemetryHeader`.
        unsafe {
            assert_eq!(CFE_MSG_SetMsgTime(tlm_ptr, set_time), CFE_SUCCESS);
            assert_eq!(CFE_MSG_GetMsgTime(tlm_ptr, &raw mut got_time), CFE_SUCCESS);
            assert_eq!(got_time.seconds, set_time.seconds);
            assert_eq!(got_time.subseconds, set_time.subseconds);

            // Command functions fail with WRONG_MSG_TYPE and `GetFcnCode` zeroes `fcn`.
            assert_eq!(
                CFE_MSG_GetFcnCode(tlm_ptr, &raw mut fcn),
                CFE_MSG_WRONG_MSG_TYPE
            );
            assert_eq!(fcn, 0);
            assert_eq!(CFE_MSG_SetFcnCode(tlm_ptr, 1), CFE_MSG_WRONG_MSG_TYPE);
            assert_eq!(CFE_MSG_GenerateChecksum(tlm_ptr), CFE_MSG_WRONG_MSG_TYPE);
            assert_eq!(
                CFE_MSG_ValidateChecksum(tlm_ptr, &raw mut valid),
                CFE_MSG_WRONG_MSG_TYPE
            );
        }
    }

    #[test]
    fn checksum_edge_cases_and_bare_msg_wrong_type() {
        #[repr(C)]
        struct CmdPacket {
            hdr: CfeMsgCommandHeader,
            payload: [u8; 4],
        }

        let mut cmd_pkt = CmdPacket {
            hdr: CfeMsgCommandHeader {
                msg: CfeMsgMessage {
                    pri: [0x18, 0x01, 0xC0, 0x00, 0x00, 0x05], // size = 12
                },
                sec: CfeMsgCommandSecondaryHeader {
                    function_code: 0,
                    checksum: 0,
                },
            },
            payload: [0xAA, 0x55, 0x12, 0x34],
        };
        let mut valid = true;

        // SAFETY: `cmd_pkt` is a valid 12-byte command packet; pointers are re-derived after mutation.
        unsafe {
            assert_eq!(
                CFE_MSG_GenerateChecksum(core::ptr::from_mut(&mut cmd_pkt).cast::<CfeMsgMessage>()),
                CFE_SUCCESS
            );
            cmd_pkt.payload[0] ^= 0xFF;
            assert_eq!(
                CFE_MSG_ValidateChecksum(
                    core::ptr::from_ref(&cmd_pkt).cast::<CfeMsgMessage>(),
                    &raw mut valid,
                ),
                CFE_SUCCESS
            );
            assert!(!valid);

            // 8-byte CommandHeader with encoded size 7 exercises `.max(COMMAND_HEADER_LEN)`.
            let mut min_cmd = CfeMsgCommandHeader {
                msg: CfeMsgMessage {
                    pri: [0x18, 0x01, 0xC0, 0x00, 0x00, 0x00], // size = 7
                },
                sec: CfeMsgCommandSecondaryHeader {
                    function_code: 0,
                    checksum: 0,
                },
            };
            let min_cmd_ptr = core::ptr::from_mut(&mut min_cmd).cast::<CfeMsgMessage>();
            assert_eq!(CFE_MSG_GenerateChecksum(min_cmd_ptr), CFE_SUCCESS);
            assert_eq!(
                CFE_MSG_ValidateChecksum(min_cmd_ptr, &raw mut valid),
                CFE_SUCCESS
            );
            assert!(!valid);

            // Wrong-type calls on a bare 6-byte CfeMsgMessage must not read past 6 bytes.
            let mut bare_msg = CfeMsgMessage {
                pri: [0x00, 0x01, 0xC0, 0x00, 0x00, 0x00], // tlm without secondary header
            };
            let bare_ptr = &raw mut bare_msg;
            let mut fcn: CfeMsgFcnCode = 99;
            assert_eq!(
                CFE_MSG_GetFcnCode(bare_ptr, &raw mut fcn),
                CFE_MSG_WRONG_MSG_TYPE
            );
            assert_eq!(fcn, 0);
            assert_eq!(CFE_MSG_SetFcnCode(bare_ptr, 1), CFE_MSG_WRONG_MSG_TYPE);
            assert_eq!(CFE_MSG_GenerateChecksum(bare_ptr), CFE_MSG_WRONG_MSG_TYPE);
            assert_eq!(
                CFE_MSG_ValidateChecksum(bare_ptr, &raw mut valid),
                CFE_MSG_WRONG_MSG_TYPE
            );
            let mut bare_time = CfeTimeSysTime {
                seconds: 1,
                subseconds: 2,
            };
            assert_eq!(
                CFE_MSG_GetMsgTime(bare_ptr, &raw mut bare_time),
                CFE_MSG_WRONG_MSG_TYPE
            );
            assert_eq!(bare_time.seconds, 0);
            assert_eq!(bare_time.subseconds, 0);
            assert_eq!(
                CFE_MSG_SetMsgTime(bare_ptr, bare_time),
                CFE_MSG_WRONG_MSG_TYPE
            );
        }
    }
}
