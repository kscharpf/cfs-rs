//! Safe wrappers for cFE Message (MSG) functions.

use crate::sb::MsgId;
use crate::status::Status;

/// A cFE message header type alias.
pub type Message = cfe_sys::CFE_MSG_Message_t;

/// Initializes a message header and clears payload to zero based on `size`.
///
/// # Errors
///
/// Returns `Err(Status)` if `CFE_MSG_Init` fails.
pub fn init(
    msg: &mut cfe_sys::CFE_MSG_Message_t,
    msg_id: MsgId,
    size: usize,
) -> Result<(), Status> {
    // SAFETY: msg is a valid reference and guaranteed non-null.
    let status = unsafe { cfe_sys::CFE_MSG_Init(core::ptr::from_mut(msg), msg_id.to_raw(), size) };
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
