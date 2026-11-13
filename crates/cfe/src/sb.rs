//! Software Bus (SB) safe wrappers.

use crate::status::Status;

/// A Software Bus message identifier.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MsgId(u32);

impl MsgId {
    /// Create a `MsgId` from a `u32` value.
    #[must_use]
    pub const fn from_u32(val: u32) -> Self {
        Self(val)
    }

    /// Returns the raw `u32` value.
    #[must_use]
    pub const fn as_u32(self) -> u32 {
        self.0
    }

    /// Create from raw cFE `CFE_SB_MsgId_t`.
    #[must_use]
    pub const fn from_raw(raw: cfe_sys::CFE_SB_MsgId_t) -> Self {
        Self(raw.Value)
    }

    /// Convert to raw cFE `CFE_SB_MsgId_t`.
    #[must_use]
    pub const fn to_raw(self) -> cfe_sys::CFE_SB_MsgId_t {
        cfe_sys::CFE_SB_MsgId_t { Value: self.0 }
    }
}

/// A Software Bus pipe identifier.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct PipeId(cfe_sys::CFE_SB_PipeId_t);

impl PipeId {
    /// Create from raw pipe ID.
    #[must_use]
    pub const fn from_raw(raw: cfe_sys::CFE_SB_PipeId_t) -> Self {
        Self(raw)
    }

    /// Return the raw pipe ID.
    #[must_use]
    pub const fn to_raw(self) -> cfe_sys::CFE_SB_PipeId_t {
        self.0
    }
}

/// Quality of Service for Software Bus subscriptions.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Qos {
    /// Message priority (0 = low, 1 = high).
    pub priority: u8,
    /// Message reliability (0 = low, 1 = high).
    pub reliability: u8,
}

impl Qos {
    /// Default Quality of Service.
    pub const DEFAULT: Self = Self {
        priority: 0,
        reliability: 0,
    };

    /// Convert to raw `CFE_SB_Qos_t`.
    #[must_use]
    pub const fn to_raw(self) -> cfe_sys::CFE_SB_Qos_t {
        cfe_sys::CFE_SB_Qos_t {
            Priority: self.priority,
            Reliability: self.reliability,
        }
    }
}

/// Timeout for receiving messages from a pipe.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Timeout {
    /// Wait forever until a message arrives.
    PendForever,
    /// Do not wait; poll the pipe.
    Poll,
    /// Wait up to the specified number of milliseconds.
    Millis(u32),
}

impl Timeout {
    /// Convert to the raw integer value expected by `CFE_SB_ReceiveBuffer`.
    #[must_use]
    pub const fn to_raw(self) -> i32 {
        match self {
            Self::PendForever => cfe_sys::CFE_SB_PEND_FOREVER,
            Self::Poll => cfe_sys::CFE_SB_POLL.cast_signed(),
            Self::Millis(ms) => {
                if ms > i32::MAX.cast_unsigned() {
                    i32::MAX
                } else {
                    ms.cast_signed()
                }
            }
        }
    }
}

/// A Software Bus pipe.
#[derive(Debug)]
pub struct Pipe {
    id: PipeId,
}

impl Pipe {
    /// Creates a new Software Bus pipe with the given queue depth and name.
    ///
    /// # Errors
    ///
    /// Returns `Err(Status)` if pipe creation fails.
    pub fn create(depth: u16, name: &str) -> Result<Self, Status> {
        let mut name_buf = [0u8; 32];
        let len = name.len().min(31);
        let mut i = 0usize;
        for b in name.as_bytes().iter().take(len) {
            if let Some(slot) = name_buf.get_mut(i) {
                *slot = *b;
            }
            i = i.saturating_add(1);
        }
        if let Some(slot) = name_buf.get_mut(i) {
            *slot = 0;
        }

        let mut raw_id = 0u32;
        // SAFETY: PipeIdPtr is a valid out-pointer; name_buf is null-terminated.
        let status =
            unsafe { cfe_sys::CFE_SB_CreatePipe(&raw mut raw_id, depth, name_buf.as_ptr().cast()) };
        let status = Status::from_raw(status);
        if status.is_success() {
            Ok(Self {
                id: PipeId::from_raw(raw_id),
            })
        } else {
            Err(status)
        }
    }

    /// Returns the pipe's identifier.
    #[must_use]
    pub const fn id(&self) -> PipeId {
        self.id
    }

    /// Subscribes this pipe to a message ID with default `QoS` and message limit.
    ///
    /// # Errors
    ///
    /// Returns `Err(Status)` if subscription fails.
    pub fn subscribe(&self, msg_id: MsgId) -> Result<(), Status> {
        // SAFETY: Valid MsgId and PipeId.
        let status = unsafe { cfe_sys::CFE_SB_Subscribe(msg_id.to_raw(), self.id.to_raw()) };
        Status::from_raw(status).result()
    }

    /// Subscribes this pipe to a message ID with custom `QoS` and message limit.
    ///
    /// # Errors
    ///
    /// Returns `Err(Status)` if subscription fails.
    pub fn subscribe_ex(&self, msg_id: MsgId, qos: Qos, msg_limit: u16) -> Result<(), Status> {
        // SAFETY: Valid MsgId, PipeId, and Qos.
        let status = unsafe {
            cfe_sys::CFE_SB_SubscribeEx(msg_id.to_raw(), self.id.to_raw(), qos.to_raw(), msg_limit)
        };
        Status::from_raw(status).result()
    }

    /// Receives a buffer from the pipe.
    ///
    /// Returns a raw buffer pointer valid until the next receive on this pipe.
    ///
    /// # Errors
    ///
    /// Returns `Err(Status)` if pipe read fails or times out.
    pub fn receive_buffer(
        &self,
        timeout: Timeout,
    ) -> Result<*const cfe_sys::CFE_SB_Buffer_t, Status> {
        let mut buf_ptr: *mut cfe_sys::CFE_SB_Buffer_t = core::ptr::null_mut();
        // SAFETY: buf_ptr is a valid pointer-to-pointer; pipe id is valid.
        let status = unsafe {
            cfe_sys::CFE_SB_ReceiveBuffer(&raw mut buf_ptr, self.id.to_raw(), timeout.to_raw())
        };
        let status = Status::from_raw(status);
        if status.is_success() && !buf_ptr.is_null() {
            Ok(buf_ptr)
        } else {
            Err(status)
        }
    }

    /// Deletes this pipe.
    ///
    /// # Errors
    ///
    /// Returns `Err(Status)` if pipe deletion fails.
    pub fn delete(self) -> Result<(), Status> {
        // SAFETY: PipeId is valid.
        let status = unsafe { cfe_sys::CFE_SB_DeletePipe(self.id.to_raw()) };
        Status::from_raw(status).result()
    }
}

/// Transmits a message on the Software Bus.
///
/// # Errors
///
/// Returns `Err(Status)` if `msg` is null or if `CFE_SB_TransmitMsg` fails.
#[expect(
    clippy::not_unsafe_ptr_arg_deref,
    reason = "C API and null checks handle pointer validation"
)]
pub fn transmit_msg(
    msg: *const cfe_sys::CFE_MSG_Message_t,
    is_origination: bool,
) -> Result<(), Status> {
    if msg.is_null() {
        return Err(Status::from_raw(-1));
    }
    // SAFETY: msg pointer is checked non-null.
    let status = unsafe { cfe_sys::CFE_SB_TransmitMsg(msg, is_origination) };
    Status::from_raw(status).result()
}

/// Allocates a zero-copy Software Bus message buffer.
///
/// # Errors
///
/// Returns `Err(Status)` if allocation fails.
pub fn allocate_message_buffer(size: usize) -> Result<*mut cfe_sys::CFE_SB_Buffer_t, Status> {
    // SAFETY: CFE_SB_AllocateMessageBuffer takes a size.
    let ptr = unsafe { cfe_sys::CFE_SB_AllocateMessageBuffer(size) };
    if ptr.is_null() {
        Err(Status::from_raw(-1))
    } else {
        Ok(ptr)
    }
}

/// Releases an unused zero-copy Software Bus message buffer.
///
/// # Errors
///
/// Returns `Err(Status)` if `buf` is null or if `CFE_SB_ReleaseMessageBuffer` fails.
#[expect(
    clippy::not_unsafe_ptr_arg_deref,
    reason = "C API and null checks handle pointer validation"
)]
pub fn release_message_buffer(buf: *mut cfe_sys::CFE_SB_Buffer_t) -> Result<(), Status> {
    if buf.is_null() {
        return Err(Status::from_raw(-1));
    }
    // SAFETY: buf pointer is checked non-null.
    let status = unsafe { cfe_sys::CFE_SB_ReleaseMessageBuffer(buf) };
    Status::from_raw(status).result()
}

/// Transmits a zero-copy message buffer.
///
/// # Errors
///
/// Returns `Err(Status)` if `buf` is null or if `CFE_SB_TransmitBuffer` fails.
#[expect(
    clippy::not_unsafe_ptr_arg_deref,
    reason = "C API and null checks handle pointer validation"
)]
pub fn transmit_buffer(
    buf: *mut cfe_sys::CFE_SB_Buffer_t,
    is_origination: bool,
) -> Result<(), Status> {
    if buf.is_null() {
        return Err(Status::from_raw(-1));
    }
    // SAFETY: buf pointer is checked non-null.
    let status = unsafe { cfe_sys::CFE_SB_TransmitBuffer(buf, is_origination) };
    Status::from_raw(status).result()
}
