//! `scn_lab`: scenario flight applications.

#![cfg_attr(not(test), no_std)]
#![deny(unsafe_code)]

use cfe::es::{self, RunStatus};
use cfe::evs::{self, EventType};
use cfe::sb::{MsgId, Pipe, Qos, Timeout, transmit_msg};
use refapp_flight::FlightNode;
use refapp_scenario::{Apid, Delivered, Instant, Node, Output, StepInput};
use step_lab_core::msg::{
    CmdPacketDoneMsg, CmdPacketMsg, DoneMsg, RecordMsg, TlmPacketMsg, WakeAppsMsg,
};

/// Event ID for `scn_lab` start-up event.
pub const SCN_LAB_STARTUP_EID: u16 = 1;

/// Prepares a Software Bus message header by setting its message ID and total size.
fn prepare_msg(msg: &mut cfe_sys::CFE_MSG_Message_t, msg_id: u32, size: usize) -> Result<(), ()> {
    cfe::msg::set_msg_id(msg, MsgId::from_u32(msg_id)).map_err(|_| ())?;
    cfe::msg::set_size(msg, size).map_err(|_| ())
}

/// Sends a `DONE` message to `step_lab` signaling turn completion.
fn send_done(next_deadline: Option<u64>) {
    let mut done = DoneMsg::new(next_deadline);
    if prepare_msg(
        done.msg_mut(),
        cfe_sys::NODE_DONE_MID,
        core::mem::size_of::<DoneMsg>(),
    )
    .is_err()
    {
        let _ = evs::send_event(4, EventType::Critical, "SCN_LAB: Failed to init DONE msg");
        return;
    }
    if transmit_msg(done.msg(), true).is_err() {
        let _ = evs::send_event(
            5,
            EventType::Critical,
            "SCN_LAB: Failed to transmit DONE msg",
        );
    }
}

/// Sends a `CMD_PACKET_DONE` message to `link_lab` signaling command packet processing complete.
fn send_cmd_packet_done() {
    let mut done = CmdPacketDoneMsg::new();
    if prepare_msg(
        done.msg_mut(),
        cfe_sys::NODE_CMD_PACKET_DONE_MID,
        core::mem::size_of::<CmdPacketDoneMsg>(),
    )
    .is_err()
    {
        let _ = evs::send_event(
            6,
            EventType::Critical,
            "SCN_LAB: Failed to init CMD_PACKET_DONE msg",
        );
        return;
    }
    if transmit_msg(done.msg(), true).is_err() {
        let _ = evs::send_event(
            7,
            EventType::Critical,
            "SCN_LAB: Failed to transmit CMD_PACKET_DONE msg",
        );
    }
}

/// Sends a telemetry packet to `link_lab`.
fn send_tlm_packet(packet: &[u8]) {
    let Some(mut tlm) = TlmPacketMsg::new(packet) else {
        let _ = evs::send_event(
            8,
            EventType::Critical,
            "SCN_LAB: Packet too long for TlmPacketMsg",
        );
        return;
    };
    if prepare_msg(
        tlm.msg_mut(),
        cfe_sys::NODE_TLM_PACKET_MID,
        core::mem::size_of::<TlmPacketMsg>(),
    )
    .is_err()
    {
        let _ = evs::send_event(
            9,
            EventType::Critical,
            "SCN_LAB: Failed to init TLM_PACKET msg",
        );
        return;
    }
    if transmit_msg(tlm.msg(), true).is_err() {
        let _ = evs::send_event(
            10,
            EventType::Critical,
            "SCN_LAB: Failed to transmit TLM_PACKET msg",
        );
    }
}

/// Sends an application event to `step_lab`.
fn send_record_event(code: u16, args: &[u64]) {
    let Some(mut rec) = RecordMsg::new_event(code, args) else {
        let _ = evs::send_event(
            11,
            EventType::Critical,
            "SCN_LAB: Too many args for RecordMsg event",
        );
        return;
    };
    if prepare_msg(
        rec.msg_mut(),
        cfe_sys::NODE_RECORD_MID,
        core::mem::size_of::<RecordMsg>(),
    )
    .is_err()
    {
        let _ = evs::send_event(
            12,
            EventType::Critical,
            "SCN_LAB: Failed to init RECORD msg",
        );
        return;
    }
    if transmit_msg(rec.msg(), true).is_err() {
        let _ = evs::send_event(
            13,
            EventType::Critical,
            "SCN_LAB: Failed to transmit RECORD msg",
        );
    }
}

/// Output sink for `FlightNode` running inside `scn_lab`.
struct ScnSink;

impl Output for ScnSink {
    fn unit(&mut self, _octets: &[u8]) {}

    fn packet_out(&mut self, _apid: Apid, octets: &[u8]) {
        send_tlm_packet(octets);
    }

    fn packet_in(&mut self, _apid: Apid, _octets: &[u8]) {}

    fn app_event(&mut self, code: u16, args: &[u64]) {
        send_record_event(code, args);
    }
}

/// Runs the `scn_lab` initialization, Software Bus dispatch loop, and teardown.
fn app_main() {
    // 1. Register with EVS.
    if evs::register().is_err() {
        es::exit_app(RunStatus::AppError);
        return;
    }

    // 2. Create Software Bus input pipe.
    let Ok(mut pipe) = Pipe::create(32, "SCN_LAB_PIPE") else {
        let _ = evs::send_event(2, EventType::Critical, "SCN_LAB: Failed to create SB pipe");
        es::exit_app(RunStatus::AppError);
        return;
    };

    // 3. Subscribe to scenario turns.
    if pipe
        .subscribe_ex(MsgId::from_u32(cfe_sys::NODE_START_MID), Qos::DEFAULT, 4)
        .is_err()
        || pipe
            .subscribe_ex(
                MsgId::from_u32(cfe_sys::NODE_WAKE_APPS_MID),
                Qos::DEFAULT,
                4,
            )
            .is_err()
        || pipe
            .subscribe_ex(
                MsgId::from_u32(cfe_sys::NODE_CMD_PACKET_MID),
                Qos::DEFAULT,
                32,
            )
            .is_err()
    {
        let _ = evs::send_event(
            3,
            EventType::Critical,
            "SCN_LAB: Failed to subscribe to turns",
        );
        let _ = pipe.delete();
        es::exit_app(RunStatus::AppError);
        return;
    }

    // 4. Send start-up event.
    let _ = evs::send_event(
        SCN_LAB_STARTUP_EID,
        EventType::Information,
        "SCN_LAB App Initialized. Version 0.1.0",
    );

    // 5. Initialize flight node state.
    let mut flight_node = FlightNode::new();

    // 6. Run ES loop, pending on the pipe.
    let mut run_status = RunStatus::AppRun;
    while es::run_loop(&mut run_status) {
        let Ok(buf) = pipe.receive_buffer(Timeout::PendForever) else {
            continue;
        };

        let Ok(mid) = buf.msg_id() else {
            continue;
        };

        match mid.as_u32() {
            cfe_sys::NODE_START_MID => {
                flight_node = FlightNode::new();
                send_done(None);
            }
            cfe_sys::NODE_CMD_PACKET_MID => {
                if let Some(cmd) = CmdPacketMsg::from_bytes(buf.as_bytes()) {
                    let now = Instant::from_micros(cmd.now);
                    let pkt = cmd.packet_bytes();
                    let mut sink = ScnSink;
                    flight_node.handle_delivered(now, Delivered::new(&[pkt]), &mut sink);
                }
                send_cmd_packet_done();
            }
            cfe_sys::NODE_WAKE_APPS_MID => {
                if let Some(wake) = WakeAppsMsg::from_bytes(buf.as_bytes()) {
                    let now = Instant::from_micros(wake.now);
                    let mut sink = ScnSink;
                    let next_deadline = flight_node.step(
                        &StepInput {
                            now,
                            delivered: Delivered::new(&[]),
                            ops: &[],
                            tx_slot: false,
                        },
                        &mut sink,
                    );
                    send_done(next_deadline.map(Instant::as_micros));
                } else {
                    send_done(None);
                }
            }
            _ => {}
        }
    }

    let _ = pipe.delete();
    es::exit_app(run_status);
}

/// Application entry point exported to cFE.
#[expect(unsafe_code, reason = "cFS entry point must be extern \"C\"")]
#[unsafe(no_mangle)]
pub extern "C" fn SCN_LAB_AppMain() {
    app_main();
}

#[cfg(not(test))]
#[panic_handler]
const fn panic(_info: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}
