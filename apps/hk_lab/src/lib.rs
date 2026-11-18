//! `hk_lab`: periodic housekeeping telemetry app.

#![cfg_attr(not(test), no_std)]
#![deny(unsafe_code)]

use aldis::spp::SequenceFlags;
use cfe::es::{self, RunStatus};
use cfe::evs::{self, EventType};
use cfe::sb::{MsgId, Pipe, Qos, Timeout, transmit_msg};
use refapp_flight::event::APP_PACKET_REJECTED;
use refapp_flight::layout::{PayloadMode, Status, housekeeping};
use refapp_flight::schedule::Periodic;
use refapp_flight::{Downlink, Stream};
use refapp_scenario::{Apid, Instant, Output};
use step_lab_core::msg::{DoneMsg, RecordMsg, ScnStatusMsg, TlmPacketMsg, WakeHkMsg};

/// Event ID for `hk_lab` start-up event.
pub const HK_LAB_STARTUP_EID: u16 = 1;

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
        let _ = evs::send_event(4, EventType::Critical, "HK_LAB: Failed to init DONE msg");
        return;
    }
    if transmit_msg(done.msg(), true).is_err() {
        let _ = evs::send_event(
            5,
            EventType::Critical,
            "HK_LAB: Failed to transmit DONE msg",
        );
    }
}

/// Sends a telemetry packet to `link_lab`.
fn send_tlm_packet(packet: &[u8]) {
    let Some(mut tlm) = TlmPacketMsg::new(packet) else {
        let _ = evs::send_event(
            6,
            EventType::Critical,
            "HK_LAB: Packet too long for TlmPacketMsg",
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
            "HK_LAB: Failed to init TLM_PACKET msg",
        );
        return;
    }
    if transmit_msg(tlm.msg(), true).is_err() {
        let _ = evs::send_event(
            8,
            EventType::Critical,
            "HK_LAB: Failed to transmit TLM_PACKET msg",
        );
    }
}

/// Sends an application event to `step_lab`.
fn send_record_event(code: u16, args: &[u64]) {
    let Some(mut rec) = RecordMsg::new_event(code, args) else {
        let _ = evs::send_event(
            9,
            EventType::Critical,
            "HK_LAB: Too many args for RecordMsg event",
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
        let _ = evs::send_event(10, EventType::Critical, "HK_LAB: Failed to init RECORD msg");
        return;
    }
    if transmit_msg(rec.msg(), true).is_err() {
        let _ = evs::send_event(
            11,
            EventType::Critical,
            "HK_LAB: Failed to transmit RECORD msg",
        );
    }
}

/// Output sink for `Downlink` running inside `hk_lab`.
struct HkSink;

impl Output for HkSink {
    fn unit(&mut self, _octets: &[u8]) {}

    fn packet_out(&mut self, _apid: Apid, octets: &[u8]) {
        send_tlm_packet(octets);
    }

    fn packet_in(&mut self, _apid: Apid, _octets: &[u8]) {}

    fn app_event(&mut self, code: u16, args: &[u64]) {
        send_record_event(code, args);
    }
}

/// Converts a [`ScnStatusMsg`] into [`refapp_flight::layout::Status`].
const fn status_from_msg(msg: &ScnStatusMsg) -> Status {
    let s = msg.status();
    Status {
        commands_executed: s.commands_executed,
        commands_rejected: s.commands_rejected,
        queue_occupancy: s.queue_occupancy,
        table_id: s.table_id,
        table_crc: s.table_crc,
        dumps_completed: s.dumps_completed,
        payload_mode: if s.payload_mode_on {
            PayloadMode::On
        } else {
            PayloadMode::Off
        },
    }
}

/// Emits all housekeeping packets due up to `now` and returns the next deadline.
fn handle_wake_hk(
    downlink: &mut Downlink,
    status: &Status,
    hk_slot: &mut u32,
    now: Instant,
) -> Option<u64> {
    let mut sink = HkSink;
    while let Some(at) = Periodic::HOUSEKEEPING.at(*hk_slot).filter(|at| *at <= now) {
        let data = housekeeping(status, *hk_slot);
        if let Err(e) = downlink.send(
            Stream::Housekeeping,
            SequenceFlags::Unsegmented,
            at,
            &data,
            &mut sink,
        ) {
            send_record_event(APP_PACKET_REJECTED, &[u64::from(e.kind().code())]);
        }
        *hk_slot = hk_slot.saturating_add(1);
    }
    Periodic::HOUSEKEEPING.at(*hk_slot).map(Instant::as_micros)
}

/// Runs the `hk_lab` initialization, Software Bus dispatch loop, and teardown.
fn app_main() {
    // 1. Register with EVS.
    if evs::register().is_err() {
        es::exit_app(RunStatus::AppError);
        return;
    }

    // 2. Create Software Bus input pipe.
    let Ok(mut pipe) = Pipe::create(32, "HK_LAB_PIPE") else {
        let _ = evs::send_event(2, EventType::Critical, "HK_LAB: Failed to create SB pipe");
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
            .subscribe_ex(MsgId::from_u32(cfe_sys::NODE_WAKE_HK_MID), Qos::DEFAULT, 4)
            .is_err()
    {
        let _ = evs::send_event(
            3,
            EventType::Critical,
            "HK_LAB: Failed to subscribe to turns",
        );
        let _ = pipe.delete();
        es::exit_app(RunStatus::AppError);
        return;
    }

    // 4. Send start-up event.
    let _ = evs::send_event(
        HK_LAB_STARTUP_EID,
        EventType::Information,
        "HK_LAB App Initialized. Version 0.1.0",
    );

    // 5. Initialize housekeeping state.
    let mut downlink = Downlink::new();
    let mut status = Status::default();
    let mut hk_slot = 0_u32;

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
                status = Status::default();
                hk_slot = 0;
                send_done(None);
            }
            cfe_sys::NODE_SCN_STATUS_MID => {
                if let Some(msg) = ScnStatusMsg::from_bytes(buf.as_bytes()) {
                    status = status_from_msg(&msg);
                }
            }
            cfe_sys::NODE_WAKE_HK_MID => {
                let next_deadline = WakeHkMsg::from_bytes(buf.as_bytes()).and_then(|wake| {
                    handle_wake_hk(
                        &mut downlink,
                        &status,
                        &mut hk_slot,
                        Instant::from_micros(wake.now),
                    )
                });
                send_done(next_deadline);
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
pub extern "C" fn HK_LAB_AppMain() {
    app_main();
}

#[cfg(not(test))]
#[panic_handler]
const fn panic(_info: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}
