//! Safe wrappers for cFE Message (MSG) functions.

use crate::sb::MsgId;
use crate::status::Status;

/// Initializes a message header and clears payload to zero based on `size`.
///
/// # Errors
///
/// Returns `Err(Status)` if `msg` is null or if `CFE_MSG_Init` fails.
#[expect(
    clippy::not_unsafe_ptr_arg_deref,
    reason = "C API and null checks handle pointer validation"
)]
pub fn init(
    msg: *mut cfe_sys::CFE_MSG_Message_t,
    msg_id: MsgId,
    size: usize,
) -> Result<(), Status> {
    if msg.is_null() {
        return Err(Status::from_raw(-1));
    }
    // SAFETY: msg is validated non-null.
    let status = unsafe { cfe_sys::CFE_MSG_Init(msg, msg_id.to_raw(), size) };
    Status::from_raw(status).result()
}

/// Gets the total size of a message.
///
/// # Errors
///
/// Returns `Err(Status)` if `msg` is null or if `CFE_MSG_GetSize` fails.
#[expect(
    clippy::not_unsafe_ptr_arg_deref,
    reason = "C API and null checks handle pointer validation"
)]
pub fn get_size(msg: *const cfe_sys::CFE_MSG_Message_t) -> Result<usize, Status> {
    if msg.is_null() {
        return Err(Status::from_raw(-1));
    }
    let mut size: cfe_sys::CFE_MSG_Size_t = 0;
    // SAFETY: msg is non-null and size points to a valid local variable.
    let status = unsafe { cfe_sys::CFE_MSG_GetSize(msg, &raw mut size) };
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
/// Returns `Err(Status)` if `msg` is null or if `CFE_MSG_SetSize` fails.
#[expect(
    clippy::not_unsafe_ptr_arg_deref,
    reason = "C API and null checks handle pointer validation"
)]
pub fn set_size(msg: *mut cfe_sys::CFE_MSG_Message_t, size: usize) -> Result<(), Status> {
    if msg.is_null() {
        return Err(Status::from_raw(-1));
    }
    // SAFETY: msg is validated non-null.
    let status = unsafe { cfe_sys::CFE_MSG_SetSize(msg, size) };
    Status::from_raw(status).result()
}

/// Gets the message ID of a message.
///
/// # Errors
///
/// Returns `Err(Status)` if `msg` is null or if `CFE_MSG_GetMsgId` fails.
#[expect(
    clippy::not_unsafe_ptr_arg_deref,
    reason = "C API and null checks handle pointer validation"
)]
pub fn get_msg_id(msg: *const cfe_sys::CFE_MSG_Message_t) -> Result<MsgId, Status> {
    if msg.is_null() {
        return Err(Status::from_raw(-1));
    }
    let mut raw_id = cfe_sys::CFE_SB_MsgId_t { Value: 0 };
    // SAFETY: msg is non-null and raw_id points to a valid local variable.
    let status = unsafe { cfe_sys::CFE_MSG_GetMsgId(msg, &raw mut raw_id) };
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
/// Returns `Err(Status)` if `msg` is null or if `CFE_MSG_SetMsgId` fails.
#[expect(
    clippy::not_unsafe_ptr_arg_deref,
    reason = "C API and null checks handle pointer validation"
)]
pub fn set_msg_id(msg: *mut cfe_sys::CFE_MSG_Message_t, msg_id: MsgId) -> Result<(), Status> {
    if msg.is_null() {
        return Err(Status::from_raw(-1));
    }
    // SAFETY: msg is validated non-null.
    let status = unsafe { cfe_sys::CFE_MSG_SetMsgId(msg, msg_id.to_raw()) };
    Status::from_raw(status).result()
}
