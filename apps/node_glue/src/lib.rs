//! Shared Software Bus glue for node cFS applications.
//!
//! `#![no_std]`, `#![forbid(unsafe_code)]`, and free of `aldis`.

#![no_std]
#![forbid(unsafe_code)]

use cfe::evs::{self, EventType};
use cfe::sb::{MsgId, transmit_msg};
use cfe::status::Status;
use step_lab_core::msg::{DoneMsg, RecordMsg, TlmPacketMsg};

/// Shared EVS event IDs used by [`node_glue`](crate) helpers (kept above per-app EIDs).
const EID_DONE_INIT: u16 = 100;
const EID_DONE_TX: u16 = 101;
const EID_TLM_LEN: u16 = 102;
const EID_TLM_INIT: u16 = 103;
const EID_TLM_TX: u16 = 104;
const EID_RECORD_ARGS: u16 = 105;
const EID_RECORD_INIT: u16 = 106;
const EID_RECORD_TX: u16 = 107;

/// Prepares a Software Bus message header by setting its message ID and total size.
///
/// # Errors
///
/// Returns `Err(Status)` if cFE fails to set the message ID or message size.
pub fn prepare_msg(
    msg: &mut cfe_sys::CFE_MSG_Message_t,
    msg_id: u32,
    size: usize,
) -> Result<(), Status> {
    cfe::msg::set_msg_id(msg, MsgId::from_u32(msg_id))?;
    cfe::msg::set_size(msg, size)
}

/// Initializes a Software Bus message header and transmits it, emitting critical EVS events on failure.
pub fn send_sb_msg(
    msg: &mut cfe_sys::CFE_MSG_Message_t,
    msg_id: u32,
    size: usize,
    init_eid: u16,
    init_err: &str,
    tx_eid: u16,
    tx_err: &str,
) {
    if prepare_msg(msg, msg_id, size).is_err() {
        let _ = evs::send_event(init_eid, EventType::Critical, init_err);
        return;
    }
    if transmit_msg(msg, true).is_err() {
        let _ = evs::send_event(tx_eid, EventType::Critical, tx_err);
    }
}

/// Sends a `DONE` message to `step_lab` signaling turn completion.
pub fn send_done(next_deadline: Option<u64>) {
    let mut done = DoneMsg::new(next_deadline);
    send_sb_msg(
        done.msg_mut(),
        cfe_sys::NODE_DONE_MID,
        core::mem::size_of::<DoneMsg>(),
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
    send_sb_msg(
        tlm.msg_mut(),
        cfe_sys::NODE_TLM_PACKET_MID,
        core::mem::size_of::<TlmPacketMsg>(),
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
    send_sb_msg(
        rec.msg_mut(),
        cfe_sys::NODE_RECORD_MID,
        core::mem::size_of::<RecordMsg>(),
        EID_RECORD_INIT,
        "NODE: Failed to init RECORD msg",
        EID_RECORD_TX,
        "NODE: Failed to transmit RECORD msg",
    );
}
