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

const STATUS_PREFIX_SEP: &[u8] = b" (status=0x";
const STATUS_HEX_LEN: usize = 8;
const STATUS_SUFFIX_LEN: usize = STATUS_PREFIX_SEP.len() + STATUS_HEX_LEN + 1;

/// Maximum length in bytes of the prefix passed to [`send_event_with_status`].
pub const MAX_STATUS_PREFIX_LEN: usize = MAX_EVENT_MESSAGE_LEN - STATUS_SUFFIX_LEN;

const _: () = {
    assert!(MAX_EVENT_MESSAGE_LEN > STATUS_SUFFIX_LEN);
    assert!(STATUS_SUFFIX_LEN == 20);
};

/// Formats `"<prefix> (status=0xXXXXXXXX)\0"` into `buf` without `core::fmt`, returning the
/// payload byte length (excluding the trailing NUL).
pub(crate) fn format_event_with_status(
    prefix: &str,
    status: Status,
    buf: &mut [u8; EVS_MAX_MESSAGE_LENGTH],
) -> Result<usize, Status> {
    const HEX_DIGITS: [u8; 16] = *b"0123456789ABCDEF";
    const NIBBLE_SHIFTS: [u32; STATUS_HEX_LEN] = [28, 24, 20, 16, 12, 8, 4, 0];

    if prefix.len() > MAX_STATUS_PREFIX_LEN || prefix.as_bytes().contains(&0) {
        return Err(Status::from_raw(cfe_sys::CFE_EVS_INVALID_PARAMETER));
    }

    let prefix_len = prefix.len();
    let sep_end = prefix_len.saturating_add(STATUS_PREFIX_SEP.len());
    let hex_end = sep_end.saturating_add(STATUS_HEX_LEN);
    let total_len = hex_end.saturating_add(1);

    if let Some(dest) = buf.get_mut(..prefix_len) {
        dest.copy_from_slice(prefix.as_bytes());
    }
    if let Some(dest) = buf.get_mut(prefix_len..sep_end) {
        dest.copy_from_slice(STATUS_PREFIX_SEP);
    }
    let raw = status.as_raw().cast_unsigned();
    for (idx, shift) in NIBBLE_SHIFTS.iter().enumerate() {
        let nibble = usize::try_from((raw >> shift) & 0x0F).unwrap_or(0);
        if let (Some(&digit), Some(slot)) = (
            HEX_DIGITS.get(nibble),
            buf.get_mut(sep_end.saturating_add(idx)),
        ) {
            *slot = digit;
        }
    }
    if let Some(slot) = buf.get_mut(hex_end) {
        *slot = b')';
    }
    if let Some(nul) = buf.get_mut(total_len) {
        *nul = 0;
    }
    Ok(total_len)
}

/// Sends an event message with the specified ID and severity.
///
/// # Errors
///
/// Returns `Err(Status)` with `CFE_EVS_INVALID_PARAMETER` if `message` exceeds
/// [`MAX_EVENT_MESSAGE_LEN`] bytes or contains interior null bytes, or if cFE EVS fails to
/// deliver the event message.
pub fn send_event(event_id: u16, event_type: EventType, message: &str) -> Result<(), Status> {
    if message.len() > MAX_EVENT_MESSAGE_LEN || message.as_bytes().contains(&0) {
        return Err(Status::from_raw(cfe_sys::CFE_EVS_INVALID_PARAMETER));
    }
    let mut buf = [0u8; EVS_MAX_MESSAGE_LENGTH];
    if let Some(dest) = buf.get_mut(..message.len()) {
        dest.copy_from_slice(message.as_bytes());
    }

    // SAFETY: %s specifier with a valid null-terminated string buffer.
    let status = unsafe {
        cfe_sys::CFE_EVS_SendEvent(
            event_id,
            event_type.to_raw(),
            c"%s".as_ptr(),
            buf.as_ptr().cast::<core::ffi::c_char>(),
        )
    };
    Status::from_raw(status).result()
}

/// Sends an event message formatted as `"<prefix> (status=0xXXXXXXXX)"`.
///
/// # Errors
///
/// Returns `Err(Status)` with `CFE_EVS_INVALID_PARAMETER` if `prefix` exceeds
/// [`MAX_STATUS_PREFIX_LEN`] bytes or contains interior null bytes, or if cFE EVS fails to
/// deliver the event message.
pub fn send_event_with_status(
    event_id: u16,
    event_type: EventType,
    prefix: &str,
    status: Status,
) -> Result<(), Status> {
    let mut buf = [0u8; EVS_MAX_MESSAGE_LENGTH];
    let _ = format_event_with_status(prefix, status, &mut buf)?;

    // SAFETY: %s specifier with a valid null-terminated string buffer populated by
    // `format_event_with_status`.
    let raw_status = unsafe {
        cfe_sys::CFE_EVS_SendEvent(
            event_id,
            event_type.to_raw(),
            c"%s".as_ptr(),
            buf.as_ptr().cast::<core::ffi::c_char>(),
        )
    };
    Status::from_raw(raw_status).result()
}
