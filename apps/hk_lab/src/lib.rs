//! `hk_lab`: periodic housekeeping telemetry app.

#![cfg_attr(not(test), no_std)]
#![deny(unsafe_code)]

use aldis::spp::SequenceFlags;
use cfe::es::{self, ExitStatus, RunStatus};
use cfe::evs::{self, EventType};
use cfe::sb::{MsgId, Pipe, Qos};
use node_glue::{PipeRecvGuard, RecvAction, send_done, send_record_event, send_tlm_packet};
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

/// Initializes `hk_lab`: registers with EVS, creates the SB pipe, subscribes, and emits startup event.
fn init_app() -> Result<Pipe, ()> {
    if evs::register().is_err() {
        return Err(());
    }

    let Ok(pipe) = Pipe::create(32, "HK_LAB_PIPE") else {
        let _ = evs::send_event(2, EventType::Critical, "HK_LAB: Failed to create SB pipe");
        return Err(());
    };

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
        return Err(());
    }

    let _ = evs::send_event(
        HK_LAB_STARTUP_EID,
        EventType::Information,
        "HK_LAB App Initialized. Version 0.1.0",
    );

    Ok(pipe)
}

/// Runs the `hk_lab` initialization, Software Bus dispatch loop, and teardown.
fn app_main() {
    let Ok(mut pipe) = init_app() else {
        es::exit_app(ExitStatus::AppError);
        return;
    };

    // Initialize housekeeping state.
    let mut downlink = Downlink::new();
    let mut status = Status::default();
    let mut hk_slot = 0_u32;

    // 6. Run ES loop, pending on the pipe.
    let mut run_status = RunStatus::AppRun;
    let mut pipe_guard = PipeRecvGuard::new();
    while es::run_loop(&mut run_status) {
        let buf = match pipe.receive_buffer(PipeRecvGuard::RECV_TIMEOUT) {
            Ok(buf) => {
                pipe_guard.on_ok();
                buf
            }
            Err(status) => {
                if pipe_guard.on_err(status, "HK_LAB: Pipe receive failed") == RecvAction::Abort {
                    run_status = RunStatus::AppError;
                    break;
                }
                continue;
            }
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
    es::exit_app(ExitStatus::from_run_status(run_status));
}

/// Application entry point exported to cFE.
#[expect(unsafe_code, reason = "cFS entry point must be extern \"C\"")]
#[unsafe(no_mangle)]
pub extern "C" fn HK_LAB_AppMain() {
    app_main();
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo<'_>) -> ! {
    es::exit_app(ExitStatus::AppError);
    loop {
        core::hint::spin_loop();
    }
}
