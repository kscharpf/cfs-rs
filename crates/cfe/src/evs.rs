//! Event Services (EVS) safe wrappers.

use crate::status::Status;

/// Severity of an EVS event.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(u16)]
pub enum EventType {
    /// Debug event.
    Debug = 1,
    /// Informational event.
    Information = 2,
    /// Error event.
    Error = 3,
    /// Critical event.
    Critical = 4,
}

impl EventType {
    /// Convert to the raw C enum value.
    #[must_use]
    pub const fn to_raw(self) -> cfe_sys::CFE_EVS_EventType_Enum_t {
        match self {
            Self::Debug => 1,
            Self::Information => 2,
            Self::Error => 3,
            Self::Critical => 4,
        }
    }
}

const _: () = {
    assert!(EventType::Debug.to_raw() == 1);
    assert!(EventType::Information.to_raw() == 2);
    assert!(EventType::Error.to_raw() == 3);
    assert!(EventType::Critical.to_raw() == 4);
    assert!(cfe_sys::CFE_EVS_EventType_CFE_EVS_EventType_DEBUG == 1);
    assert!(cfe_sys::CFE_EVS_EventType_CFE_EVS_EventType_INFORMATION == 2);
    assert!(cfe_sys::CFE_EVS_EventType_CFE_EVS_EventType_ERROR == 3);
    assert!(cfe_sys::CFE_EVS_EventType_CFE_EVS_EventType_CRITICAL == 4);
    assert!(cfe_sys::CFE_EVS_EventFilter_CFE_EVS_EventFilter_BINARY == 0);
};

/// Registers the calling application with EVS.
///
/// Uses the default binary filter scheme with no custom filters.
///
/// # Errors
///
/// Returns `Err(Status)` if cFE EVS registration fails.
pub fn register() -> Result<(), Status> {
    // SAFETY: NULL filters with count 0 and BINARY scheme (0) is valid and standard in cFS apps.
    let status = unsafe { cfe_sys::CFE_EVS_Register(core::ptr::null(), 0, 0) };
    Status::from_raw(status).result()
}

#[expect(
    clippy::as_conversions,
    reason = "CFE_MISSION_EVS_MAX_MESSAGE_LENGTH is a C bindgen u32 constant used as buffer size"
)]
const EVS_MAX_MESSAGE_LENGTH: usize = cfe_sys::CFE_MISSION_EVS_MAX_MESSAGE_LENGTH as usize;

/// Maximum length of an event message payload excluding the null terminator.
pub const MAX_EVENT_MESSAGE_LEN: usize = EVS_MAX_MESSAGE_LENGTH - 1;

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
    let mut buf = [0u8; EVS_MAX_MESSAGE_LENGTH];
    if let Some(dest) = buf.get_mut(..message.len()) {
        dest.copy_from_slice(message.as_bytes());
    }

    // SAFETY: %s specifier with a valid null-terminated string buffer.
    let status = unsafe {
        cfe_sys::CFE_EVS_SendEvent(event_id, event_type.to_raw(), c"%s".as_ptr(), buf.as_ptr())
    };
    Status::from_raw(status).result()
}
