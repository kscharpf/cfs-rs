//! `pl_lab`: periodic payload telemetry app.

#![cfg_attr(not(test), no_std)]
#![deny(unsafe_code)]

use aldis::spp::SequenceFlags;
use cfe::es::{self, RunStatus};
use cfe::evs::{self, EventType};
use cfe::sb::{MsgId, Pipe, Qos, Timeout};
use node_glue::{send_done, send_record_event, send_tlm_packet};
use refapp_flight::event::APP_PACKET_REJECTED;
use refapp_flight::layout::{PayloadMode, payload};
use refapp_flight::schedule::Periodic;
use refapp_flight::{Downlink, Stream};
use refapp_scenario::{Apid, Instant, Output};
use step_lab_core::msg::{ScnPayloadMode, ScnStatusMsg, WakeTurn};

/// Event ID for `pl_lab` start-up event.
pub const PL_LAB_STARTUP_EID: u16 = 1;

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

/// Emits all payload packets due up to `now` when payload mode is `On`, and returns the next deadline.
fn handle_wake_pl(
    downlink: &mut Downlink,
    payload_mode: PayloadMode,
    payload_slot: &mut u32,
    payload_number: &mut u32,
    now: Instant,
) -> Option<u64> {
    let mut sink = PlSink;
    while let Some(at) = Periodic::PAYLOAD.at(*payload_slot).filter(|at| *at <= now) {
        if payload_mode == PayloadMode::On {
            let data = payload(*payload_number);
            if let Err(e) = downlink.send(
                Stream::Payload,
                SequenceFlags::Unsegmented,
                at,
                &data,
                &mut sink,
            ) {
                send_record_event(APP_PACKET_REJECTED, &[u64::from(e.kind().code())]);
            }
            *payload_number = payload_number.saturating_add(1);
        }
        *payload_slot = payload_slot.saturating_add(1);
    }
    Periodic::PAYLOAD.at(*payload_slot).map(Instant::as_micros)
}

/// Initializes `pl_lab`: registers with EVS, creates the SB pipe, subscribes, and emits startup event.
fn init_app() -> Result<Pipe, ()> {
    if evs::register().is_err() {
        return Err(());
    }

    let Ok(pipe) = Pipe::create(32, "PL_LAB_PIPE") else {
        let _ = evs::send_event(2, EventType::Critical, "PL_LAB: Failed to create SB pipe");
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
            .subscribe_ex(MsgId::from_u32(cfe_sys::NODE_WAKE_PL_MID), Qos::DEFAULT, 4)
            .is_err()
    {
        let _ = evs::send_event(
            3,
            EventType::Critical,
            "PL_LAB: Failed to subscribe to turns",
        );
        let _ = pipe.delete();
        return Err(());
    }

    let _ = evs::send_event(
        PL_LAB_STARTUP_EID,
        EventType::Information,
        "PL_LAB App Initialized. Version 0.1.0",
    );

    Ok(pipe)
}

/// Runs the `pl_lab` initialization, Software Bus dispatch loop, and teardown.
fn app_main() {
    let Ok(mut pipe) = init_app() else {
        es::exit_app(RunStatus::AppError);
        return;
    };

    // Initialize payload state.
    let mut downlink = Downlink::new();
    let mut payload_mode = PayloadMode::default();
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
                payload_mode = PayloadMode::default();
                payload_slot = 0;
                payload_number = 0;
                send_done(None);
            }
            cfe_sys::NODE_SCN_STATUS_MID => {
                if let Some(status) =
                    ScnStatusMsg::from_bytes(buf.as_bytes()).and_then(|msg| msg.status())
                {
                    payload_mode = match status.payload_mode {
                        ScnPayloadMode::Off => PayloadMode::Off,
                        ScnPayloadMode::On => PayloadMode::On,
                    };
                } else {
                    let _ = evs::send_event(
                        12,
                        EventType::Error,
                        "PL_LAB: Malformed ScnStatusMsg received",
                    );
                }
            }
            cfe_sys::NODE_WAKE_PL_MID => {
                let next_deadline = WakeTurn::Pl
                    .decode(mid.as_u32(), buf.as_bytes())
                    .map_or_else(
                        || {
                            let _ = evs::send_event(
                                13,
                                EventType::Error,
                                "PL_LAB: Malformed WakeTimeMsg received",
                            );
                            None
                        },
                        |now_us| {
                            handle_wake_pl(
                                &mut downlink,
                                payload_mode,
                                &mut payload_slot,
                                &mut payload_number,
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
pub extern "C" fn PL_LAB_AppMain() {
    app_main();
}

#[cfg(not(test))]
#[panic_handler]
const fn panic(_info: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}
