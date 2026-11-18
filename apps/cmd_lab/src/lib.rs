//! `cmd_lab`: scenario command handler, time-tag queue, table store, and memory dump app.

#![cfg_attr(not(test), no_std)]
#![deny(unsafe_code)]

use cfe::es::{self, RunStatus};
use cfe::evs::{self, EventType};
use cfe::sb::{MsgId, Pipe, Qos, Timeout, transmit_msg};
use refapp_flight::FlightNode;
use refapp_flight::layout::{PayloadMode, Status};
use refapp_scenario::{Apid, Delivered, Instant, Node, Output, StepInput};
use step_lab_core::msg::{
    CmdPacketDoneMsg, CmdPacketMsg, DoneMsg, RecordMsg, ScnStatus, ScnStatusMsg, TlmPacketMsg,
    WakeCmdMsg,
};

/// Event ID for `cmd_lab` start-up event.
pub const CMD_LAB_STARTUP_EID: u16 = 1;

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
        let _ = evs::send_event(4, EventType::Critical, "CMD_LAB: Failed to init DONE msg");
        return;
    }
    if transmit_msg(done.msg(), true).is_err() {
        let _ = evs::send_event(
            5,
            EventType::Critical,
            "CMD_LAB: Failed to transmit DONE msg",
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
            "CMD_LAB: Failed to init CMD_PACKET_DONE msg",
        );
        return;
    }
    if transmit_msg(done.msg(), true).is_err() {
        let _ = evs::send_event(
            7,
            EventType::Critical,
            "CMD_LAB: Failed to transmit CMD_PACKET_DONE msg",
        );
    }
}

/// Sends a telemetry packet to `link_lab`.
fn send_tlm_packet(packet: &[u8]) {
    let Some(mut tlm) = TlmPacketMsg::new(packet) else {
        let _ = evs::send_event(
            8,
            EventType::Critical,
            "CMD_LAB: Packet too long for TlmPacketMsg",
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
            "CMD_LAB: Failed to init TLM_PACKET msg",
        );
        return;
    }
    if transmit_msg(tlm.msg(), true).is_err() {
        let _ = evs::send_event(
            10,
            EventType::Critical,
            "CMD_LAB: Failed to transmit TLM_PACKET msg",
        );
    }
}

/// Sends an application event to `step_lab`.
fn send_record_event(code: u16, args: &[u64]) {
    let Some(mut rec) = RecordMsg::new_event(code, args) else {
        let _ = evs::send_event(
            11,
            EventType::Critical,
            "CMD_LAB: Too many args for RecordMsg event",
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
            "CMD_LAB: Failed to init RECORD msg",
        );
        return;
    }
    if transmit_msg(rec.msg(), true).is_err() {
        let _ = evs::send_event(
            13,
            EventType::Critical,
            "CMD_LAB: Failed to transmit RECORD msg",
        );
    }
}

/// Broadcasts the current scenario [`Status`] to `hk_lab` and `pl_lab`.
fn send_scn_status(status: &Status) {
    let mut msg = ScnStatusMsg::new(ScnStatus {
        commands_executed: status.commands_executed,
        commands_rejected: status.commands_rejected,
        queue_occupancy: status.queue_occupancy,
        table_id: status.table_id,
        table_crc: status.table_crc,
        dumps_completed: status.dumps_completed,
        payload_mode_on: status.payload_mode == PayloadMode::On,
    });
    if prepare_msg(
        msg.msg_mut(),
        cfe_sys::NODE_SCN_STATUS_MID,
        core::mem::size_of::<ScnStatusMsg>(),
    )
    .is_err()
    {
        let _ = evs::send_event(
            14,
            EventType::Critical,
            "CMD_LAB: Failed to init SCN_STATUS msg",
        );
        return;
    }
    if transmit_msg(msg.msg(), true).is_err() {
        let _ = evs::send_event(
            15,
            EventType::Critical,
            "CMD_LAB: Failed to transmit SCN_STATUS msg",
        );
    }
}

/// Output sink for `FlightNode` command/dump execution inside `cmd_lab`.
struct CmdSink;

impl Output for CmdSink {
    fn unit(&mut self, _octets: &[u8]) {}

    fn packet_out(&mut self, _apid: Apid, octets: &[u8]) {
        send_tlm_packet(octets);
    }

    fn packet_in(&mut self, _apid: Apid, _octets: &[u8]) {}

    fn app_event(&mut self, code: u16, args: &[u64]) {
        send_record_event(code, args);
    }
}

/// No-op sink used when probing `FlightNode::step` for the next command/dump deadline.
struct NullSink;

impl Output for NullSink {
    fn unit(&mut self, _octets: &[u8]) {}
    fn packet_out(&mut self, _apid: Apid, _octets: &[u8]) {}
    fn packet_in(&mut self, _apid: Apid, _octets: &[u8]) {}
    fn app_event(&mut self, _code: u16, _args: &[u64]) {}
}

/// Runs the `cmd_lab` initialization, Software Bus dispatch loop, and teardown.
fn app_main() {
    // 1. Register with EVS.
    if evs::register().is_err() {
        es::exit_app(RunStatus::AppError);
        return;
    }

    // 2. Create Software Bus input pipe.
    let Ok(mut pipe) = Pipe::create(32, "CMD_LAB_PIPE") else {
        let _ = evs::send_event(2, EventType::Critical, "CMD_LAB: Failed to create SB pipe");
        es::exit_app(RunStatus::AppError);
        return;
    };

    // 3. Subscribe to scenario turns.
    if pipe
        .subscribe_ex(MsgId::from_u32(cfe_sys::NODE_START_MID), Qos::DEFAULT, 4)
        .is_err()
        || pipe
            .subscribe_ex(MsgId::from_u32(cfe_sys::NODE_WAKE_CMD_MID), Qos::DEFAULT, 4)
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
            "CMD_LAB: Failed to subscribe to turns",
        );
        let _ = pipe.delete();
        es::exit_app(RunStatus::AppError);
        return;
    }

    // 4. Send start-up event.
    let _ = evs::send_event(
        CMD_LAB_STARTUP_EID,
        EventType::Information,
        "CMD_LAB App Initialized. Version 0.1.0",
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
                send_scn_status(flight_node.status());
                send_done(None);
            }
            cfe_sys::NODE_CMD_PACKET_MID => {
                if let Some(cmd) = CmdPacketMsg::from_bytes(buf.as_bytes()) {
                    let now = Instant::from_micros(cmd.now);
                    let pkt = cmd.packet_bytes();
                    let mut sink = CmdSink;
                    flight_node.handle_delivered(now, Delivered::new(&[pkt]), &mut sink);
                }
                send_scn_status(flight_node.status());
                send_cmd_packet_done();
            }
            cfe_sys::NODE_WAKE_CMD_MID => {
                if let Some(wake) = WakeCmdMsg::from_bytes(buf.as_bytes()) {
                    let now = Instant::from_micros(wake.now);
                    let mut sink = CmdSink;
                    flight_node.execute_due(now, &mut sink);
                    flight_node.dump_segments(now, &mut sink);
                    send_scn_status(flight_node.status());
                    let mut probe = flight_node.clone();
                    let next_deadline = probe.step(
                        &StepInput {
                            now,
                            delivered: Delivered::new(&[]),
                            ops: &[],
                            tx_slot: false,
                        },
                        &mut NullSink,
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
pub extern "C" fn CMD_LAB_AppMain() {
    app_main();
}

#[cfg(not(test))]
#[panic_handler]
const fn panic(_info: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}
