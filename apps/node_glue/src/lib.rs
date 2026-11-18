//! Shared Software Bus glue for node cFS applications.
//!
//! `#![no_std]`, `#![forbid(unsafe_code)]`, and free of `aldis`.

#![no_std]
#![forbid(unsafe_code)]

use cfe::evs::{self, EventType};
use cfe::sb::{MsgId, transmit_msg};
use cfe::status::Status;
use step_lab_core::msg::{
    CmdPacketDoneMsg, CmdPacketMsg, DoneMsg, RecordMsg, ScnStatusMsg, StartMsg, TlmPacketMsg,
    UnitMsg, WakeCmdMsg, WakeHkMsg, WakePlMsg, WakeRxMsg, WakeTurn, WakeTxMsg,
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

/// A Software Bus message struct bound at compile time to its [`MsgId`] and byte size (`size_of::<Self>()`).
pub trait SbMessage: Sized {
    /// The Software Bus [`MsgId`] for this message type.
    const MID: MsgId;

    /// Returns a reference to the generic cFE message header.
    fn msg(&self) -> &cfe_sys::CFE_MSG_Message_t;

    /// Returns a mutable reference to the generic cFE message header.
    fn msg_mut(&mut self) -> &mut cfe_sys::CFE_MSG_Message_t;
}

macro_rules! impl_sb_message {
    ($ty:ty, $mid:expr) => {
        impl SbMessage for $ty {
            const MID: MsgId = MsgId::from_u32($mid);

            fn msg(&self) -> &cfe_sys::CFE_MSG_Message_t {
                Self::msg(self)
            }

            fn msg_mut(&mut self) -> &mut cfe_sys::CFE_MSG_Message_t {
                Self::msg_mut(self)
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
        let _ = evs::send_event(init_eid, EventType::Critical, init_err);
        return Err(err);
    }
    if let Err(err) = transmit_msg(msg.msg(), true) {
        let _ = evs::send_event(tx_eid, EventType::Critical, tx_err);
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
}
