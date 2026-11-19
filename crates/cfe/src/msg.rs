//! Safe wrappers for cFE Message (MSG) functions.

use crate::sb::MsgId;
use crate::status::Status;

/// A cFE message header type alias.
pub type Message = cfe_sys::CFE_MSG_Message_t;

const _: () = {
    assert!(core::mem::align_of::<cfe_sys::CFE_MSG_Message_t>() == 1);
    assert!(core::mem::size_of::<cfe_sys::CFE_MSG_Message_t>() == 6);
    assert!(core::mem::offset_of!(cfe_sys::CFE_MSG_CommandHeader_t, Msg) == 0);
    assert!(core::mem::offset_of!(cfe_sys::CFE_MSG_TelemetryHeader_t, Msg) == 0);
};

/// A `#[repr(C)]` cFE message type backed by a valid, zero-initializable buffer starting with
/// [`cfe_sys::CFE_MSG_Message_t`] at byte offset `0`.
///
/// # Safety
/// Implementors must guarantee all of the following:
/// 1. `Self` is `#[repr(C)]` and begins with a `cfe_sys::CFE_MSG_Message_t` field at byte offset
///    `0`, and [`as_header`](Self::as_header) / [`as_header_mut`](Self::as_header_mut) return
///    references to that field.
/// 2. `core::mem::size_of::<Self>() >= core::mem::size_of::<cfe_sys::CFE_MSG_Message_t>()`.
/// 3. An all-zero byte pattern of `core::mem::size_of::<Self>()` bytes is a valid value of `Self`
///    (required because `CFE_MSG_Init` zeroes the entire message via `memset`).
/// 4. All `core::mem::size_of::<Self>()` bytes of a live `&Self` are initialized (no uninitialized
///    padding bytes are read by `CFE_SB_TransmitMsg`).
pub unsafe trait CfeMessage: Sized {
    /// Returns a shared reference to the leading generic cFE message header.
    fn as_header(&self) -> &cfe_sys::CFE_MSG_Message_t;

    /// Returns an exclusive reference to the leading generic cFE message header.
    fn as_header_mut(&mut self) -> &mut cfe_sys::CFE_MSG_Message_t;
}

// SAFETY: `CFE_MSG_CommandHeader_t` is `#[repr(C)]`, has `Msg` at offset 0 (asserted above),
// has size 8 >= 6, contains only `u8`/byte arrays (valid when all-zero and free of padding).
unsafe impl CfeMessage for cfe_sys::CFE_MSG_CommandHeader_t {
    fn as_header(&self) -> &cfe_sys::CFE_MSG_Message_t {
        &self.Msg
    }

    fn as_header_mut(&mut self) -> &mut cfe_sys::CFE_MSG_Message_t {
        &mut self.Msg
    }
}

// SAFETY: `CFE_MSG_TelemetryHeader_t` is `#[repr(C)]`, has `Msg` at offset 0 (asserted above),
// has size 16 >= 6, contains only `u8`/byte arrays (valid when all-zero and free of padding).
unsafe impl CfeMessage for cfe_sys::CFE_MSG_TelemetryHeader_t {
    fn as_header(&self) -> &cfe_sys::CFE_MSG_Message_t {
        &self.Msg
    }

    fn as_header_mut(&mut self) -> &mut cfe_sys::CFE_MSG_Message_t {
        &mut self.Msg
    }
}

/// Initializes a typed cFE message header and clears its full `size_of::<M>()` backing storage
/// to zero.
///
/// # Errors
///
/// Returns `Err(Status)` if `CFE_MSG_Init` fails.
pub fn init<M: CfeMessage>(msg: &mut M, msg_id: MsgId) -> Result<(), Status> {
    let ptr = core::ptr::from_mut(msg).cast::<cfe_sys::CFE_MSG_Message_t>();
    // SAFETY: `M: CfeMessage` guarantees `msg` is `size_of::<M>()` writable bytes starting with
    // `CFE_MSG_Message_t` at offset 0 and that an all-zero byte pattern is valid for `M`.
    let status = unsafe { cfe_sys::CFE_MSG_Init(ptr, msg_id.to_raw(), core::mem::size_of::<M>()) };
    Status::from_raw(status).result()
}

/// Initializes a cFE message header inside `buf` and clears all `buf.len()` bytes to zero.
///
/// # Errors
///
/// Returns `Err(Status)` with `CFE_MSG_BAD_ARGUMENT` if `buf` is shorter than
/// `size_of::<CFE_MSG_Message_t>()`, or if `CFE_MSG_Init` fails.
pub fn init_bytes(buf: &mut [u8], msg_id: MsgId) -> Result<(), Status> {
    if buf.len() < core::mem::size_of::<cfe_sys::CFE_MSG_Message_t>() {
        return Err(Status::from_raw(cfe_sys::CFE_MSG_BAD_ARGUMENT));
    }
    let ptr = buf.as_mut_ptr().cast::<cfe_sys::CFE_MSG_Message_t>();
    // SAFETY: `align_of::<CFE_MSG_Message_t>() == 1` (asserted above), `buf` is writable for
    // `buf.len() >= size_of::<CFE_MSG_Message_t>()` bytes, and `u8` has no invalid bit patterns.
    let status = unsafe { cfe_sys::CFE_MSG_Init(ptr, msg_id.to_raw(), buf.len()) };
    Status::from_raw(status).result()
}

/// Gets the total size of a message.
///
/// # Errors
///
/// Returns `Err(Status)` if `CFE_MSG_GetSize` fails.
pub fn get_size(msg: &cfe_sys::CFE_MSG_Message_t) -> Result<usize, Status> {
    let mut size: cfe_sys::CFE_MSG_Size_t = 0;
    // SAFETY: msg is a valid reference and size points to a valid local variable.
    let status = unsafe { cfe_sys::CFE_MSG_GetSize(core::ptr::from_ref(msg), &raw mut size) };
    let status = Status::from_raw(status);
    if status.is_success() {
        Ok(size)
    } else {
        Err(status)
    }
}

/// Sets the total size of a message.
///
/// # Errors
///
/// Returns `Err(Status)` if `CFE_MSG_SetSize` fails.
pub fn set_size(msg: &mut cfe_sys::CFE_MSG_Message_t, size: usize) -> Result<(), Status> {
    // SAFETY: msg is a valid reference and guaranteed non-null.
    let status = unsafe { cfe_sys::CFE_MSG_SetSize(core::ptr::from_mut(msg), size) };
    Status::from_raw(status).result()
}

/// Gets the message ID of a message.
///
/// # Errors
///
/// Returns `Err(Status)` if `CFE_MSG_GetMsgId` fails.
pub fn get_msg_id(msg: &cfe_sys::CFE_MSG_Message_t) -> Result<MsgId, Status> {
    let mut raw_id = cfe_sys::CFE_SB_MsgId_t { Value: 0 };
    // SAFETY: msg is a valid reference and raw_id points to a valid local variable.
    let status = unsafe { cfe_sys::CFE_MSG_GetMsgId(core::ptr::from_ref(msg), &raw mut raw_id) };
    let status = Status::from_raw(status);
    if status.is_success() {
        Ok(MsgId::from_raw(raw_id))
    } else {
        Err(status)
    }
}

/// Sets the message ID of a message.
///
/// # Errors
///
/// Returns `Err(Status)` if `CFE_MSG_SetMsgId` fails.
pub fn set_msg_id(msg: &mut cfe_sys::CFE_MSG_Message_t, msg_id: MsgId) -> Result<(), Status> {
    // SAFETY: msg is a valid reference and guaranteed non-null.
    let status = unsafe { cfe_sys::CFE_MSG_SetMsgId(core::ptr::from_mut(msg), msg_id.to_raw()) };
    Status::from_raw(status).result()
}
