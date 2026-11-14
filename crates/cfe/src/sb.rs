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
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
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
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
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

/// A borrowed view of a buffer received from a Software Bus pipe.
///
/// The buffer is valid until the mutable borrow of the [`Pipe`] ends.
#[derive(Debug)]
pub struct ReceivedBuffer<'a> {
    ptr: *const cfe_sys::CFE_SB_Buffer_t,
    _pipe: core::marker::PhantomData<&'a Pipe>,
}

impl ReceivedBuffer<'_> {
    /// Returns a shared reference to the message header.
    #[must_use]
    pub const fn msg(&self) -> &cfe_sys::CFE_MSG_Message_t {
        // SAFETY: ptr is non-null and points to a valid CFE_SB_Buffer_t whose first field is CFE_MSG_Message_t.
        unsafe { &(*self.ptr).Msg }
    }

    /// Returns the raw pointer to the underlying buffer.
    #[must_use]
    pub const fn as_ptr(&self) -> *const cfe_sys::CFE_SB_Buffer_t {
        self.ptr
    }

    /// Returns the message ID of the received message.
    ///
    /// # Errors
    ///
    /// Returns `Err(Status)` if querying the message ID fails.
    pub fn msg_id(&self) -> Result<MsgId, Status> {
        let mut raw_id = cfe_sys::CFE_SB_MsgId_t { Value: 0 };
        // SAFETY: self.msg() is a valid pointer.
        let status = unsafe { cfe_sys::CFE_MSG_GetMsgId(self.msg(), &raw mut raw_id) };
        let status = Status::from_raw(status);
        if status.is_success() {
            Ok(MsgId::from_raw(raw_id))
        } else {
            Err(status)
        }
    }

    /// Returns a slice of the total message bytes including headers.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        let mut total_size = 0usize;
        // SAFETY: self.msg() is a valid pointer.
        let status = unsafe { cfe_sys::CFE_MSG_GetSize(self.msg(), &raw mut total_size) };
        if status != cfe_sys::CFE_SUCCESS.cast_signed() || total_size == 0 {
            return &[];
        }
        let ptr = core::ptr::from_ref(self.msg()).cast::<u8>();
        // SAFETY: CFE_MSG_GetSize returned a valid size and msg points to at least total_size initialized bytes.
        unsafe { core::slice::from_raw_parts(ptr, total_size) }
    }

    /// Returns a slice of the message user data payload.
    #[must_use]
    pub fn user_data(&self) -> &[u8] {
        let msg_ptr = core::ptr::from_ref(self.msg()).cast_mut();
        // SAFETY: msg_ptr is non-null and points to a valid message.
        let data = unsafe { cfe_sys::CFE_SB_GetUserData(msg_ptr) };
        // SAFETY: msg_ptr is non-null and points to a valid message.
        let len = unsafe { cfe_sys::CFE_SB_GetUserDataLength(self.msg()) };
        if data.is_null() || len == 0 {
            &[]
        } else {
            // SAFETY: data points to len initialized payload bytes.
            unsafe { core::slice::from_raw_parts(data.cast::<u8>(), len) }
        }
    }
}

/// A zero-copy Software Bus message buffer allocated via [`allocate_message_buffer`].
///
/// If dropped before being passed to [`transmit_buffer`], the underlying buffer
/// is automatically released via `CFE_SB_ReleaseMessageBuffer`.
#[derive(Debug)]
pub struct MessageBuffer {
    ptr: *mut cfe_sys::CFE_SB_Buffer_t,
    size: usize,
}

impl MessageBuffer {
    /// Returns the allocated buffer size in bytes.
    #[must_use]
    pub const fn size(&self) -> usize {
        self.size
    }

    /// Returns a shared reference to the message header.
    #[must_use]
    pub const fn msg(&self) -> &cfe_sys::CFE_MSG_Message_t {
        // SAFETY: self.ptr is non-null, properly aligned, and points to an allocated SB buffer.
        unsafe { &(*self.ptr).Msg }
    }

    /// Returns an exclusive reference to the message header.
    pub const fn msg_mut(&mut self) -> &mut cfe_sys::CFE_MSG_Message_t {
        // SAFETY: self.ptr is non-null, properly aligned, and points to an allocated SB buffer.
        unsafe { &mut (*self.ptr).Msg }
    }

    /// Returns a shared slice of the buffer's raw bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8] {
        // SAFETY: self.ptr is non-null and valid for reads of self.size bytes.
        unsafe { core::slice::from_raw_parts(self.ptr.cast::<u8>(), self.size) }
    }

    /// Returns an exclusive slice of the buffer's raw bytes.
    pub const fn as_bytes_mut(&mut self) -> &mut [u8] {
        // SAFETY: self.ptr is non-null and valid for reads/writes of self.size bytes.
        unsafe { core::slice::from_raw_parts_mut(self.ptr.cast::<u8>(), self.size) }
    }

    /// Explicitly releases the buffer back to the Software Bus.
    ///
    /// # Errors
    ///
    /// Returns `Err(Status)` if releasing the buffer fails.
    pub fn release(mut self) -> Result<(), Status> {
        let ptr = self.ptr;
        self.ptr = core::ptr::null_mut();
        // SAFETY: ptr is non-null and was allocated via CFE_SB_AllocateMessageBuffer.
        let status = unsafe { cfe_sys::CFE_SB_ReleaseMessageBuffer(ptr) };
        Status::from_raw(status).result()
    }
}

impl Drop for MessageBuffer {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            // SAFETY: self.ptr is non-null and was not transmitted.
            unsafe {
                let _ = cfe_sys::CFE_SB_ReleaseMessageBuffer(self.ptr);
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
    /// Returns `Err(Status)` with `CFE_SB_BAD_ARGUMENT` if `name` is longer than
    /// `OS_MAX_API_NAME - 1` bytes, or if cFE pipe creation fails.
    pub fn create(depth: u16, name: &str) -> Result<Self, Status> {
        let max_len = cfe_sys::OS_MAX_API_NAME as usize;
        if name.len() >= max_len {
            return Err(Status::from_raw(cfe_sys::CFE_SB_BAD_ARGUMENT));
        }

        let mut name_buf = [0u8; cfe_sys::OS_MAX_API_NAME as usize];
        if let Some(dest) = name_buf.get_mut(..name.len()) {
            dest.copy_from_slice(name.as_bytes());
        }

        let mut raw_id = 0u32;
        // SAFETY: PipeIdPtr is a valid out-pointer; name_buf is null-terminated and bounded by OS_MAX_API_NAME.
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
    /// Returns a [`ReceivedBuffer`] view valid until the borrow of this pipe ends.
    ///
    /// # Errors
    ///
    /// Returns `Err(Status)` if pipe read fails, times out, or if the buffer pointer is null.
    pub fn receive_buffer(&mut self, timeout: Timeout) -> Result<ReceivedBuffer<'_>, Status> {
        let mut buf_ptr: *mut cfe_sys::CFE_SB_Buffer_t = core::ptr::null_mut();
        // SAFETY: buf_ptr is a valid pointer-to-pointer; pipe id is valid.
        let status = unsafe {
            cfe_sys::CFE_SB_ReceiveBuffer(&raw mut buf_ptr, self.id.to_raw(), timeout.to_raw())
        };
        let status = Status::from_raw(status);
        if !status.is_success() {
            Err(status)
        } else if buf_ptr.is_null() {
            Err(Status::from_raw(cfe_sys::CFE_SB_PIPE_RD_ERR))
        } else {
            Ok(ReceivedBuffer {
                ptr: buf_ptr,
                _pipe: core::marker::PhantomData,
            })
        }
    }

    /// Returns options currently set on this pipe.
    ///
    /// # Errors
    ///
    /// Returns `Err(Status)` if querying pipe options fails.
    pub fn opts(&self) -> Result<u8, Status> {
        let mut opts = 0u8;
        // SAFETY: self.id is a valid pipe ID, opts is a valid out-pointer.
        let status = unsafe { cfe_sys::CFE_SB_GetPipeOpts(self.id.to_raw(), &raw mut opts) };
        let status = Status::from_raw(status);
        if status.is_success() {
            Ok(opts)
        } else {
            Err(status)
        }
    }

    /// Returns the name of this pipe.
    ///
    /// # Errors
    ///
    /// Returns `Err(Status)` if querying pipe name fails or name is not valid UTF-8.
    pub fn name<'a>(
        &self,
        buf: &'a mut [u8; cfe_sys::OS_MAX_API_NAME as usize],
    ) -> Result<&'a str, Status> {
        // SAFETY: self.id is a valid pipe ID, buf is a valid buffer of OS_MAX_API_NAME bytes.
        let status = unsafe {
            cfe_sys::CFE_SB_GetPipeName(buf.as_mut_ptr().cast(), buf.len(), self.id.to_raw())
        };
        let status = Status::from_raw(status);
        if !status.is_success() {
            return Err(status);
        }
        let nul_pos = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        let slice = buf
            .get(..nul_pos)
            .ok_or(Status::from_raw(cfe_sys::CFE_SB_BAD_ARGUMENT))?;
        core::str::from_utf8(slice).map_err(|_| Status::from_raw(cfe_sys::CFE_SB_BAD_ARGUMENT))
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
/// Returns `Err(Status)` if `CFE_SB_TransmitMsg` fails.
pub fn transmit_msg(msg: &cfe_sys::CFE_MSG_Message_t, is_origination: bool) -> Result<(), Status> {
    // SAFETY: msg is a valid reference and guaranteed non-null.
    let status = unsafe { cfe_sys::CFE_SB_TransmitMsg(core::ptr::from_ref(msg), is_origination) };
    Status::from_raw(status).result()
}

/// Allocates a zero-copy Software Bus message buffer.
///
/// # Errors
///
/// Returns `Err(Status)` with `CFE_SB_BUF_ALOC_ERR` if allocation fails, or
/// `CFE_SB_BAD_ARGUMENT` if `size` is smaller than `CFE_MSG_Message_t`.
pub fn allocate_message_buffer(size: usize) -> Result<MessageBuffer, Status> {
    if size < core::mem::size_of::<cfe_sys::CFE_MSG_Message_t>() {
        return Err(Status::from_raw(cfe_sys::CFE_SB_BAD_ARGUMENT));
    }
    // SAFETY: CFE_SB_AllocateMessageBuffer takes a size.
    let ptr = unsafe { cfe_sys::CFE_SB_AllocateMessageBuffer(size) };
    if ptr.is_null() {
        Err(Status::from_raw(cfe_sys::CFE_SB_BUF_ALOC_ERR))
    } else {
        Ok(MessageBuffer { ptr, size })
    }
}

/// Releases an unused zero-copy Software Bus message buffer.
///
/// # Errors
///
/// Returns `Err(Status)` if `CFE_SB_ReleaseMessageBuffer` fails.
pub fn release_message_buffer(buf: MessageBuffer) -> Result<(), Status> {
    buf.release()
}

/// Transmits a zero-copy message buffer.
///
/// Consumes the [`MessageBuffer`]. On success, cFE takes ownership of the buffer.
/// On failure, the buffer is dropped and automatically released via `CFE_SB_ReleaseMessageBuffer`.
///
/// # Errors
///
/// Returns `Err(Status)` if `CFE_SB_TransmitBuffer` fails.
pub fn transmit_buffer(mut buf: MessageBuffer, is_origination: bool) -> Result<(), Status> {
    let ptr = buf.ptr;
    // SAFETY: ptr is non-null and allocated by cFE SB.
    let status = unsafe { cfe_sys::CFE_SB_TransmitBuffer(ptr, is_origination) };
    let status = Status::from_raw(status);
    if status.is_success() {
        // cFE now owns the buffer; prevent Drop from releasing it.
        buf.ptr = core::ptr::null_mut();
        Ok(())
    } else {
        // buf will drop here and release the buffer.
        Err(status)
    }
}

/// Checks that the Software Bus and underlying OSAL queue can hold `depth` messages
/// without silent truncation.
///
/// Creates a temporary pipe with depth `depth`, subscribes to `test_mid` with limit `depth`,
/// transmits `depth` messages, verifies that none fail with `CFE_SB_PIPE_WR_ERR`,
/// drains the pipe, and deletes it.
///
/// # Errors
///
/// Returns `Err(Status)` if pipe creation, subscription, or message transmission fails.
pub fn check_pipe_depth(depth: u16, test_mid: MsgId) -> Result<(), Status> {
    let pipe = Pipe::create(depth, "DEPTH_TEST_PIPE")?;
    if let Err(e) = pipe.subscribe_ex(test_mid, Qos::DEFAULT, depth) {
        let _ = pipe.delete();
        return Err(e);
    }

    let mut cmd_msg = cfe_sys::CFE_MSG_CommandHeader_t {
        Msg: cfe_sys::CFE_MSG_Message_t {
            CCSDS: cfe_sys::CCSDS_SpacePacket_t {
                Pri: cfe_sys::CCSDS_PrimaryHeader {
                    StreamId: [0, 0],
                    Sequence: [0, 0],
                    Length: [0, 0],
                },
            },
        },
        Sec: cfe_sys::CFE_MSG_CommandSecondaryHeader_t {
            FunctionCode: 0,
            Checksum: 0,
        },
    };
    // SAFETY: Initializing command header with test_mid and valid size >= 7.
    let init_status = unsafe {
        cfe_sys::CFE_MSG_Init(
            &raw mut cmd_msg.Msg,
            test_mid.to_raw(),
            core::mem::size_of::<cfe_sys::CFE_MSG_CommandHeader_t>(),
        )
    };
    if init_status != cfe_sys::CFE_SUCCESS.cast_signed() {
        let _ = pipe.delete();
        return Err(Status::from_raw(init_status));
    }

    let mut send_err = None;
    for _ in 0..depth {
        let tx_status = transmit_msg(&cmd_msg.Msg, true);
        if let Err(e) = tx_status {
            send_err = Some(e);
            break;
        }
    }

    // Drain all queued messages
    let mut drain_pipe = pipe;
    for _ in 0..depth {
        let _ = drain_pipe.receive_buffer(Timeout::Poll);
    }
    let _ = drain_pipe.delete();

    send_err.map_or(Ok(()), Err)
}
