//! Event Services (EVS) safe wrappers.

use crate::status::Status;

/// Severity of an EVS event.
#[expect(
    clippy::cast_possible_truncation,
    reason = "cFE event types fit in u16"
)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(u16)]
pub enum EventType {
    /// Debug event.
    Debug = cfe_sys::CFE_EVS_EventType_CFE_EVS_EventType_DEBUG as u16,
    /// Informational event.
    Information = cfe_sys::CFE_EVS_EventType_CFE_EVS_EventType_INFORMATION as u16,
    /// Error event.
    Error = cfe_sys::CFE_EVS_EventType_CFE_EVS_EventType_ERROR as u16,
    /// Critical event.
    Critical = cfe_sys::CFE_EVS_EventType_CFE_EVS_EventType_CRITICAL as u16,
}

impl EventType {
    /// Convert to the raw C enum value.
    #[must_use]
    pub const fn to_raw(self) -> cfe_sys::CFE_EVS_EventType_Enum_t {
        self as cfe_sys::CFE_EVS_EventType_Enum_t
    }
}

/// Registers the calling application with EVS.
///
/// Uses the default binary filter scheme with no custom filters.
///
/// # Errors
///
/// Returns `Err(Status)` if cFE EVS registration fails.
pub fn register() -> Result<(), Status> {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "binary filter scheme fits in uint8"
    )]
    // SAFETY: NULL filters with count 0 and BINARY scheme is valid and standard in cFS apps.
    let status = unsafe {
        cfe_sys::CFE_EVS_Register(
            core::ptr::null(),
            0,
            cfe_sys::CFE_EVS_EventFilter_CFE_EVS_EventFilter_BINARY as _,
        )
    };
    Status::from_raw(status).result()
}

/// Maximum length of an event message payload excluding the null terminator.
pub const MAX_EVENT_MESSAGE_LEN: usize = 127;

/// Sends an event message with the specified ID and severity.
///
/// # Errors
///
/// Returns `Err(Status)` with `CFE_EVS_INVALID_PARAMETER` if `message` exceeds
/// [`MAX_EVENT_MESSAGE_LEN`] bytes, or if cFE EVS fails to deliver the event message.
pub fn send_event(event_id: u16, event_type: EventType, message: &str) -> Result<(), Status> {
    if message.len() > MAX_EVENT_MESSAGE_LEN {
        return Err(Status::from_raw(cfe_sys::CFE_EVS_INVALID_PARAMETER));
    }
    let mut buf = [0u8; 128];
    if let Some(dest) = buf.get_mut(..message.len()) {
        dest.copy_from_slice(message.as_bytes());
    }

    // SAFETY: %s specifier with a valid null-terminated string buffer.
    let status = unsafe {
        cfe_sys::CFE_EVS_SendEvent(event_id, event_type.to_raw(), c"%s".as_ptr(), buf.as_ptr())
    };
    Status::from_raw(status).result()
}
