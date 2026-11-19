//! Shared Software Bus glue for node cFS applications.
//!
//! `#![no_std]`, `#![forbid(unsafe_code)]`, and free of `aldis`.

#![no_std]
#![forbid(unsafe_code)]

use cfe::evs::{self, EventType};
use cfe::sb::{MsgId, Timeout, transmit_bytes};
use cfe::status::Status;
use step_lab_core::msg::{
    CmdPacketDoneMsg, CmdPacketMsg, DoneMsg, MAX_SB_MSG_LEN, RecordMsg, ScnStatusMsg, StartMsg,
    TlmPacketMsg, UnitMsg, WakeCmdMsg, WakeHkMsg, WakePlMsg, WakeRxMsg, WakeTurn, WakeTxMsg,
};

/// Shared EVS event IDs used by [`node_glue`](crate) helpers (kept above per-app EIDs).
const EID_DONE_INIT: u16 = 100;
const EID_DONE_TX: u16 = 101;
const EID_TLM_LEN: u16 = 102;
const EID_TLM_INIT: u16 = 103;
const EID_TLM_TX: u16 = 104;
const EID_RECORD_ARGS: u16 = 105;
const EID_RECORD_INIT: u16 = 106;
const EID_RECORD_TX: u16 = 107;
const EID_PIPE_RX_ERR: u16 = 108;

/// Action returned by [`PipeRecvGuard`] after a pipe receive failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecvAction {
    /// Continue the run loop (timeout or transient error below threshold).
    Continue,
    /// Persistent error threshold reached; exit the app with `RunStatus::AppError`.
    Abort,
}

/// Tracks consecutive non-timeout Software Bus pipe receive errors with rate-limited EVS logging.
#[derive(Debug, Clone, Copy, Default)]
pub struct PipeRecvGuard {
    consecutive_errors: u8,
}

impl PipeRecvGuard {
    /// Poll timeout for main application Software Bus loops, allowing periodic `CFE_ES_RunLoop`
    /// checks when idle.
    pub const RECV_TIMEOUT: Timeout = Timeout::Millis(500);

    /// Consecutive non-timeout pipe receive errors before aborting the application loop.
    pub const MAX_CONSECUTIVE_ERRORS: u8 = 5;

    /// Creates a new [`PipeRecvGuard`] with zero recorded errors.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            consecutive_errors: 0,
        }
    }

    /// Resets the consecutive error counter after a successful pipe receive.
    pub const fn on_ok(&mut self) {
        self.consecutive_errors = 0;
    }

    /// Classifies a pipe receive error as a timeout or non-timeout failure, returning any EVS
    /// severity to emit and whether the caller should continue or abort.
    pub const fn classify_error(&mut self, is_timeout: bool) -> (Option<EventType>, RecvAction) {
        if is_timeout {
            self.consecutive_errors = 0;
            return (None, RecvAction::Continue);
        }
        let prev = self.consecutive_errors;
        self.consecutive_errors = prev.saturating_add(1);
        if self.consecutive_errors >= Self::MAX_CONSECUTIVE_ERRORS {
            (Some(EventType::Critical), RecvAction::Abort)
        } else if prev == 0 {
            (Some(EventType::Error), RecvAction::Continue)
        } else {
            (None, RecvAction::Continue)
        }
    }

    /// Handles a `Pipe::receive_buffer` error, emitting rate-limited status-bearing EVS events
    /// and returning [`RecvAction::Abort`] once [`MAX_CONSECUTIVE_ERRORS`](Self::MAX_CONSECUTIVE_ERRORS)
    /// non-timeout errors occur consecutively.
    pub fn on_err(&mut self, status: Status, msg: &str) -> RecvAction {
        let (event_ty, action) = self.classify_error(cfe::sb::is_pipe_timeout(status));
        if let Some(ty) = event_ty {
            let _ = evs::send_event_with_status(EID_PIPE_RX_ERR, ty, msg, status);
        }
        action
    }
}

/// Bounds waiting for a peer response with periodic timeout polling so `es::run_loop` stays responsive.
#[derive(Debug, Clone, Copy, Default)]
pub struct PeerWaitGuard {
    timeouts: u32,
}

impl PeerWaitGuard {
    /// Poll timeout for peer response waits (`500 ms`).
    pub const POLL_TIMEOUT: Timeout = Timeout::Millis(500);

    /// Maximum number of consecutive [`POLL_TIMEOUT`](Self::POLL_TIMEOUT) intervals before aborting (`10 s`).
    pub const MAX_TIMEOUTS: u32 = 20;

    /// Creates a new [`PeerWaitGuard`] with zero recorded timeouts.
    #[must_use]
    pub const fn new() -> Self {
        Self { timeouts: 0 }
    }

    /// Resets the consecutive timeout counter after receiving any pipe message.
    pub const fn on_ok(&mut self) {
        self.timeouts = 0;
    }

    /// Classifies a pipe receive error during a peer wait as retryable or terminal.
    pub const fn classify_error(&mut self, is_timeout: bool) -> RecvAction {
        if is_timeout {
            self.timeouts = self.timeouts.saturating_add(1);
            if self.timeouts < Self::MAX_TIMEOUTS {
                RecvAction::Continue
            } else {
                RecvAction::Abort
            }
        } else {
            RecvAction::Abort
        }
    }

    /// Handles a `Pipe::receive_buffer` error during a bounded peer wait, emitting a critical
    /// status-bearing EVS event when returning [`RecvAction::Abort`].
    pub fn on_err(
        &mut self,
        status: Status,
        timeout_eid: u16,
        timeout_msg: &str,
        err_eid: u16,
        err_msg: &str,
    ) -> RecvAction {
        let is_timeout = cfe::sb::is_pipe_timeout(status);
        let action = self.classify_error(is_timeout);
        if action == RecvAction::Abort {
            let (eid, msg) = if is_timeout {
                (timeout_eid, timeout_msg)
            } else {
                (err_eid, err_msg)
            };
            let _ = evs::send_event_with_status(eid, EventType::Critical, msg, status);
        }
        action
    }
}

const _: () = {
    assert!(PipeRecvGuard::MAX_CONSECUTIVE_ERRORS >= 2);
    assert!(PeerWaitGuard::MAX_TIMEOUTS > 0);
};

/// A Software Bus message struct bound at compile time to its [`MsgId`] and byte size (`size_of::<Self>()`).
pub trait SbMessage: Sized {
    /// The Software Bus [`MsgId`] for this message type.
    const MID: MsgId;

    /// Returns a reference to the generic cFE message header.
    fn msg(&self) -> &cfe_sys::CFE_MSG_Message_t;

    /// Returns a mutable reference to the generic cFE message header.
    fn msg_mut(&mut self) -> &mut cfe_sys::CFE_MSG_Message_t;

    /// Encodes this message (header and payload) into `dst`, returning the number of bytes written.
    fn encode_into(&self, dst: &mut [u8]) -> Option<usize>;
}

macro_rules! impl_sb_message {
    ($ty:ty, $mid:expr) => {
        const _: () = assert!(core::mem::size_of::<$ty>() <= MAX_SB_MSG_LEN);

        impl SbMessage for $ty {
            const MID: MsgId = MsgId::from_u32($mid);

            fn msg(&self) -> &cfe_sys::CFE_MSG_Message_t {
                Self::msg(self)
            }

            fn msg_mut(&mut self) -> &mut cfe_sys::CFE_MSG_Message_t {
                Self::msg_mut(self)
            }

            fn encode_into(&self, dst: &mut [u8]) -> Option<usize> {
                Self::encode_into(self, dst)
            }
        }
    };
}

impl_sb_message!(StartMsg, cfe_sys::NODE_START_MID);
impl_sb_message!(WakeRxMsg, cfe_sys::NODE_WAKE_RX_MID);
impl_sb_message!(WakeTxMsg, cfe_sys::NODE_WAKE_TX_MID);
impl_sb_message!(DoneMsg, cfe_sys::NODE_DONE_MID);
impl_sb_message!(ScnStatusMsg, cfe_sys::NODE_SCN_STATUS_MID);
impl_sb_message!(CmdPacketMsg, cfe_sys::NODE_CMD_PACKET_MID);
impl_sb_message!(CmdPacketDoneMsg, cfe_sys::NODE_CMD_PACKET_DONE_MID);
impl_sb_message!(TlmPacketMsg, cfe_sys::NODE_TLM_PACKET_MID);
impl_sb_message!(UnitMsg, cfe_sys::NODE_UNIT_MID);
impl_sb_message!(RecordMsg, cfe_sys::NODE_RECORD_MID);
impl_sb_message!(WakeCmdMsg, cfe_sys::NODE_WAKE_CMD_MID);
impl_sb_message!(WakeHkMsg, cfe_sys::NODE_WAKE_HK_MID);
impl_sb_message!(WakePlMsg, cfe_sys::NODE_WAKE_PL_MID);

/// Prepares a typed Software Bus message header by setting its compile-time [`SbMessage::MID`] and `size_of::<M>()`.
///
/// # Errors
///
/// Returns `Err(Status)` if cFE fails to set the message ID or message size.
pub fn prepare_msg<M: SbMessage>(msg: &mut M) -> Result<(), Status> {
    cfe::msg::set_msg_id(msg.msg_mut(), M::MID)?;
    cfe::msg::set_size(msg.msg_mut(), core::mem::size_of::<M>())
}

/// Initializes a typed Software Bus message header and transmits it, emitting critical EVS events on failure.
///
/// # Errors
///
/// Returns `Err(Status)` if message initialization or transmission fails.
pub fn send_sb_msg<M: SbMessage>(
    msg: &mut M,
    init_eid: u16,
    init_err: &str,
    tx_eid: u16,
    tx_err: &str,
) -> Result<(), Status> {
    if let Err(err) = prepare_msg(msg) {
        let _ = evs::send_event_with_status(init_eid, EventType::Critical, init_err, err);
        return Err(err);
    }
    let mut wire = [0u8; MAX_SB_MSG_LEN];
    let bad_arg = Status::from_raw(cfe_sys::CFE_SB_BAD_ARGUMENT);
    let Some(len) = msg.encode_into(&mut wire) else {
        let _ = evs::send_event_with_status(init_eid, EventType::Critical, init_err, bad_arg);
        return Err(bad_arg);
    };
    let Some(bytes) = wire.get(..len) else {
        let _ = evs::send_event_with_status(init_eid, EventType::Critical, init_err, bad_arg);
        return Err(bad_arg);
    };
    if let Err(err) = transmit_bytes(bytes, true) {
        let _ = evs::send_event_with_status(tx_eid, EventType::Critical, tx_err, err);
        return Err(err);
    }
    Ok(())
}

/// Sends a [`WakeTimeMsg`] for `turn` at scenario time `now`, emitting critical EVS events on failure.
///
/// # Errors
///
/// Returns `Err(Status)` if message initialization or transmission fails.
pub fn send_wake_turn(
    turn: WakeTurn,
    now: u64,
    init_eid: u16,
    init_err: &str,
    tx_eid: u16,
    tx_err: &str,
) -> Result<(), Status> {
    match turn {
        WakeTurn::Cmd => send_sb_msg(
            &mut WakeCmdMsg::new(now),
            init_eid,
            init_err,
            tx_eid,
            tx_err,
        ),
        WakeTurn::Hk => send_sb_msg(&mut WakeHkMsg::new(now), init_eid, init_err, tx_eid, tx_err),
        WakeTurn::Pl => send_sb_msg(&mut WakePlMsg::new(now), init_eid, init_err, tx_eid, tx_err),
    }
}

/// Sends a `DONE` message to `step_lab` signaling turn completion.
pub fn send_done(next_deadline: Option<u64>) {
    let mut done = DoneMsg::new(next_deadline);
    let _ = send_sb_msg(
        &mut done,
        EID_DONE_INIT,
        "NODE: Failed to init DONE msg",
        EID_DONE_TX,
        "NODE: Failed to transmit DONE msg",
    );
}

/// Sends a telemetry packet to `link_lab`.
pub fn send_tlm_packet(packet: &[u8]) {
    let Some(mut tlm) = TlmPacketMsg::new(packet) else {
        let _ = evs::send_event(
            EID_TLM_LEN,
            EventType::Critical,
            "NODE: Packet too long for TlmPacketMsg",
        );
        return;
    };
    let _ = send_sb_msg(
        &mut tlm,
        EID_TLM_INIT,
        "NODE: Failed to init TLM_PACKET msg",
        EID_TLM_TX,
        "NODE: Failed to transmit TLM_PACKET msg",
    );
}

/// Sends an application event record to `step_lab`.
pub fn send_record_event(code: u16, args: &[u64]) {
    let Some(mut rec) = RecordMsg::new_event(code, args) else {
        let _ = evs::send_event(
            EID_RECORD_ARGS,
            EventType::Critical,
            "NODE: Too many args for RecordMsg event",
        );
        return;
    };
    let _ = send_sb_msg(
        &mut rec,
        EID_RECORD_INIT,
        "NODE: Failed to init RECORD msg",
        EID_RECORD_TX,
        "NODE: Failed to transmit RECORD msg",
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sb_message_mids_match_cfe_sys() {
        assert_eq!(
            <StartMsg as SbMessage>::MID,
            MsgId::from_u32(cfe_sys::NODE_START_MID)
        );
        assert_eq!(
            <WakeRxMsg as SbMessage>::MID,
            MsgId::from_u32(cfe_sys::NODE_WAKE_RX_MID)
        );
        assert_eq!(
            <WakeTxMsg as SbMessage>::MID,
            MsgId::from_u32(cfe_sys::NODE_WAKE_TX_MID)
        );
        assert_eq!(
            <DoneMsg as SbMessage>::MID,
            MsgId::from_u32(cfe_sys::NODE_DONE_MID)
        );
        assert_eq!(
            <WakeCmdMsg as SbMessage>::MID,
            MsgId::from_u32(cfe_sys::NODE_WAKE_CMD_MID)
        );
        assert_eq!(
            <WakeHkMsg as SbMessage>::MID,
            MsgId::from_u32(cfe_sys::NODE_WAKE_HK_MID)
        );
        assert_eq!(
            <WakePlMsg as SbMessage>::MID,
            MsgId::from_u32(cfe_sys::NODE_WAKE_PL_MID)
        );
        assert_eq!(
            <ScnStatusMsg as SbMessage>::MID,
            MsgId::from_u32(cfe_sys::NODE_SCN_STATUS_MID)
        );
        assert_eq!(
            <CmdPacketMsg as SbMessage>::MID,
            MsgId::from_u32(cfe_sys::NODE_CMD_PACKET_MID)
        );
        assert_eq!(
            <CmdPacketDoneMsg as SbMessage>::MID,
            MsgId::from_u32(cfe_sys::NODE_CMD_PACKET_DONE_MID)
        );
        assert_eq!(
            <TlmPacketMsg as SbMessage>::MID,
            MsgId::from_u32(cfe_sys::NODE_TLM_PACKET_MID)
        );
        assert_eq!(
            <UnitMsg as SbMessage>::MID,
            MsgId::from_u32(cfe_sys::NODE_UNIT_MID)
        );
        assert_eq!(
            <RecordMsg as SbMessage>::MID,
            MsgId::from_u32(cfe_sys::NODE_RECORD_MID)
        );
    }

    #[test]
    fn pipe_recv_guard_tracks_errors_and_resets() {
        let mut guard = PipeRecvGuard::new();

        // Timeouts reset state and emit nothing.
        assert_eq!(guard.classify_error(true), (None, RecvAction::Continue));

        // First non-timeout error logs Error and continues; intermediate errors are quiet.
        assert_eq!(
            guard.classify_error(false),
            (Some(EventType::Error), RecvAction::Continue)
        );
        assert_eq!(guard.classify_error(false), (None, RecvAction::Continue));
        assert_eq!(guard.classify_error(false), (None, RecvAction::Continue));

        // Timeout or Ok resets consecutive error count.
        guard.on_ok();
        assert_eq!(
            guard.classify_error(false),
            (Some(EventType::Error), RecvAction::Continue)
        );
        assert_eq!(guard.classify_error(true), (None, RecvAction::Continue));

        // Five consecutive non-timeout errors trigger Critical + Abort.
        assert_eq!(
            guard.classify_error(false),
            (Some(EventType::Error), RecvAction::Continue)
        );
        for _ in 0..3 {
            assert_eq!(guard.classify_error(false), (None, RecvAction::Continue));
        }
        assert_eq!(
            guard.classify_error(false),
            (Some(EventType::Critical), RecvAction::Abort)
        );
    }

    #[test]
    fn peer_wait_guard_tracks_timeouts_and_aborts() {
        let mut guard = PeerWaitGuard::new();

        // Consecutive timeouts below MAX_TIMEOUTS return Continue.
        for _ in 0..PeerWaitGuard::MAX_TIMEOUTS - 1 {
            assert_eq!(guard.classify_error(true), RecvAction::Continue);
        }

        // Any ok message resets the consecutive timeout counter.
        guard.on_ok();
        for _ in 0..PeerWaitGuard::MAX_TIMEOUTS - 1 {
            assert_eq!(guard.classify_error(true), RecvAction::Continue);
        }

        // Reaching MAX_TIMEOUTS consecutive timeouts returns Abort.
        assert_eq!(guard.classify_error(true), RecvAction::Abort);

        // Non-timeout pipe errors abort immediately even with zero prior timeouts.
        guard.on_ok();
        assert_eq!(guard.classify_error(false), RecvAction::Abort);
    }
}
