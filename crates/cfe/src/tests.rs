#![allow(
    non_snake_case,
    non_upper_case_globals,
    unreachable_pub,
    unused_variables,
    clippy::all,
    clippy::pedantic,
    clippy::nursery,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    unsafe_code
)]

extern crate std;

use crate::es::{self, RunStatus};
use crate::evs::{self, EventType};
use crate::msg;
use crate::osal::{self, TcpStream, UdpSocket, local_time_us};
use crate::sb::{self, MsgId, Pipe, PipeId, Qos, Timeout};
use crate::status::Status;
use crate::tbl::{self, Table, TableOptions};
use core::net::Ipv4Addr;
use core::num::NonZeroUsize;
use std::collections::HashSet;
use std::sync::Mutex;
use std::sync::atomic::{
    AtomicBool, AtomicI32, AtomicI64, AtomicU16, AtomicU32, AtomicUsize, Ordering,
};

static RUN_LOOP_CALLS: AtomicU32 = AtomicU32::new(0);
static WAIT_SYNC_MS: AtomicU32 = AtomicU32::new(0);
static EXIT_APP_STATUS: AtomicU32 = AtomicU32::new(0);
static EVS_REGISTER_CALLS: AtomicU32 = AtomicU32::new(0);
static LAST_EVENT_ID: AtomicU16 = AtomicU16::new(0);
static LAST_EVENT_TYPE: AtomicU16 = AtomicU16::new(0);
static PIPE_CREATE_CALLS: AtomicU32 = AtomicU32::new(0);
static PIPE_DELETE_CALLS: AtomicU32 = AtomicU32::new(0);
static SIMULATE_PIPE_INVALID_ID: AtomicBool = AtomicBool::new(false);
static SIMULATE_SUBSCRIBE_FAIL: AtomicBool = AtomicBool::new(false);
static SUBSCRIBED_MID: AtomicU32 = AtomicU32::new(0);
static SUBSCRIBED_LIMIT: AtomicU16 = AtomicU16::new(0);
static SOCKET_TEST_LOCK: Mutex<()> = Mutex::new(());
static OS_SOCKET_OPEN_CALLS: AtomicU32 = AtomicU32::new(0);
static OS_SOCKET_CONNECT_CALLS: AtomicU32 = AtomicU32::new(0);
static OS_SOCKET_BIND_CALLS: AtomicU32 = AtomicU32::new(0);
static OS_SOCKET_SEND_CALLS: AtomicU32 = AtomicU32::new(0);
static OS_SOCKET_SENDTO_CALLS: AtomicU32 = AtomicU32::new(0);
static OS_SOCKET_RECV_CALLS: AtomicU32 = AtomicU32::new(0);
static OS_TIMED_READ_CALLS: AtomicU32 = AtomicU32::new(0);
static LAST_TIMED_READ_TIMEOUT: AtomicI32 = AtomicI32::new(0);
static OS_SOCKET_RECVFROM_CALLS: AtomicU32 = AtomicU32::new(0);
static OS_CLOSE_CALLS: AtomicU32 = AtomicU32::new(0);
static SIMULATE_SOCKET_FAIL: AtomicBool = AtomicBool::new(false);
static SIMULATE_BIND_FAIL: AtomicBool = AtomicBool::new(false);
static UDP_RECVFROM_MODE: AtomicI32 = AtomicI32::new(0);
static UDP_SENDTO_FAIL: AtomicBool = AtomicBool::new(false);
static LAST_UDP_PORT: AtomicU16 = AtomicU16::new(0);
static LAST_UDP_HOST: Mutex<([u8; 64], usize)> = Mutex::new(([0; 64], 0));
static LOCAL_TIME_TICKS: AtomicI64 = AtomicI64::new(12_345_670);
static SIMULATE_TIME_FAIL: AtomicBool = AtomicBool::new(false);
static SIMULATE_RECEIVE_NULL_BUFFER: AtomicBool = AtomicBool::new(false);
static SIMULATE_RECEIVE_ERROR: AtomicBool = AtomicBool::new(false);
static SIMULATE_ALLOC_FAIL: AtomicBool = AtomicBool::new(false);
static RELEASE_CALLS: AtomicU32 = AtomicU32::new(0);
static TRANSMIT_BUFFER_CALLS: AtomicU32 = AtomicU32::new(0);
static TRANSMIT_MSG_CALLS: AtomicU32 = AtomicU32::new(0);
static LAST_INIT_SIZE: AtomicUsize = AtomicUsize::new(0);
static RECV_CHUNK_MODE: AtomicU32 = AtomicU32::new(0);
static SEND_CHUNK_MODE: AtomicU32 = AtomicU32::new(0);
static DUMMY_USER_DATA: [u8; 16] = [0xAA; 16];

const MOCK_SB_PAYLOAD_LEN: usize = 128 - core::mem::size_of::<cfe_sys::CFE_SB_Buffer_t>();

#[repr(C)]
struct MockSbBuffer {
    buf: cfe_sys::CFE_SB_Buffer_t,
    payload: [u8; MOCK_SB_PAYLOAD_LEN],
}

const _: () = assert!(core::mem::size_of::<MockSbBuffer>() == 128);

static mut STATIC_BUFFER: MockSbBuffer = MockSbBuffer {
    buf: cfe_sys::CFE_SB_Buffer_t {
        Msg: cfe_sys::CFE_MSG_Message {
            CCSDS: cfe_sys::CCSDS_SpacePacket_t {
                Pri: cfe_sys::CCSDS_PrimaryHeader {
                    StreamId: [0, 0],
                    Sequence: [0, 0],
                    Length: [0, 0],
                },
            },
        },
    },
    payload: [0xAA; MOCK_SB_PAYLOAD_LEN],
};

#[unsafe(no_mangle)]
pub unsafe extern "C" fn CFE_ES_RunLoop(RunStatus: *mut u32) -> bool {
    let calls = RUN_LOOP_CALLS.fetch_add(1, Ordering::SeqCst);
    if !RunStatus.is_null() {
        if calls == 0 {
            // SAFETY: RunStatus is checked non-null.
            unsafe { *RunStatus = cfe_sys::CFE_ES_RunStatus_CFE_ES_RunStatus_APP_RUN };
            true
        } else if calls == 1 {
            // SAFETY: RunStatus is checked non-null.
            unsafe { *RunStatus = cfe_sys::CFE_ES_RunStatus_CFE_ES_RunStatus_APP_EXIT };
            false
        } else {
            // SAFETY: RunStatus is checked non-null.
            unsafe { *RunStatus = cfe_sys::CFE_ES_RunStatus_CFE_ES_RunStatus_SYS_DELETE };
            false
        }
    } else {
        false
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn CFE_ES_WaitForStartupSync(TimeOutMilliseconds: u32) {
    WAIT_SYNC_MS.store(TimeOutMilliseconds, Ordering::SeqCst);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn CFE_ES_ExitApp(ExitStatus: u32) {
    EXIT_APP_STATUS.store(ExitStatus, Ordering::SeqCst);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn CFE_EVS_Register(
    _Filters: *const core::ffi::c_void,
    _NumEventFilters: u16,
    _FilterScheme: u16,
) -> cfe_sys::CFE_Status_t {
    EVS_REGISTER_CALLS.fetch_add(1, Ordering::SeqCst);
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn CFE_EVS_SendEvent(
    EventID: u16,
    EventType: cfe_sys::CFE_EVS_EventType_Enum_t,
    _Spec: *const core::ffi::c_char,
    _: ...
) -> cfe_sys::CFE_Status_t {
    LAST_EVENT_ID.store(EventID, Ordering::SeqCst);
    LAST_EVENT_TYPE.store(EventType, Ordering::SeqCst);
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn CFE_SB_CreatePipe(
    PipeIdPtr: *mut cfe_sys::CFE_SB_PipeId_t,
    _Depth: u16,
    PipeName: *const core::ffi::c_char,
) -> cfe_sys::CFE_Status_t {
    PIPE_CREATE_CALLS.fetch_add(1, Ordering::SeqCst);
    if PipeIdPtr.is_null() || PipeName.is_null() {
        cfe_sys::CFE_SB_BAD_ARGUMENT
    } else if SIMULATE_PIPE_INVALID_ID.load(Ordering::SeqCst) {
        // SAFETY: PipeIdPtr is checked non-null.
        unsafe { *PipeIdPtr = cfe_sys::CFE_SB_INVALID_PIPE };
        0
    } else {
        // SAFETY: PipeIdPtr is checked non-null.
        unsafe { *PipeIdPtr = 42 };
        0
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn CFE_SB_DeletePipe(
    _PipeId: cfe_sys::CFE_SB_PipeId_t,
) -> cfe_sys::CFE_Status_t {
    PIPE_DELETE_CALLS.fetch_add(1, Ordering::SeqCst);
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn CFE_SB_Subscribe(
    MsgId: cfe_sys::CFE_SB_MsgId_t,
    _PipeId: cfe_sys::CFE_SB_PipeId_t,
) -> cfe_sys::CFE_Status_t {
    SUBSCRIBED_MID.store(MsgId.Value, Ordering::SeqCst);
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn CFE_SB_SubscribeEx(
    MsgId: cfe_sys::CFE_SB_MsgId_t,
    _PipeId: cfe_sys::CFE_SB_PipeId_t,
    _Quality: cfe_sys::CFE_SB_Qos_t,
    MsgLim: u16,
) -> cfe_sys::CFE_Status_t {
    if SIMULATE_SUBSCRIBE_FAIL.load(Ordering::SeqCst) {
        return cfe_sys::CFE_SB_MAX_MSGS_MET;
    }
    SUBSCRIBED_MID.store(MsgId.Value, Ordering::SeqCst);
    SUBSCRIBED_LIMIT.store(MsgLim, Ordering::SeqCst);
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn CFE_SB_ReceiveBuffer(
    BufPtr: *mut *mut cfe_sys::CFE_SB_Buffer_t,
    _PipeId: cfe_sys::CFE_SB_PipeId_t,
    _TimeOut: i32,
) -> cfe_sys::CFE_Status_t {
    if BufPtr.is_null() {
        return cfe_sys::CFE_SB_BAD_ARGUMENT;
    }
    if SIMULATE_RECEIVE_ERROR.load(Ordering::SeqCst) {
        return cfe_sys::CFE_SB_TIME_OUT;
    }
    if SIMULATE_RECEIVE_NULL_BUFFER.load(Ordering::SeqCst) {
        // SAFETY: BufPtr is checked non-null.
        unsafe { *BufPtr = core::ptr::null_mut() };
        return 0;
    }
    // SAFETY: BufPtr is checked non-null and points to valid storage.
    unsafe {
        *BufPtr = core::ptr::addr_of_mut!(STATIC_BUFFER).cast::<cfe_sys::CFE_SB_Buffer_t>();
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn CFE_SB_TransmitMsg(
    MsgPtr: *const cfe_sys::CFE_MSG_Message_t,
    _IsOrigination: bool,
) -> cfe_sys::CFE_Status_t {
    TRANSMIT_MSG_CALLS.fetch_add(1, Ordering::SeqCst);
    if MsgPtr.is_null() {
        cfe_sys::CFE_SB_BAD_ARGUMENT
    } else {
        0
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn CFE_SB_GetUserData(
    MsgPtr: *mut cfe_sys::CFE_MSG_Message_t,
) -> *mut core::ffi::c_void {
    if MsgPtr.is_null() {
        core::ptr::null_mut()
    } else {
        // Offset `MsgPtr` into the buffer's payload (matching real cFE `MsgPtr + HdrSize`)
        // so Miri checks full SB buffer provenance rather than a separate static.
        // SAFETY: `MsgPtr` points into `STATIC_BUFFER` (128 bytes); `size_of::<CFE_SB_Buffer_t>()`
        // is the start of `STATIC_BUFFER.payload`.
        unsafe {
            MsgPtr
                .cast::<u8>()
                .add(core::mem::size_of::<cfe_sys::CFE_SB_Buffer_t>())
                .cast()
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn CFE_SB_GetUserDataLength(
    MsgPtr: *const cfe_sys::CFE_MSG_Message_t,
) -> usize {
    if MsgPtr.is_null() {
        0
    } else {
        DUMMY_USER_DATA.len()
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn CFE_SB_GetPipeOpts(
    _PipeId: cfe_sys::CFE_SB_PipeId_t,
    OptsPtr: *mut u8,
) -> cfe_sys::CFE_Status_t {
    if OptsPtr.is_null() {
        cfe_sys::CFE_SB_BAD_ARGUMENT
    } else {
        // SAFETY: OptsPtr is checked non-null.
        unsafe { *OptsPtr = 1 };
        0
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn CFE_SB_GetPipeName(
    PipeNameBuf: *mut core::ffi::c_char,
    PipeNameSize: usize,
    _PipeId: cfe_sys::CFE_SB_PipeId_t,
) -> cfe_sys::CFE_Status_t {
    if PipeNameBuf.is_null() || PipeNameSize < 5 {
        cfe_sys::CFE_SB_BAD_ARGUMENT
    } else {
        let name = c"TEST";
        // SAFETY: PipeNameBuf is non-null and PipeNameSize >= 5.
        unsafe {
            core::ptr::copy_nonoverlapping(name.as_ptr(), PipeNameBuf, 5);
        }
        0
    }
}

static OS_TASK_DELAY_MS: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

#[unsafe(no_mangle)]
pub unsafe extern "C" fn OS_TaskDelay(millisecond: u32) -> i32 {
    OS_TASK_DELAY_MS.store(millisecond, Ordering::SeqCst);
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn CFE_SB_AllocateMessageBuffer(
    _MsgSize: usize,
) -> *mut cfe_sys::CFE_SB_Buffer_t {
    if SIMULATE_ALLOC_FAIL.load(Ordering::SeqCst) {
        core::ptr::null_mut()
    } else {
        core::ptr::addr_of_mut!(STATIC_BUFFER).cast::<cfe_sys::CFE_SB_Buffer_t>()
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn CFE_SB_ReleaseMessageBuffer(
    BufPtr: *mut cfe_sys::CFE_SB_Buffer_t,
) -> cfe_sys::CFE_Status_t {
    RELEASE_CALLS.fetch_add(1, Ordering::SeqCst);
    if BufPtr.is_null() {
        cfe_sys::CFE_SB_BUFFER_INVALID
    } else {
        0
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn CFE_SB_TransmitBuffer(
    BufPtr: *mut cfe_sys::CFE_SB_Buffer_t,
    _IsOrigination: bool,
) -> cfe_sys::CFE_Status_t {
    TRANSMIT_BUFFER_CALLS.fetch_add(1, Ordering::SeqCst);
    if BufPtr.is_null() {
        cfe_sys::CFE_SB_BAD_ARGUMENT
    } else {
        0
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn CFE_MSG_Init(
    MsgPtr: *mut cfe_sys::CFE_MSG_Message_t,
    MsgId: cfe_sys::CFE_SB_MsgId_t,
    Size: cfe_sys::CFE_MSG_Size_t,
) -> cfe_sys::CFE_Status_t {
    if MsgPtr.is_null() {
        return cfe_sys::CFE_MSG_BAD_ARGUMENT;
    }
    LAST_INIT_SIZE.store(Size, Ordering::SeqCst);
    // Mirror real cFE `memset(MsgPtr, 0, Size)` so tests exercise full-buffer writability.
    // SAFETY: Caller guarantees `MsgPtr` points to at least `Size` writable bytes.
    unsafe {
        core::ptr::write_bytes(MsgPtr.cast::<u8>(), 0, Size);
    }
    SUBSCRIBED_MID.store(MsgId.Value, Ordering::SeqCst);
    if !(7..=65_542).contains(&Size) {
        return cfe_sys::CFE_MSG_BAD_ARGUMENT;
    }
    let len_field = u16::try_from(Size - 7).unwrap().to_be_bytes();
    // SAFETY: `Size >= 7 > size_of::<CFE_MSG_Message_t>()`.
    unsafe {
        (*MsgPtr).CCSDS.Pri.Length = len_field;
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn CFE_MSG_GetSize(
    MsgPtr: *const cfe_sys::CFE_MSG_Message_t,
    Size: *mut cfe_sys::CFE_MSG_Size_t,
) -> cfe_sys::CFE_Status_t {
    if MsgPtr.is_null() || Size.is_null() {
        cfe_sys::CFE_MSG_BAD_ARGUMENT
    } else {
        // SAFETY: MsgPtr and Size are checked non-null.
        let raw_len = unsafe { u16::from_be_bytes((*MsgPtr).CCSDS.Pri.Length) };
        let decoded = if raw_len == 0 {
            64
        } else {
            usize::from(raw_len) + 7
        };
        // SAFETY: Size is checked non-null.
        unsafe { *Size = decoded };
        0
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn CFE_MSG_SetSize(
    MsgPtr: *mut cfe_sys::CFE_MSG_Message_t,
    Size: cfe_sys::CFE_MSG_Size_t,
) -> cfe_sys::CFE_Status_t {
    if MsgPtr.is_null() || !(7..=65_542).contains(&Size) {
        cfe_sys::CFE_MSG_BAD_ARGUMENT
    } else {
        let len_field = u16::try_from(Size - 7).unwrap().to_be_bytes();
        // SAFETY: MsgPtr is checked non-null.
        unsafe {
            (*MsgPtr).CCSDS.Pri.Length = len_field;
        }
        0
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn CFE_MSG_GetMsgId(
    MsgPtr: *const cfe_sys::CFE_MSG_Message_t,
    MsgId: *mut cfe_sys::CFE_SB_MsgId_t,
) -> cfe_sys::CFE_Status_t {
    if MsgPtr.is_null() || MsgId.is_null() {
        cfe_sys::CFE_MSG_BAD_ARGUMENT
    } else {
        // SAFETY: MsgId is checked non-null.
        unsafe { (*MsgId).Value = 0x1901 };
        0
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn CFE_MSG_SetMsgId(
    MsgPtr: *mut cfe_sys::CFE_MSG_Message_t,
    MsgId: cfe_sys::CFE_SB_MsgId_t,
) -> cfe_sys::CFE_Status_t {
    if MsgPtr.is_null() {
        cfe_sys::CFE_MSG_BAD_ARGUMENT
    } else {
        SUBSCRIBED_MID.store(MsgId.Value, Ordering::SeqCst);
        0
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn OS_SocketOpen(
    sock_id: *mut cfe_sys::osal_id_t,
    _Domain: cfe_sys::OS_SocketDomain_t,
    _Type: cfe_sys::OS_SocketType_t,
) -> i32 {
    if SIMULATE_SOCKET_FAIL.load(Ordering::SeqCst) {
        return -1;
    }
    OS_SOCKET_OPEN_CALLS.fetch_add(1, Ordering::SeqCst);
    if !sock_id.is_null() {
        // SAFETY: sock_id is checked non-null.
        unsafe { *sock_id = 99 };
        0
    } else {
        -1
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn OS_SocketAddrInit(
    _Addr: *mut cfe_sys::OS_SockAddr_t,
    _Domain: cfe_sys::OS_SocketDomain_t,
) -> i32 {
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn OS_SocketAddrFromString(
    Addr: *mut cfe_sys::OS_SockAddr_t,
    string: *const core::ffi::c_char,
) -> i32 {
    if !string.is_null() {
        // SAFETY: string is checked non-null and NUL-terminated by caller.
        let bytes = unsafe { std::ffi::CStr::from_ptr(string) }.to_bytes();
        let len = bytes.len().min(64);
        if let Ok(mut guard) = LAST_UDP_HOST.lock() {
            guard.0[..len].copy_from_slice(&bytes[..len]);
            guard.1 = len;
        }
    }
    let _ = Addr;
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn OS_SocketAddrSetPort(
    _Addr: *mut cfe_sys::OS_SockAddr_t,
    PortNum: u16,
) -> i32 {
    LAST_UDP_PORT.store(PortNum, Ordering::SeqCst);
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn OS_SocketBind(
    _sock_id: cfe_sys::osal_id_t,
    _Addr: *const cfe_sys::OS_SockAddr_t,
) -> i32 {
    OS_SOCKET_BIND_CALLS.fetch_add(1, Ordering::SeqCst);
    if SIMULATE_BIND_FAIL.load(Ordering::SeqCst) {
        cfe_sys::OS_ERROR
    } else {
        0
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn OS_SocketRecvFrom(
    _sock_id: cfe_sys::osal_id_t,
    buffer: *mut core::ffi::c_void,
    buflen: usize,
    _RemoteAddr: *mut cfe_sys::OS_SockAddr_t,
    _timeout: i32,
) -> i32 {
    OS_SOCKET_RECVFROM_CALLS.fetch_add(1, Ordering::SeqCst);
    if buffer.is_null() || buflen == 0 {
        return cfe_sys::OS_ERR_INVALID_SIZE;
    }
    match UDP_RECVFROM_MODE.load(Ordering::SeqCst) {
        1 => cfe_sys::OS_ERROR_TIMEOUT,
        2 => cfe_sys::OS_QUEUE_EMPTY,
        3 => cfe_sys::OS_ERROR,
        _ => {
            let n = buflen.min(3);
            let dest = buffer.cast::<u8>();
            for i in 0..n {
                let val = 0xA0_u8.saturating_add(u8::try_from(i).unwrap_or(0));
                // SAFETY: buffer is non-null and holds at least buflen >= n bytes.
                unsafe { *dest.add(i) = val };
            }
            i32::try_from(n).unwrap_or(0)
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn OS_SocketSendTo(
    _sock_id: cfe_sys::osal_id_t,
    buffer: *const core::ffi::c_void,
    buflen: usize,
    RemoteAddr: *const cfe_sys::OS_SockAddr_t,
) -> i32 {
    OS_SOCKET_SENDTO_CALLS.fetch_add(1, Ordering::SeqCst);
    if buffer.is_null() || RemoteAddr.is_null() {
        return cfe_sys::OS_INVALID_POINTER;
    }
    if UDP_SENDTO_FAIL.load(Ordering::SeqCst) {
        return cfe_sys::OS_ERROR;
    }
    i32::try_from(buflen).unwrap_or(0)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn OS_GetLocalTime(time_struct: *mut cfe_sys::OS_time_t) -> i32 {
    if time_struct.is_null() {
        return cfe_sys::OS_INVALID_POINTER;
    }
    if SIMULATE_TIME_FAIL.load(Ordering::SeqCst) {
        return cfe_sys::OS_ERROR;
    }
    // SAFETY: time_struct is checked non-null.
    unsafe {
        (*time_struct).ticks = LOCAL_TIME_TICKS.load(Ordering::SeqCst);
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn OS_SocketConnect(
    _sock_id: cfe_sys::osal_id_t,
    _Addr: *const cfe_sys::OS_SockAddr_t,
    _timeout: i32,
) -> i32 {
    OS_SOCKET_CONNECT_CALLS.fetch_add(1, Ordering::SeqCst);
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn OS_read(
    _filedes: cfe_sys::osal_id_t,
    buffer: *mut core::ffi::c_void,
    nbytes: usize,
) -> i32 {
    OS_SOCKET_RECV_CALLS.fetch_add(1, Ordering::SeqCst);
    if buffer.is_null() {
        return -1;
    }
    match RECV_CHUNK_MODE.load(Ordering::SeqCst) {
        1 => {
            // Return 2 bytes first time, then 2 bytes next time
            let n = nbytes.min(2);
            let dest = buffer.cast::<u8>();
            for i in 0..n {
                let byte_val = u8::try_from(i.saturating_add(1)).unwrap_or(0);
                // SAFETY: buffer is non-null and holds at least nbytes; i < n <= nbytes.
                unsafe { *dest.add(i) = byte_val };
            }
            i32::try_from(n).unwrap_or(0)
        }
        2 => {
            // EOF disconnect
            0
        }
        _ => {
            if nbytes >= 4 {
                let dest = buffer.cast::<u8>();
                // SAFETY: buffer is non-null and holds at least nbytes >= 4.
                unsafe {
                    *dest = 1;
                    *dest.add(1) = 2;
                    *dest.add(2) = 3;
                    *dest.add(3) = 4;
                }
                4
            } else {
                -1
            }
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn OS_TimedRead(
    filedes: cfe_sys::osal_id_t,
    buffer: *mut core::ffi::c_void,
    nbytes: usize,
    timeout: i32,
) -> i32 {
    OS_TIMED_READ_CALLS.fetch_add(1, Ordering::SeqCst);
    LAST_TIMED_READ_TIMEOUT.store(timeout, Ordering::SeqCst);
    // SAFETY: caller guarantees filedes and buffer are valid for OS_read.
    unsafe { OS_read(filedes, buffer, nbytes) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn OS_write(
    _filedes: cfe_sys::osal_id_t,
    _buffer: *const core::ffi::c_void,
    nbytes: usize,
) -> i32 {
    OS_SOCKET_SEND_CALLS.fetch_add(1, Ordering::SeqCst);
    match SEND_CHUNK_MODE.load(Ordering::SeqCst) {
        1 => {
            // Send half (at least 1 byte if non-empty)
            i32::try_from(nbytes.min(2)).unwrap_or(0)
        }
        2 => {
            // Disconnect
            0
        }
        _ => i32::try_from(nbytes).unwrap_or(0),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn OS_TimedWrite(
    filedes: cfe_sys::osal_id_t,
    buffer: *const core::ffi::c_void,
    nbytes: usize,
    _timeout: i32,
) -> i32 {
    // SAFETY: caller guarantees filedes and buffer are valid for OS_write.
    unsafe { OS_write(filedes, buffer, nbytes) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn OS_close(_filedes: cfe_sys::osal_id_t) -> i32 {
    OS_CLOSE_CALLS.fetch_add(1, Ordering::SeqCst);
    0
}

#[test]
fn test_status_methods() {
    let s = Status::SUCCESS;
    assert!(s.is_success());
    assert!(!s.is_error());
    assert_eq!(s.as_raw(), 0);
    assert!(s.result().is_ok());

    let err = Status::from_raw(-42);
    assert!(!err.is_success());
    assert!(err.is_error());
    assert_eq!(err.as_raw(), -42);
    assert_eq!(err.result(), Err(err));

    // Test PartialOrd, Ord, Hash
    let mut set = HashSet::new();
    set.insert(s);
    set.insert(err);
    assert_eq!(set.len(), 2);
    assert!(s > err);
}

#[test]
fn test_es_wrappers() {
    RUN_LOOP_CALLS.store(0, Ordering::SeqCst);
    let mut status = RunStatus::AppRun;
    assert!(es::run_loop(&mut status));
    assert_eq!(status, RunStatus::AppRun);

    assert!(!es::run_loop(&mut status));
    assert_eq!(status, RunStatus::AppExit);

    // Third call returns false with SYS_DELETE
    assert!(!es::run_loop(&mut status));
    assert_eq!(status, RunStatus::SysDelete);

    es::wait_for_startup_sync(500);
    assert_eq!(WAIT_SYNC_MS.load(Ordering::SeqCst), 500);

    es::exit_app(RunStatus::SysDelete);
    assert_eq!(
        EXIT_APP_STATUS.load(Ordering::SeqCst),
        RunStatus::SysDelete.to_raw()
    );

    es::exit_app(RunStatus::CoreAppRuntimeError);
    assert_eq!(
        EXIT_APP_STATUS.load(Ordering::SeqCst),
        RunStatus::CoreAppRuntimeError.to_raw()
    );

    // Test all RunStatus enum variants and from_raw / to_raw round-trip
    let all_statuses = [
        RunStatus::Undefined,
        RunStatus::AppRun,
        RunStatus::AppExit,
        RunStatus::AppError,
        RunStatus::SysException,
        RunStatus::SysRestart,
        RunStatus::SysReload,
        RunStatus::SysDelete,
        RunStatus::CoreAppInitError,
        RunStatus::CoreAppRuntimeError,
    ];
    let mut status_set = HashSet::new();
    for st in all_statuses {
        assert_eq!(RunStatus::from_raw(st.to_raw()), Some(st));
        status_set.insert(st);
    }
    assert_eq!(status_set.len(), 10);
    assert_eq!(RunStatus::from_raw(99), None);
}

#[test]
fn test_evs_wrappers() {
    assert!(evs::register().is_ok());
    assert_eq!(EVS_REGISTER_CALLS.load(Ordering::SeqCst), 1);

    assert!(evs::send_event(101, EventType::Information, "hello world").is_ok());
    assert_eq!(LAST_EVENT_ID.load(Ordering::SeqCst), 101);
    assert_eq!(
        LAST_EVENT_TYPE.load(Ordering::SeqCst),
        EventType::Information.to_raw()
    );

    // Message exceeding MAX_EVENT_MESSAGE_LEN bytes returns CFE_EVS_INVALID_PARAMETER error
    let long_msg = "A".repeat(evs::MAX_EVENT_MESSAGE_LEN + 1);
    let err = evs::send_event(102, EventType::Error, &long_msg);
    assert_eq!(
        err,
        Err(Status::from_raw(cfe_sys::CFE_EVS_INVALID_PARAMETER))
    );

    // Exactly MAX_EVENT_MESSAGE_LEN bytes succeeds
    let max_msg = "B".repeat(evs::MAX_EVENT_MESSAGE_LEN);
    assert!(evs::send_event(103, EventType::Information, &max_msg).is_ok());

    // Interior NUL byte is rejected with CFE_EVS_INVALID_PARAMETER
    assert_eq!(
        evs::send_event(104, EventType::Error, "bad\0msg"),
        Err(Status::from_raw(cfe_sys::CFE_EVS_INVALID_PARAMETER))
    );
}

#[test]
fn test_sb_wrappers() {
    // Pipe creation rejects empty, interior-NUL, and overlong names
    assert_eq!(
        Pipe::create(8, "").unwrap_err(),
        Status::from_raw(cfe_sys::CFE_SB_BAD_ARGUMENT)
    );
    assert_eq!(
        Pipe::create(8, "BAD\0PIPE").unwrap_err(),
        Status::from_raw(cfe_sys::CFE_SB_BAD_ARGUMENT)
    );
    let too_long_name = "THIS_PIPE_NAME_IS_TOO_LONG_FOR_CFS";
    let bad_pipe = Pipe::create(8, too_long_name);
    assert_eq!(
        bad_pipe.unwrap_err(),
        Status::from_raw(cfe_sys::CFE_SB_BAD_ARGUMENT)
    );

    let mut pipe = Pipe::create(8, "TEST_PIPE").unwrap();
    assert_eq!(pipe.id().to_raw(), 42);

    // PipeId ordering and Hash
    let p1 = PipeId::from_raw(1);
    let p2 = PipeId::from_raw(2);
    assert!(p1 < p2);
    let mut pipe_set = HashSet::new();
    pipe_set.insert(p1);
    pipe_set.insert(p2);
    assert_eq!(pipe_set.len(), 2);

    let mid = MsgId::from_u32(0x1901);
    assert_eq!(mid.as_u32(), 0x1901);
    assert!(pipe.subscribe(mid).is_ok());
    assert_eq!(SUBSCRIBED_MID.load(Ordering::SeqCst), 0x1901);

    let qos = Qos {
        priority: 1,
        reliability: 1,
    };
    let mut qos_set = HashSet::new();
    qos_set.insert(qos);
    assert_eq!(qos_set.len(), 1);

    assert!(pipe.subscribe_ex(mid, qos, 16).is_ok());
    assert_eq!(SUBSCRIBED_LIMIT.load(Ordering::SeqCst), 16);

    // Successful receive_buffer
    {
        let recvd = pipe.receive_buffer(Timeout::PendForever).unwrap();
        assert!(!recvd.as_ptr().is_null());
        let _ = recvd.msg();
        assert_eq!(recvd.user_data(), &DUMMY_USER_DATA);
        assert_eq!(recvd.msg_id().unwrap().as_u32(), 0x1901);
        let bytes = recvd.as_bytes();
        assert_eq!(bytes.len(), 64);
    }

    // Pipe opts and name
    assert_eq!(pipe.opts().unwrap(), 1);
    let mut name_buf = [0u8; sb::OS_MAX_API_NAME];
    assert_eq!(pipe.name(&mut name_buf).unwrap(), "TEST");

    // receive_buffer returning null pointer maps to CFE_SB_PIPE_RD_ERR
    SIMULATE_RECEIVE_NULL_BUFFER.store(true, Ordering::SeqCst);
    let err_null = pipe.receive_buffer(Timeout::PendForever);
    assert_eq!(
        err_null.unwrap_err(),
        Status::from_raw(cfe_sys::CFE_SB_PIPE_RD_ERR)
    );
    SIMULATE_RECEIVE_NULL_BUFFER.store(false, Ordering::SeqCst);

    // receive_buffer returning timeout status
    SIMULATE_RECEIVE_ERROR.store(true, Ordering::SeqCst);
    let err_timeout = pipe.receive_buffer(Timeout::PendForever);
    assert_eq!(
        err_timeout.unwrap_err(),
        Status::from_raw(cfe_sys::CFE_SB_TIME_OUT)
    );
    SIMULATE_RECEIVE_ERROR.store(false, Ordering::SeqCst);

    // transmit_msg takes a typed CfeMessage and enforces encoded size <= size_of::<M>()
    let mut cmd_hdr = cfe_sys::CFE_MSG_CommandHeader_t {
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
    assert!(msg::init(&mut cmd_hdr, mid).is_ok());
    assert!(sb::transmit_msg(&cmd_hdr, true).is_ok());

    // Oversized header in transmit_msg is rejected before calling CFE_SB_TransmitMsg
    assert!(msg::set_size(&mut cmd_hdr.Msg, 64).is_ok());
    let before_tx = TRANSMIT_MSG_CALLS.load(Ordering::SeqCst);
    assert_eq!(
        sb::transmit_msg(&cmd_hdr, true),
        Err(Status::from_raw(cfe_sys::CFE_SB_BAD_ARGUMENT))
    );
    assert_eq!(TRANSMIT_MSG_CALLS.load(Ordering::SeqCst), before_tx);

    // transmit_bytes rejects short buffers and buffers shorter than the encoded header size
    let short_buf = [0u8; 5];
    assert_eq!(
        sb::transmit_bytes(&short_buf, true),
        Err(Status::from_raw(cfe_sys::CFE_SB_BAD_ARGUMENT))
    );
    let six_byte_buf = [0u8; 6];
    assert_eq!(
        sb::transmit_bytes(&six_byte_buf, true),
        Err(Status::from_raw(cfe_sys::CFE_SB_BAD_ARGUMENT))
    );
    let mut wire = [0u8; 16];
    assert!(msg::init_bytes(&mut wire, mid).is_ok());
    assert!(sb::transmit_bytes(&wire, true).is_ok());
    assert_eq!(
        sb::transmit_bytes(&wire[..8], true),
        Err(Status::from_raw(cfe_sys::CFE_SB_BAD_ARGUMENT))
    );

    // MessageBuffer allocation, inspection, and transmit_buffer
    let mut alloc_buf = sb::allocate_message_buffer(128).unwrap();
    assert_eq!(alloc_buf.size(), 128);
    let _ = alloc_buf.msg();
    let _ = alloc_buf.msg_mut();
    assert_eq!(alloc_buf.as_bytes().len(), 128);
    assert_eq!(alloc_buf.as_bytes_mut().len(), 128);
    assert!(sb::transmit_buffer(alloc_buf, false).is_ok());

    // MessageBuffer explicit release
    let alloc_buf2 = sb::allocate_message_buffer(64).unwrap();
    assert!(alloc_buf2.release().is_ok());

    // MessageBuffer release_message_buffer wrapper
    let alloc_buf3 = sb::allocate_message_buffer(64).unwrap();
    assert!(sb::release_message_buffer(alloc_buf3).is_ok());

    // MessageBuffer Drop releases automatically if not transmitted
    let before_drops = RELEASE_CALLS.load(Ordering::SeqCst);
    {
        let _untransmitted = sb::allocate_message_buffer(64).unwrap();
    }
    assert_eq!(RELEASE_CALLS.load(Ordering::SeqCst), before_drops + 1);

    // MessageBuffer size validation (< size_of::<CFE_MSG_Message_t>() returns CFE_SB_BAD_ARGUMENT)
    let too_small =
        sb::allocate_message_buffer(core::mem::size_of::<cfe_sys::CFE_MSG_Message_t>() - 1);
    assert_eq!(
        too_small.unwrap_err(),
        Status::from_raw(cfe_sys::CFE_SB_BAD_ARGUMENT)
    );

    // Allocation failure returns CFE_SB_BUF_ALOC_ERR
    SIMULATE_ALLOC_FAIL.store(true, Ordering::SeqCst);
    let alloc_err = sb::allocate_message_buffer(64);
    assert_eq!(
        alloc_err.unwrap_err(),
        Status::from_raw(cfe_sys::CFE_SB_BUF_ALOC_ERR)
    );
    SIMULATE_ALLOC_FAIL.store(false, Ordering::SeqCst);

    assert!(sb::check_pipe_depth(4, MsgId::from_u32(0x190F)).is_ok());

    // Explicit delete calls CFE_SB_DeletePipe exactly once (Drop does not double-delete)
    let before_explicit = PIPE_DELETE_CALLS.load(Ordering::SeqCst);
    assert!(pipe.delete().is_ok());
    assert_eq!(
        PIPE_DELETE_CALLS.load(Ordering::SeqCst),
        before_explicit + 1
    );

    // Dropping a Pipe without calling delete() automatically calls CFE_SB_DeletePipe once
    let before_drop = PIPE_DELETE_CALLS.load(Ordering::SeqCst);
    {
        let max_len_name = "P".repeat(sb::OS_MAX_API_NAME - 1);
        let _auto_pipe = Pipe::create(4, &max_len_name).unwrap();
    }
    assert_eq!(PIPE_DELETE_CALLS.load(Ordering::SeqCst), before_drop + 1);

    // Pipe::create rejects CFE_SUCCESS paired with CFE_SB_INVALID_PIPE
    SIMULATE_PIPE_INVALID_ID.store(true, Ordering::SeqCst);
    assert_eq!(
        Pipe::create(4, "BAD_ID_PIPE").unwrap_err(),
        Status::from_raw(cfe_sys::CFE_SB_BAD_ARGUMENT)
    );
    SIMULATE_PIPE_INVALID_ID.store(false, Ordering::SeqCst);

    // check_pipe_depth cleans up its temporary pipe via Drop when subscribe_ex fails
    let before_err_drop = PIPE_DELETE_CALLS.load(Ordering::SeqCst);
    SIMULATE_SUBSCRIBE_FAIL.store(true, Ordering::SeqCst);
    assert_eq!(
        sb::check_pipe_depth(4, MsgId::from_u32(0x190F)).unwrap_err(),
        Status::from_raw(cfe_sys::CFE_SB_MAX_MSGS_MET)
    );
    SIMULATE_SUBSCRIBE_FAIL.store(false, Ordering::SeqCst);
    assert_eq!(
        PIPE_DELETE_CALLS.load(Ordering::SeqCst),
        before_err_drop + 1
    );
}

#[test]
fn test_msg_wrappers() {
    let mut cmd_hdr = cfe_sys::CFE_MSG_CommandHeader_t {
        Msg: cfe_sys::CFE_MSG_Message_t {
            CCSDS: cfe_sys::CCSDS_SpacePacket_t {
                Pri: cfe_sys::CCSDS_PrimaryHeader {
                    StreamId: [0xFF, 0xFF],
                    Sequence: [0xFF, 0xFF],
                    Length: [0xFF, 0xFF],
                },
            },
        },
        Sec: cfe_sys::CFE_MSG_CommandSecondaryHeader_t {
            FunctionCode: 0xAA,
            Checksum: 0x55,
        },
    };

    let mid = MsgId::from_u32(0x1904);
    assert!(msg::init(&mut cmd_hdr, mid).is_ok());
    assert_eq!(
        LAST_INIT_SIZE.load(Ordering::SeqCst),
        core::mem::size_of::<cfe_sys::CFE_MSG_CommandHeader_t>()
    );
    assert_eq!(cmd_hdr.Sec.FunctionCode, 0);
    assert_eq!(cmd_hdr.Sec.Checksum, 0);

    let size = msg::get_size(msg::CfeMessage::as_header(&cmd_hdr)).unwrap();
    assert_eq!(size, 8);

    let mut tlm_hdr = cfe_sys::CFE_MSG_TelemetryHeader_t {
        Msg: cmd_hdr.Msg,
        Sec: cfe_sys::CFE_MSG_TelemetrySecondaryHeader_t { Time: [0xFF; 6] },
        Spare: [0xFF; 4],
    };
    assert!(msg::init(&mut tlm_hdr, mid).is_ok());
    assert_eq!(
        LAST_INIT_SIZE.load(Ordering::SeqCst),
        core::mem::size_of::<cfe_sys::CFE_MSG_TelemetryHeader_t>()
    );
    assert_eq!(tlm_hdr.Sec.Time, [0; 6]);
    assert_eq!(tlm_hdr.Spare, [0; 4]);

    // init_bytes rejects buffers shorter than the minimum CCSDS packet length (7 bytes) without mutation
    let mut short = [0xAAu8; 5];
    assert_eq!(
        msg::init_bytes(&mut short, mid),
        Err(Status::from_raw(cfe_sys::CFE_MSG_BAD_ARGUMENT))
    );
    assert_eq!(short, [0xAA; 5]);

    let mut six = [0xCCu8; 6];
    assert_eq!(
        msg::init_bytes(&mut six, mid),
        Err(Status::from_raw(cfe_sys::CFE_MSG_BAD_ARGUMENT))
    );
    assert_eq!(six, [0xCC; 6]);

    let mut wire = [0xBBu8; 24];
    assert!(msg::init_bytes(&mut wire, mid).is_ok());
    assert_eq!(LAST_INIT_SIZE.load(Ordering::SeqCst), 24);
    assert_eq!(&wire[6..], &[0u8; 18]);

    assert!(msg::set_size(&mut cmd_hdr.Msg, 48).is_ok());
    assert_eq!(msg::get_size(&cmd_hdr.Msg).unwrap(), 48);

    let got_mid = msg::get_msg_id(&cmd_hdr.Msg).unwrap();
    assert_eq!(got_mid.as_u32(), 0x1901);

    assert!(msg::set_msg_id(&mut cmd_hdr.Msg, MsgId::from_u32(0x1902)).is_ok());
    assert_eq!(SUBSCRIBED_MID.load(Ordering::SeqCst), 0x1902);
}

#[test]
fn test_osal_tcp_stream() {
    let _guard = SOCKET_TEST_LOCK.lock().unwrap();
    OS_SOCKET_OPEN_CALLS.store(0, Ordering::SeqCst);
    OS_SOCKET_CONNECT_CALLS.store(0, Ordering::SeqCst);
    OS_SOCKET_SEND_CALLS.store(0, Ordering::SeqCst);
    OS_SOCKET_RECV_CALLS.store(0, Ordering::SeqCst);
    OS_TIMED_READ_CALLS.store(0, Ordering::SeqCst);
    LAST_TIMED_READ_TIMEOUT.store(i32::MIN, Ordering::SeqCst);
    OS_CLOSE_CALLS.store(0, Ordering::SeqCst);

    let mut stream = TcpStream::connect("127.0.0.1", 1234).unwrap();
    assert_eq!(OS_SOCKET_OPEN_CALLS.load(Ordering::SeqCst), 1);
    assert_eq!(OS_SOCKET_CONNECT_CALLS.load(Ordering::SeqCst), 1);

    let send_bytes = [10, 20, 30];
    let sent = stream.send(&send_bytes).unwrap();
    assert_eq!(sent, 3);
    assert_eq!(OS_SOCKET_SEND_CALLS.load(Ordering::SeqCst), 1);

    let mut recv_bytes = [0u8; 8];
    let recvd = stream
        .recv(&mut recv_bytes, osal::Timeout::Millis(1000))
        .unwrap();
    assert_eq!(recvd, 4);
    assert_eq!(&recv_bytes[..4], &[1, 2, 3, 4]);
    assert_eq!(OS_TIMED_READ_CALLS.load(Ordering::SeqCst), 1);
    assert_eq!(LAST_TIMED_READ_TIMEOUT.load(Ordering::SeqCst), 1000);
    assert_eq!(OS_SOCKET_RECV_CALLS.load(Ordering::SeqCst), 1);
    assert_eq!(
        stream.recv(&mut recv_bytes, osal::Timeout::Poll).unwrap(),
        4
    );
    assert_eq!(OS_TIMED_READ_CALLS.load(Ordering::SeqCst), 2);
    assert_eq!(
        LAST_TIMED_READ_TIMEOUT.load(Ordering::SeqCst),
        cfe_sys::OS_CHECK.cast_signed()
    );
    assert_eq!(OS_SOCKET_RECV_CALLS.load(Ordering::SeqCst), 2);
    assert_eq!(
        stream
            .recv(&mut recv_bytes, osal::Timeout::PendForever)
            .unwrap(),
        4
    );
    assert_eq!(OS_TIMED_READ_CALLS.load(Ordering::SeqCst), 2);
    assert_eq!(OS_SOCKET_RECV_CALLS.load(Ordering::SeqCst), 3);

    // Test read_exact with chunked recv
    RECV_CHUNK_MODE.store(1, Ordering::SeqCst);
    let mut exact_buf = [0u8; 4];
    assert!(stream.read_exact(&mut exact_buf).is_ok());
    assert_eq!(&exact_buf, &[1, 2, 1, 2]);
    RECV_CHUNK_MODE.store(0, Ordering::SeqCst);

    // Test read_exact with EOF disconnect
    RECV_CHUNK_MODE.store(2, Ordering::SeqCst);
    assert_eq!(
        stream.read_exact(&mut exact_buf).unwrap_err(),
        Status::from_raw(cfe_sys::OS_ERR_STREAM_DISCONNECTED)
    );
    RECV_CHUNK_MODE.store(0, Ordering::SeqCst);

    // Test write_all with chunked send
    SEND_CHUNK_MODE.store(1, Ordering::SeqCst);
    assert!(stream.write_all(&[1, 2, 3, 4]).is_ok());
    SEND_CHUNK_MODE.store(0, Ordering::SeqCst);

    // Test write_all with disconnect
    SEND_CHUNK_MODE.store(2, Ordering::SeqCst);
    assert_eq!(
        stream.write_all(&[1, 2, 3, 4]).unwrap_err(),
        Status::from_raw(cfe_sys::OS_ERR_STREAM_DISCONNECTED)
    );
    SEND_CHUNK_MODE.store(0, Ordering::SeqCst);

    // Test task_delay
    assert!(crate::osal::task_delay(25).is_ok());
    assert_eq!(OS_TASK_DELAY_MS.load(Ordering::SeqCst), 25);

    assert!(stream.close().is_ok());
    assert_eq!(OS_CLOSE_CALLS.load(Ordering::SeqCst), 1);

    // Test socket failure path
    SIMULATE_SOCKET_FAIL.store(true, Ordering::SeqCst);
    assert!(TcpStream::connect("127.0.0.1", 1234).is_err());
    SIMULATE_SOCKET_FAIL.store(false, Ordering::SeqCst);
}

#[test]
fn test_osal_udp_socket_and_local_time() {
    let _guard = SOCKET_TEST_LOCK.lock().unwrap();
    OS_SOCKET_OPEN_CALLS.store(0, Ordering::SeqCst);
    OS_SOCKET_BIND_CALLS.store(0, Ordering::SeqCst);
    OS_SOCKET_SENDTO_CALLS.store(0, Ordering::SeqCst);
    OS_SOCKET_RECVFROM_CALLS.store(0, Ordering::SeqCst);
    OS_CLOSE_CALLS.store(0, Ordering::SeqCst);
    SIMULATE_SOCKET_FAIL.store(false, Ordering::SeqCst);
    SIMULATE_BIND_FAIL.store(false, Ordering::SeqCst);
    UDP_RECVFROM_MODE.store(0, Ordering::SeqCst);
    UDP_SENDTO_FAIL.store(false, Ordering::SeqCst);

    {
        let sock = UdpSocket::bind("0.0.0.0", 1234).unwrap();
        assert_eq!(OS_SOCKET_OPEN_CALLS.load(Ordering::SeqCst), 1);
        assert_eq!(OS_SOCKET_BIND_CALLS.load(Ordering::SeqCst), 1);
        assert_eq!(LAST_UDP_PORT.load(Ordering::SeqCst), 1234);
        {
            let guard = LAST_UDP_HOST.lock().unwrap();
            assert_eq!(&guard.0[..guard.1], b"0.0.0.0");
        }

        // osal::Timeout conversion
        assert_eq!(
            osal::Timeout::Poll.to_raw(),
            cfe_sys::OS_CHECK.cast_signed()
        );
        assert_eq!(osal::Timeout::PendForever.to_raw(), cfe_sys::OS_PEND);
        assert_eq!(
            osal::Timeout::Millis(0).to_raw(),
            cfe_sys::OS_CHECK.cast_signed()
        );
        assert_eq!(osal::Timeout::Millis(250).to_raw(), 250);
        assert_eq!(osal::Timeout::Millis(u32::MAX).to_raw(), i32::MAX);

        // Receive datagram
        let mut buf = [0u8; 16];
        assert_eq!(sock.recv_from(&mut buf, osal::Timeout::Poll), Ok(Some(3)));
        assert_eq!(&buf[..3], &[0xA0, 0xA1, 0xA2]);

        // Non-blocking OS_CHECK timeout / empty queue returns Ok(None)
        UDP_RECVFROM_MODE.store(1, Ordering::SeqCst);
        assert_eq!(sock.recv_from(&mut buf, osal::Timeout::Poll), Ok(None));
        UDP_RECVFROM_MODE.store(2, Ordering::SeqCst);
        assert_eq!(sock.recv_from(&mut buf, osal::Timeout::Poll), Ok(None));

        // Hard recv error returns Err(Status)
        UDP_RECVFROM_MODE.store(3, Ordering::SeqCst);
        assert_eq!(
            sock.recv_from(&mut buf, osal::Timeout::Poll),
            Err(Status::from_raw(cfe_sys::OS_ERROR))
        );
        UDP_RECVFROM_MODE.store(0, Ordering::SeqCst);

        // send_to and send_to_ipv4
        assert_eq!(sock.send_to(&[1, 2, 3, 4], "127.0.0.1", 2234), Ok(4));
        assert_eq!(LAST_UDP_PORT.load(Ordering::SeqCst), 2234);
        {
            let guard = LAST_UDP_HOST.lock().unwrap();
            assert_eq!(&guard.0[..guard.1], b"127.0.0.1");
        }

        assert_eq!(
            sock.send_to_ipv4(&[5, 6, 7], Ipv4Addr::new(192, 168, 10, 42), 3234),
            Ok(3)
        );
        assert_eq!(LAST_UDP_PORT.load(Ordering::SeqCst), 3234);
        {
            let guard = LAST_UDP_HOST.lock().unwrap();
            assert_eq!(&guard.0[..guard.1], b"192.168.10.42");
        }
        assert_eq!(OS_SOCKET_SENDTO_CALLS.load(Ordering::SeqCst), 2);

        // send_to failure
        UDP_SENDTO_FAIL.store(true, Ordering::SeqCst);
        assert_eq!(
            sock.send_to_ipv4(&[1], Ipv4Addr::LOCALHOST, 2234),
            Err(Status::from_raw(cfe_sys::OS_ERROR))
        );
        UDP_SENDTO_FAIL.store(false, Ordering::SeqCst);
    }
    // Drop closed the socket
    assert_eq!(OS_CLOSE_CALLS.load(Ordering::SeqCst), 1);

    // Explicit close does not double-close on Drop
    let sock2 = UdpSocket::bind("0.0.0.0", 1235).unwrap();
    assert!(sock2.close().is_ok());
    assert_eq!(OS_CLOSE_CALLS.load(Ordering::SeqCst), 2);

    // Bind failure closes opened socket
    SIMULATE_BIND_FAIL.store(true, Ordering::SeqCst);
    assert_eq!(
        UdpSocket::bind("0.0.0.0", 1234).unwrap_err(),
        Status::from_raw(cfe_sys::OS_ERROR)
    );
    assert_eq!(OS_CLOSE_CALLS.load(Ordering::SeqCst), 3);
    SIMULATE_BIND_FAIL.store(false, Ordering::SeqCst);

    // Invalid host rejects early without leaking socket
    assert_eq!(
        UdpSocket::bind("", 1234).unwrap_err(),
        Status::from_raw(cfe_sys::OS_ERR_BAD_ADDRESS)
    );
    assert_eq!(OS_CLOSE_CALLS.load(Ordering::SeqCst), 4);

    // local_time_us() success, failure, and negative ticks
    LOCAL_TIME_TICKS.store(12_345_670, Ordering::SeqCst);
    SIMULATE_TIME_FAIL.store(false, Ordering::SeqCst);
    assert_eq!(local_time_us(), Ok(1_234_567));

    LOCAL_TIME_TICKS.store(-10, Ordering::SeqCst);
    assert_eq!(local_time_us(), Err(Status::from_raw(cfe_sys::OS_ERROR)));

    // Sub-microsecond negative ticks (-9..=-1) must also return Err rather than Ok(0)
    LOCAL_TIME_TICKS.store(-1, Ordering::SeqCst);
    assert_eq!(local_time_us(), Err(Status::from_raw(cfe_sys::OS_ERROR)));

    LOCAL_TIME_TICKS.store(-9, Ordering::SeqCst);
    assert_eq!(local_time_us(), Err(Status::from_raw(cfe_sys::OS_ERROR)));

    SIMULATE_TIME_FAIL.store(true, Ordering::SeqCst);
    assert_eq!(local_time_us(), Err(Status::from_raw(cfe_sys::OS_ERROR)));
    SIMULATE_TIME_FAIL.store(false, Ordering::SeqCst);
}

static TBL_TEST_LOCK: Mutex<()> = Mutex::new(());
static TBL_REGISTER_CALLS: AtomicU32 = AtomicU32::new(0);
static TBL_UNREGISTER_CALLS: AtomicU32 = AtomicU32::new(0);
static TBL_LOAD_CALLS: AtomicU32 = AtomicU32::new(0);
static TBL_MANAGE_CALLS: AtomicU32 = AtomicU32::new(0);
static TBL_GETADDR_CALLS: AtomicU32 = AtomicU32::new(0);
static TBL_RELEASE_CALLS: AtomicU32 = AtomicU32::new(0);
static TBL_SIM_REG_FAIL: AtomicBool = AtomicBool::new(false);
static TBL_SIM_REG_STATUS: AtomicI32 = AtomicI32::new(0);
static TBL_SIM_UNREG_FAIL: AtomicBool = AtomicBool::new(false);
static TBL_SIM_LOAD_FAIL: AtomicBool = AtomicBool::new(false);
static TBL_SIM_LOAD_STATUS: AtomicI32 = AtomicI32::new(0);
static TBL_SIM_MANAGE_STATUS: AtomicI32 = AtomicI32::new(0);
static TBL_SIM_NEVER_LOADED: AtomicBool = AtomicBool::new(false);
static TBL_SIM_GETADDR_NULL: AtomicBool = AtomicBool::new(false);
static TBL_SIM_UPDATED_ON_GETADDR: AtomicBool = AtomicBool::new(false);
static TBL_SIM_STATUS: AtomicI32 = AtomicI32::new(0);
static mut TBL_BACKING_DATA: [u8; 16] = [
    0x10, 0x20, 0x30, 0x40, 0x50, 0x60, 0x70, 0x80, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08,
];

fn reset_tbl_stubs() {
    TBL_REGISTER_CALLS.store(0, Ordering::SeqCst);
    TBL_UNREGISTER_CALLS.store(0, Ordering::SeqCst);
    TBL_LOAD_CALLS.store(0, Ordering::SeqCst);
    TBL_MANAGE_CALLS.store(0, Ordering::SeqCst);
    TBL_GETADDR_CALLS.store(0, Ordering::SeqCst);
    TBL_RELEASE_CALLS.store(0, Ordering::SeqCst);
    TBL_SIM_REG_FAIL.store(false, Ordering::SeqCst);
    TBL_SIM_REG_STATUS.store(0, Ordering::SeqCst);
    TBL_SIM_UNREG_FAIL.store(false, Ordering::SeqCst);
    TBL_SIM_LOAD_FAIL.store(false, Ordering::SeqCst);
    TBL_SIM_LOAD_STATUS.store(0, Ordering::SeqCst);
    TBL_SIM_MANAGE_STATUS.store(0, Ordering::SeqCst);
    TBL_SIM_NEVER_LOADED.store(false, Ordering::SeqCst);
    TBL_SIM_GETADDR_NULL.store(false, Ordering::SeqCst);
    TBL_SIM_UPDATED_ON_GETADDR.store(false, Ordering::SeqCst);
    TBL_SIM_STATUS.store(0, Ordering::SeqCst);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn CFE_TBL_Register(
    TblHandlePtr: *mut cfe_sys::CFE_TBL_Handle_t,
    Name: *const core::ffi::c_char,
    Size: usize,
    _TblOptionFlags: u16,
    _TblValidationFuncPtr: cfe_sys::CFE_TBL_CallbackFuncPtr_t,
) -> cfe_sys::CFE_Status_t {
    TBL_REGISTER_CALLS.fetch_add(1, Ordering::SeqCst);
    if TblHandlePtr.is_null() || Name.is_null() {
        return cfe_sys::CFE_TBL_BAD_ARGUMENT;
    }
    if Size == 0 {
        // SAFETY: TblHandlePtr is checked non-null.
        unsafe { *TblHandlePtr = -1 };
        return cfe_sys::CFE_TBL_ERR_INVALID_SIZE;
    }
    if TBL_SIM_REG_FAIL.load(Ordering::SeqCst) {
        // SAFETY: TblHandlePtr is checked non-null.
        unsafe { *TblHandlePtr = -1 };
        return cfe_sys::CFE_TBL_ERR_REGISTRY_FULL;
    }
    // SAFETY: TblHandlePtr is checked non-null.
    unsafe { *TblHandlePtr = 7 };
    TBL_SIM_REG_STATUS.load(Ordering::SeqCst)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn CFE_TBL_Unregister(
    TblHandle: cfe_sys::CFE_TBL_Handle_t,
) -> cfe_sys::CFE_Status_t {
    TBL_UNREGISTER_CALLS.fetch_add(1, Ordering::SeqCst);
    if TblHandle != 7 || TBL_SIM_UNREG_FAIL.load(Ordering::SeqCst) {
        return cfe_sys::CFE_TBL_ERR_INVALID_HANDLE;
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn CFE_TBL_Load(
    TblHandle: cfe_sys::CFE_TBL_Handle_t,
    SrcType: cfe_sys::CFE_TBL_SrcEnum_t,
    SrcDataPtr: *const core::ffi::c_void,
) -> cfe_sys::CFE_Status_t {
    TBL_LOAD_CALLS.fetch_add(1, Ordering::SeqCst);
    if TblHandle != 7 {
        return cfe_sys::CFE_TBL_ERR_INVALID_HANDLE;
    }
    if SrcDataPtr.is_null() {
        return cfe_sys::CFE_TBL_BAD_ARGUMENT;
    }
    if SrcType != cfe_sys::CFE_TBL_SrcEnum_CFE_TBL_SRC_FILE {
        return cfe_sys::CFE_TBL_ERR_ILLEGAL_SRC_TYPE;
    }
    if TBL_SIM_LOAD_FAIL.load(Ordering::SeqCst) {
        return cfe_sys::CFE_TBL_ERR_ACCESS;
    }
    TBL_SIM_UPDATED_ON_GETADDR.store(true, Ordering::SeqCst);
    TBL_SIM_LOAD_STATUS.load(Ordering::SeqCst)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn CFE_TBL_Manage(
    TblHandle: cfe_sys::CFE_TBL_Handle_t,
) -> cfe_sys::CFE_Status_t {
    TBL_MANAGE_CALLS.fetch_add(1, Ordering::SeqCst);
    if TblHandle != 7 {
        return cfe_sys::CFE_TBL_ERR_INVALID_HANDLE;
    }
    TBL_SIM_MANAGE_STATUS.load(Ordering::SeqCst)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn CFE_TBL_GetStatus(
    TblHandle: cfe_sys::CFE_TBL_Handle_t,
) -> cfe_sys::CFE_Status_t {
    if TblHandle != 7 {
        return cfe_sys::CFE_TBL_ERR_INVALID_HANDLE;
    }
    TBL_SIM_STATUS.load(Ordering::SeqCst)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn CFE_TBL_GetAddress(
    TblPtr: *mut *mut core::ffi::c_void,
    TblHandle: cfe_sys::CFE_TBL_Handle_t,
) -> cfe_sys::CFE_Status_t {
    TBL_GETADDR_CALLS.fetch_add(1, Ordering::SeqCst);
    if TblPtr.is_null() {
        return cfe_sys::CFE_TBL_BAD_ARGUMENT;
    }
    if TblHandle != 7 {
        // SAFETY: TblPtr is checked non-null.
        unsafe { *TblPtr = core::ptr::null_mut() };
        return cfe_sys::CFE_TBL_ERR_INVALID_HANDLE;
    }
    if TBL_SIM_NEVER_LOADED.load(Ordering::SeqCst) {
        // SAFETY: TblPtr is checked non-null.
        unsafe { *TblPtr = core::ptr::null_mut() };
        return cfe_sys::CFE_TBL_ERR_NEVER_LOADED;
    }
    if TBL_SIM_GETADDR_NULL.load(Ordering::SeqCst) {
        // SAFETY: TblPtr is checked non-null.
        unsafe { *TblPtr = core::ptr::null_mut() };
        return 0;
    }
    // SAFETY: TblPtr is checked non-null and TBL_BACKING_DATA is valid static storage.
    unsafe { *TblPtr = core::ptr::addr_of_mut!(TBL_BACKING_DATA).cast() };
    if TBL_SIM_UPDATED_ON_GETADDR.swap(false, Ordering::SeqCst) {
        cfe_sys::CFE_TBL_INFO_UPDATED.cast_signed()
    } else {
        0
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn CFE_TBL_ReleaseAddress(
    TblHandle: cfe_sys::CFE_TBL_Handle_t,
) -> cfe_sys::CFE_Status_t {
    TBL_RELEASE_CALLS.fetch_add(1, Ordering::SeqCst);
    if TblHandle != 7 {
        return cfe_sys::CFE_TBL_ERR_INVALID_HANDLE;
    }
    0
}

#[test]
fn test_tbl_lifecycle_load_manage_read_and_drop() {
    let _guard = TBL_TEST_LOCK.lock().unwrap();
    reset_tbl_stubs();

    let size12 = NonZeroUsize::new(12).unwrap();
    let size8 = NonZeroUsize::new(8).unwrap();

    {
        let mut table = Table::register("LinkCfg", size12, TableOptions::Default).unwrap();
        assert_eq!(table.size(), size12);
        assert_eq!(table.handle(), 7);
        assert_eq!(TBL_REGISTER_CALLS.load(Ordering::SeqCst), 1);

        // Load from file
        assert!(table.load_from_file("/cf/link_lab_cfg.tbl").is_ok());
        assert_eq!(TBL_LOAD_CALLS.load(Ordering::SeqCst), 1);

        // Manage unchanged -> Ok(false)
        TBL_SIM_MANAGE_STATUS.store(0, Ordering::SeqCst);
        assert_eq!(table.manage(), Ok(false));

        // Manage updated -> Ok(true)
        TBL_SIM_MANAGE_STATUS.store(
            cfe_sys::CFE_TBL_INFO_UPDATED.cast_signed(),
            Ordering::SeqCst,
        );
        assert_eq!(table.manage(), Ok(true));

        // get_status
        TBL_SIM_STATUS.store(
            cfe_sys::CFE_TBL_INFO_UPDATE_PENDING.cast_signed(),
            Ordering::SeqCst,
        );
        assert_eq!(
            table.get_status(),
            Status::from_raw(cfe_sys::CFE_TBL_INFO_UPDATE_PENDING.cast_signed())
        );

        // First read_bytes after load gets CFE_TBL_INFO_UPDATED and still succeeds, calling ReleaseAddress
        let observed = table
            .read_bytes(|bytes| {
                assert_eq!(bytes.len(), 12);
                let mut buf = [0u8; 12];
                buf.copy_from_slice(bytes);
                buf
            })
            .unwrap();
        assert_eq!(
            observed,
            [
                0x10, 0x20, 0x30, 0x40, 0x50, 0x60, 0x70, 0x80, 0x01, 0x02, 0x03, 0x04
            ]
        );
        assert_eq!(TBL_GETADDR_CALLS.load(Ordering::SeqCst), 1);
        assert_eq!(TBL_RELEASE_CALLS.load(Ordering::SeqCst), 1);

        // Second read_bytes gets CFE_SUCCESS and still calls ReleaseAddress
        let first = table.read_bytes(|bytes| bytes[0]).unwrap();
        assert_eq!(first, 0x10);
        assert_eq!(TBL_GETADDR_CALLS.load(Ordering::SeqCst), 2);
        assert_eq!(TBL_RELEASE_CALLS.load(Ordering::SeqCst), 2);
    }
    // Dropping Table calls CFE_TBL_Unregister automatically
    assert_eq!(TBL_UNREGISTER_CALLS.load(Ordering::SeqCst), 1);

    // Explicit unregister must not double-unregister on Drop
    let table2 = Table::register("LinkCfg", size8, TableOptions::Default).unwrap();
    assert!(table2.unregister().is_ok());
    assert_eq!(TBL_UNREGISTER_CALLS.load(Ordering::SeqCst), 2);
}

#[test]
fn test_tbl_error_handling() {
    let _guard = TBL_TEST_LOCK.lock().unwrap();
    reset_tbl_stubs();

    let size8 = NonZeroUsize::new(8).unwrap();

    // Invalid names
    assert_eq!(
        Table::register("", size8, TableOptions::Default).unwrap_err(),
        Status::from_raw(cfe_sys::CFE_TBL_ERR_INVALID_NAME)
    );
    assert_eq!(
        Table::register("BAD\0NAME", size8, TableOptions::Default).unwrap_err(),
        Status::from_raw(cfe_sys::CFE_TBL_ERR_INVALID_NAME)
    );
    let long_name = "A".repeat(tbl::MAX_TABLE_NAME_LEN);
    assert_eq!(
        Table::register(&long_name, size8, TableOptions::Default).unwrap_err(),
        Status::from_raw(cfe_sys::CFE_TBL_ERR_INVALID_NAME)
    );

    // CFE_TBL_INFO_RECOVERED_TBL and CFE_TBL_WARN_NOT_CRITICAL succeed and drop unregisters them
    for (status, opts) in [
        (
            cfe_sys::CFE_TBL_INFO_RECOVERED_TBL.cast_signed(),
            TableOptions::Critical,
        ),
        (
            cfe_sys::CFE_TBL_INFO_RECOVERED_TBL.cast_signed(),
            TableOptions::DoubleBufferedCritical,
        ),
        (
            cfe_sys::CFE_TBL_WARN_NOT_CRITICAL.cast_signed(),
            TableOptions::Critical,
        ),
    ] {
        let before_unreg = TBL_UNREGISTER_CALLS.load(Ordering::SeqCst);
        TBL_SIM_REG_STATUS.store(status, Ordering::SeqCst);
        {
            let recovered = Table::register("CritCfg", size8, opts).unwrap();
            assert_eq!(recovered.handle(), 7);
            assert_eq!(TBL_UNREGISTER_CALLS.load(Ordering::SeqCst), before_unreg);
        }
        assert_eq!(
            TBL_UNREGISTER_CALLS.load(Ordering::SeqCst),
            before_unreg + 1
        );
    }
    TBL_SIM_REG_STATUS.store(0, Ordering::SeqCst);

    // CFE_TBL_WARN_DUPLICATE must return Err without owning or unregistering the aliased handle
    let before_dup_unreg = TBL_UNREGISTER_CALLS.load(Ordering::SeqCst);
    TBL_SIM_REG_STATUS.store(
        cfe_sys::CFE_TBL_WARN_DUPLICATE.cast_signed(),
        Ordering::SeqCst,
    );
    assert_eq!(
        Table::register("LinkCfg", size8, TableOptions::Default).unwrap_err(),
        Status::from_raw(cfe_sys::CFE_TBL_WARN_DUPLICATE.cast_signed())
    );
    assert_eq!(
        TBL_UNREGISTER_CALLS.load(Ordering::SeqCst),
        before_dup_unreg
    );
    TBL_SIM_REG_STATUS.store(0, Ordering::SeqCst);

    // Unexpected non-negative status with a valid handle must unregister the handle and return Err
    let before_unexpected_unreg = TBL_UNREGISTER_CALLS.load(Ordering::SeqCst);
    TBL_SIM_REG_STATUS.store(
        cfe_sys::CFE_TBL_INFO_UPDATED.cast_signed(),
        Ordering::SeqCst,
    );
    assert_eq!(
        Table::register("LinkCfg", size8, TableOptions::Default).unwrap_err(),
        Status::from_raw(cfe_sys::CFE_TBL_INFO_UPDATED.cast_signed())
    );
    assert_eq!(
        TBL_UNREGISTER_CALLS.load(Ordering::SeqCst),
        before_unexpected_unreg + 1
    );
    TBL_SIM_REG_STATUS.store(0, Ordering::SeqCst);

    // Registry full from C stub
    TBL_SIM_REG_FAIL.store(true, Ordering::SeqCst);
    assert_eq!(
        Table::register("LinkCfg", size8, TableOptions::Default).unwrap_err(),
        Status::from_raw(cfe_sys::CFE_TBL_ERR_REGISTRY_FULL)
    );
    TBL_SIM_REG_FAIL.store(false, Ordering::SeqCst);

    let mut table = Table::register("LinkCfg", size8, TableOptions::Default).unwrap();

    // Path validation in load_from_file
    assert_eq!(
        table.load_from_file("").unwrap_err(),
        Status::from_raw(cfe_sys::CFE_TBL_BAD_ARGUMENT)
    );
    assert_eq!(
        table.load_from_file("/cf/bad\0.tbl").unwrap_err(),
        Status::from_raw(cfe_sys::CFE_TBL_BAD_ARGUMENT)
    );
    let long_path = "a".repeat(tbl::MAX_PATH_LEN);
    assert_eq!(
        table.load_from_file(&long_path).unwrap_err(),
        Status::from_raw(cfe_sys::CFE_TBL_ERR_FILENAME_TOO_LONG)
    );

    // Load failure from C stub
    TBL_SIM_LOAD_FAIL.store(true, Ordering::SeqCst);
    assert_eq!(
        table.load_from_file("/cf/link_lab_cfg.tbl").unwrap_err(),
        Status::from_raw(cfe_sys::CFE_TBL_ERR_ACCESS)
    );
    TBL_SIM_LOAD_FAIL.store(false, Ordering::SeqCst);

    // Load warning statuses (truncated / partial loads) are rejected as Err
    for warn in [
        cfe_sys::CFE_TBL_WARN_SHORT_FILE.cast_signed(),
        cfe_sys::CFE_TBL_WARN_PARTIAL_LOAD.cast_signed(),
    ] {
        let before_load = TBL_LOAD_CALLS.load(Ordering::SeqCst);
        TBL_SIM_LOAD_STATUS.store(warn, Ordering::SeqCst);
        assert_eq!(
            table.load_from_file("/cf/link_lab_cfg.tbl").unwrap_err(),
            Status::from_raw(warn)
        );
        assert_eq!(TBL_LOAD_CALLS.load(Ordering::SeqCst), before_load + 1);
    }
    TBL_SIM_LOAD_STATUS.store(0, Ordering::SeqCst);

    // Manage error from C stub
    TBL_SIM_MANAGE_STATUS.store(cfe_sys::CFE_TBL_ERR_NO_ACCESS, Ordering::SeqCst);
    assert_eq!(
        table.manage().unwrap_err(),
        Status::from_raw(cfe_sys::CFE_TBL_ERR_NO_ACCESS)
    );
    TBL_SIM_MANAGE_STATUS.store(0, Ordering::SeqCst);

    // Never loaded error in read_bytes
    TBL_SIM_NEVER_LOADED.store(true, Ordering::SeqCst);
    assert_eq!(
        table
            .read_bytes(|_| panic!("should not be called"))
            .unwrap_err(),
        Status::from_raw(cfe_sys::CFE_TBL_ERR_NEVER_LOADED)
    );
    assert_eq!(TBL_RELEASE_CALLS.load(Ordering::SeqCst), 0);
    TBL_SIM_NEVER_LOADED.store(false, Ordering::SeqCst);

    // Null pointer even with status 0
    TBL_SIM_GETADDR_NULL.store(true, Ordering::SeqCst);
    assert_eq!(
        table
            .read_bytes(|_| panic!("should not be called"))
            .unwrap_err(),
        Status::from_raw(cfe_sys::CFE_TBL_ERR_UNREGISTERED)
    );
    assert_eq!(TBL_RELEASE_CALLS.load(Ordering::SeqCst), 0);
    TBL_SIM_GETADDR_NULL.store(false, Ordering::SeqCst);

    // Unregister failure
    TBL_SIM_UNREG_FAIL.store(true, Ordering::SeqCst);
    assert_eq!(
        table.unregister().unwrap_err(),
        Status::from_raw(cfe_sys::CFE_TBL_ERR_INVALID_HANDLE)
    );
    TBL_SIM_UNREG_FAIL.store(false, Ordering::SeqCst);
}
