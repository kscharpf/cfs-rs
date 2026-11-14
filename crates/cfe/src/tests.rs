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
use crate::osal::TcpStream;
use crate::sb::{self, MsgId, Pipe, PipeId, Qos, Timeout};
use crate::status::Status;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, Ordering};

static RUN_LOOP_CALLS: AtomicU32 = AtomicU32::new(0);
static WAIT_SYNC_MS: AtomicU32 = AtomicU32::new(0);
static EXIT_APP_STATUS: AtomicU32 = AtomicU32::new(0);
static EVS_REGISTER_CALLS: AtomicU32 = AtomicU32::new(0);
static LAST_EVENT_ID: AtomicU16 = AtomicU16::new(0);
static LAST_EVENT_TYPE: AtomicU16 = AtomicU16::new(0);
static PIPE_CREATE_CALLS: AtomicU32 = AtomicU32::new(0);
static SUBSCRIBED_MID: AtomicU32 = AtomicU32::new(0);
static SUBSCRIBED_LIMIT: AtomicU16 = AtomicU16::new(0);
static OS_SOCKET_OPEN_CALLS: AtomicU32 = AtomicU32::new(0);
static OS_SOCKET_CONNECT_CALLS: AtomicU32 = AtomicU32::new(0);
static OS_SOCKET_SEND_CALLS: AtomicU32 = AtomicU32::new(0);
static OS_SOCKET_RECV_CALLS: AtomicU32 = AtomicU32::new(0);
static OS_CLOSE_CALLS: AtomicU32 = AtomicU32::new(0);
static SIMULATE_SOCKET_FAIL: AtomicBool = AtomicBool::new(false);
static SIMULATE_RECEIVE_NULL_BUFFER: AtomicBool = AtomicBool::new(false);
static SIMULATE_RECEIVE_ERROR: AtomicBool = AtomicBool::new(false);
static SIMULATE_ALLOC_FAIL: AtomicBool = AtomicBool::new(false);
static RELEASE_CALLS: AtomicU32 = AtomicU32::new(0);
static TRANSMIT_BUFFER_CALLS: AtomicU32 = AtomicU32::new(0);
static TRANSMIT_MSG_CALLS: AtomicU32 = AtomicU32::new(0);
static RECV_CHUNK_MODE: AtomicU32 = AtomicU32::new(0);
static SEND_CHUNK_MODE: AtomicU32 = AtomicU32::new(0);
static DUMMY_USER_DATA: [u8; 16] = [0xAA; 16];

static mut STATIC_BUFFER: cfe_sys::CFE_SB_Buffer_t = cfe_sys::CFE_SB_Buffer_t {
    Msg: cfe_sys::CFE_MSG_Message {
        CCSDS: cfe_sys::CCSDS_SpacePacket_t {
            Pri: cfe_sys::CCSDS_PrimaryHeader {
                StreamId: [0, 0],
                Sequence: [0, 0],
                Length: [0, 0],
            },
        },
    },
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
        *BufPtr = core::ptr::addr_of_mut!(STATIC_BUFFER);
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
        DUMMY_USER_DATA.as_ptr().cast_mut().cast()
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
        core::ptr::addr_of_mut!(STATIC_BUFFER)
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
    _Size: cfe_sys::CFE_MSG_Size_t,
) -> cfe_sys::CFE_Status_t {
    if MsgPtr.is_null() {
        cfe_sys::CFE_MSG_BAD_ARGUMENT
    } else {
        SUBSCRIBED_MID.store(MsgId.Value, Ordering::SeqCst);
        0
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn CFE_MSG_GetSize(
    MsgPtr: *const cfe_sys::CFE_MSG_Message_t,
    Size: *mut cfe_sys::CFE_MSG_Size_t,
) -> cfe_sys::CFE_Status_t {
    if MsgPtr.is_null() || Size.is_null() {
        cfe_sys::CFE_MSG_BAD_ARGUMENT
    } else {
        // SAFETY: Size is checked non-null.
        unsafe { *Size = 64 };
        0
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn CFE_MSG_SetSize(
    MsgPtr: *mut cfe_sys::CFE_MSG_Message_t,
    _Size: cfe_sys::CFE_MSG_Size_t,
) -> cfe_sys::CFE_Status_t {
    if MsgPtr.is_null() {
        cfe_sys::CFE_MSG_BAD_ARGUMENT
    } else {
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
    _Addr: *mut cfe_sys::OS_SockAddr_t,
    _string: *const core::ffi::c_char,
) -> i32 {
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn OS_SocketAddrSetPort(
    _Addr: *mut cfe_sys::OS_SockAddr_t,
    _PortNum: u16,
) -> i32 {
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
    _timeout: i32,
) -> i32 {
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
}

#[test]
fn test_sb_wrappers() {
    // Pipe creation name limit check
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

    // transmit_msg takes &CFE_MSG_Message_t
    let msg_dummy = cfe_sys::CFE_MSG_Message_t {
        CCSDS: cfe_sys::CCSDS_SpacePacket_t {
            Pri: cfe_sys::CCSDS_PrimaryHeader {
                StreamId: [0, 0],
                Sequence: [0, 0],
                Length: [0, 0],
            },
        },
    };
    assert!(sb::transmit_msg(&msg_dummy, true).is_ok());

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

    assert!(pipe.delete().is_ok());
}

#[test]
fn test_msg_wrappers() {
    let mut msg_dummy = cfe_sys::CFE_MSG_Message_t {
        CCSDS: cfe_sys::CCSDS_SpacePacket_t {
            Pri: cfe_sys::CCSDS_PrimaryHeader {
                StreamId: [0, 0],
                Sequence: [0, 0],
                Length: [0, 0],
            },
        },
    };

    let mid = MsgId::from_u32(0x1904);
    assert!(msg::init(&mut msg_dummy, mid, 32).is_ok());

    let size = msg::get_size(&msg_dummy).unwrap();
    assert_eq!(size, 64);

    assert!(msg::set_size(&mut msg_dummy, 48).is_ok());

    let got_mid = msg::get_msg_id(&msg_dummy).unwrap();
    assert_eq!(got_mid.as_u32(), 0x1901);

    assert!(msg::set_msg_id(&mut msg_dummy, MsgId::from_u32(0x1902)).is_ok());
    assert_eq!(SUBSCRIBED_MID.load(Ordering::SeqCst), 0x1902);
}

#[test]
fn test_osal_tcp_stream() {
    let mut stream = TcpStream::connect("127.0.0.1", 1234).unwrap();
    assert_eq!(OS_SOCKET_OPEN_CALLS.load(Ordering::SeqCst), 1);
    assert_eq!(OS_SOCKET_CONNECT_CALLS.load(Ordering::SeqCst), 1);

    let send_bytes = [10, 20, 30];
    let sent = stream.send(&send_bytes).unwrap();
    assert_eq!(sent, 3);
    assert_eq!(OS_SOCKET_SEND_CALLS.load(Ordering::SeqCst), 1);

    let mut recv_bytes = [0u8; 8];
    let recvd = stream.recv(&mut recv_bytes, 1000).unwrap();
    assert_eq!(recvd, 4);
    assert_eq!(&recv_bytes[..4], &[1, 2, 3, 4]);

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
