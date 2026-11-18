//! `hk_lab`: periodic housekeeping telemetry app.

#![cfg_attr(not(test), no_std)]
#![deny(unsafe_code)]

use aldis::spp::SequenceFlags;
use cfe::es::{self, RunStatus};
use cfe::evs::{self, EventType};
use cfe::sb::{MsgId, Pipe, Qos, Timeout};
use node_glue::{send_done, send_record_event, send_tlm_packet};
use refapp_flight::event::APP_PACKET_REJECTED;
use refapp_flight::layout::{PayloadMode, Status, housekeeping};
use refapp_flight::schedule::Periodic;
use refapp_flight::{Downlink, Stream};
use refapp_scenario::{Apid, Instant, Output};
use step_lab_core::msg::{ScnPayloadMode, ScnStatusMsg, WakeTurn};

/// Event ID for `hk_lab` start-up event.
pub const HK_LAB_STARTUP_EID: u16 = 1;

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

/// Converts a [`step_lab_core::msg::ScnStatus`] into [`refapp_flight::layout::Status`].
const fn status_from_scn(s: step_lab_core::msg::ScnStatus) -> Status {
    Status {
        commands_executed: s.commands_executed,
        commands_rejected: s.commands_rejected,
        queue_occupancy: s.queue_occupancy,
        table_id: s.table_id,
        table_crc: s.table_crc,
        dumps_completed: s.dumps_completed,
        payload_mode: match s.payload_mode {
            ScnPayloadMode::Off => PayloadMode::Off,
            ScnPayloadMode::On => PayloadMode::On,
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
                if let Some(next_status) = ScnStatusMsg::from_bytes(buf.as_bytes())
                    .and_then(|msg| msg.status())
                    .map(status_from_scn)
                {
                    status = next_status;
                } else {
                    let _ = evs::send_event(
                        12,
                        EventType::Error,
                        "HK_LAB: Malformed ScnStatusMsg received",
                    );
                }
            }
            cfe_sys::NODE_WAKE_HK_MID => {
                let next_deadline = WakeTurn::Hk
                    .decode(mid.as_u32(), buf.as_bytes())
                    .map_or_else(
                        || {
                            let _ = evs::send_event(
                                13,
                                EventType::Error,
                                "HK_LAB: Malformed WakeTimeMsg received",
                            );
                            None
                        },
                        |now_us| {
                            handle_wake_hk(
                                &mut downlink,
                                &status,
                                &mut hk_slot,
                                Instant::from_micros(now_us),
                            )
                        },
                    );
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
