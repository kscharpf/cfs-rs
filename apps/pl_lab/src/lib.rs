//! `pl_lab`: periodic payload telemetry app.

#![cfg_attr(not(test), no_std)]
#![deny(unsafe_code)]

use aldis::spp::SequenceFlags;
use cfe::es::{self, RunStatus};
use cfe::evs::{self, EventType};
use cfe::sb::{MsgId, Pipe, Qos, Timeout, transmit_msg};
use refapp_flight::event::APP_PACKET_REJECTED;
use refapp_flight::layout::{PayloadMode, payload};
use refapp_flight::schedule::Periodic;
use refapp_flight::{Downlink, Stream};
use refapp_scenario::{Apid, Instant, Output};
use step_lab_core::msg::{DoneMsg, RecordMsg, ScnStatusMsg, TlmPacketMsg, WakePlMsg};

/// Event ID for `pl_lab` start-up event.
pub const PL_LAB_STARTUP_EID: u16 = 1;

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
        let _ = evs::send_event(4, EventType::Critical, "PL_LAB: Failed to init DONE msg");
        return;
    }
    if transmit_msg(done.msg(), true).is_err() {
        let _ = evs::send_event(
            5,
            EventType::Critical,
            "PL_LAB: Failed to transmit DONE msg",
        );
    }
}

/// Sends a telemetry packet to `link_lab`.
fn send_tlm_packet(packet: &[u8]) {
    let Some(mut tlm) = TlmPacketMsg::new(packet) else {
        let _ = evs::send_event(
            6,
            EventType::Critical,
            "PL_LAB: Packet too long for TlmPacketMsg",
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
            7,
            EventType::Critical,
            "PL_LAB: Failed to init TLM_PACKET msg",
        );
        return;
    }
    if transmit_msg(tlm.msg(), true).is_err() {
        let _ = evs::send_event(
            8,
            EventType::Critical,
            "PL_LAB: Failed to transmit TLM_PACKET msg",
        );
    }
}

/// Sends an application event to `step_lab`.
fn send_record_event(code: u16, args: &[u64]) {
    let Some(mut rec) = RecordMsg::new_event(code, args) else {
        let _ = evs::send_event(
            9,
            EventType::Critical,
            "PL_LAB: Too many args for RecordMsg event",
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
        let _ = evs::send_event(10, EventType::Critical, "PL_LAB: Failed to init RECORD msg");
        return;
    }
    if transmit_msg(rec.msg(), true).is_err() {
        let _ = evs::send_event(
            11,
            EventType::Critical,
            "PL_LAB: Failed to transmit RECORD msg",
        );
    }
}

/// Output sink for `Downlink` running inside `pl_lab`.
struct PlSink;

impl Output for PlSink {
    fn unit(&mut self, _octets: &[u8]) {}

    fn packet_out(&mut self, _apid: Apid, octets: &[u8]) {
        send_tlm_packet(octets);
    }

    fn packet_in(&mut self, _apid: Apid, _octets: &[u8]) {}

    fn app_event(&mut self, code: u16, args: &[u64]) {
        send_record_event(code, args);
    }
}

/// Runs the `pl_lab` initialization, Software Bus dispatch loop, and teardown.
fn app_main() {
    // 1. Register with EVS.
    if evs::register().is_err() {
        es::exit_app(RunStatus::AppError);
        return;
    }

    // 2. Create Software Bus input pipe.
    let Ok(mut pipe) = Pipe::create(32, "PL_LAB_PIPE") else {
        let _ = evs::send_event(2, EventType::Critical, "PL_LAB: Failed to create SB pipe");
        es::exit_app(RunStatus::AppError);
        return;
    };

    // 3. Subscribe to scenario turns and status updates.
    if pipe
        .subscribe_ex(MsgId::from_u32(cfe_sys::NODE_START_MID), Qos::DEFAULT, 4)
        .is_err()
        || pipe
            .subscribe_ex(
                MsgId::from_u32(cfe_sys::NODE_SCN_STATUS_MID),
                Qos::DEFAULT,
                32,
            )
            .is_err()
        || pipe
            .subscribe_ex(MsgId::from_u32(cfe_sys::NODE_WAKE_PL_MID), Qos::DEFAULT, 4)
            .is_err()
    {
        let _ = evs::send_event(
            3,
            EventType::Critical,
            "PL_LAB: Failed to subscribe to turns",
        );
        let _ = pipe.delete();
        es::exit_app(RunStatus::AppError);
        return;
    }

    // 4. Send start-up event.
    let _ = evs::send_event(
        PL_LAB_STARTUP_EID,
        EventType::Information,
        "PL_LAB App Initialized. Version 0.1.0",
    );

    // 5. Initialize payload state.
    let mut downlink = Downlink::new();
    let mut payload_mode = PayloadMode::On;
    let mut payload_slot = 0_u32;
    let mut payload_number = 0_u32;

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
                downlink = Downlink::new();
                payload_mode = PayloadMode::On;
                payload_slot = 0;
                payload_number = 0;
                send_done(None);
            }
            cfe_sys::NODE_SCN_STATUS_MID => {
                if let Some(msg) = ScnStatusMsg::from_bytes(buf.as_bytes()) {
                    payload_mode = if msg.status().payload_mode_on {
                        PayloadMode::On
                    } else {
                        PayloadMode::Off
                    };
                }
            }
            cfe_sys::NODE_WAKE_PL_MID => {
                if let Some(wake) = WakePlMsg::from_bytes(buf.as_bytes()) {
                    let now = Instant::from_micros(wake.now);
                    let mut sink = PlSink;
                    while let Some(at) = Periodic::PAYLOAD.at(payload_slot).filter(|at| *at <= now)
                    {
                        if payload_mode == PayloadMode::On {
                            let data = payload(payload_number);
                            if let Err(e) = downlink.send(
                                Stream::Payload,
                                SequenceFlags::Unsegmented,
                                at,
                                &data,
                                &mut sink,
                            ) {
                                send_record_event(
                                    APP_PACKET_REJECTED,
                                    &[u64::from(e.kind().code())],
                                );
                            }
                            payload_number = payload_number.saturating_add(1);
                        }
                        payload_slot = payload_slot.saturating_add(1);
                    }
                    send_done(Periodic::PAYLOAD.at(payload_slot).map(Instant::as_micros));
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
pub extern "C" fn PL_LAB_AppMain() {
    app_main();
}

#[cfg(not(test))]
#[panic_handler]
const fn panic(_info: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}
