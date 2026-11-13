//! `link_lab`: cFS application carrying Space Packets over CCSDS TM and TC frames.

#![cfg_attr(not(test), no_std)]
#![deny(unsafe_code)]

use cfe::es::{self, RunStatus};
use cfe::evs::{self, EventType};
use cfe::sb::{Pipe, Timeout};

/// Event ID for `link_lab` start-up event.
pub const LINK_LAB_STARTUP_EID: u16 = 1;

/// Application entry point exported to cFE.
#[expect(unsafe_code, reason = "cFS entry point must be extern \"C\"")]
#[unsafe(no_mangle)]
pub extern "C" fn LINK_LAB_AppMain() {
    // 1. Register with EVS.
    if evs::register().is_err() {
        es::exit_app(RunStatus::AppError);
        return;
    }

    // 2. Create Software Bus input pipe.
    let Ok(mut pipe) = Pipe::create(8, "LINK_LAB_PIPE") else {
        let _ = evs::send_event(2, EventType::Critical, "LINK_LAB: Failed to create SB pipe");
        es::exit_app(RunStatus::AppError);
        return;
    };

    // 3. Send start-up event.
    let _ = evs::send_event(
        LINK_LAB_STARTUP_EID,
        EventType::Information,
        "LINK_LAB App Initialized. Version 0.1.0",
    );

    // 4. Run ES loop, pending on the pipe.
    let mut run_status = RunStatus::AppRun;
    while es::run_loop(&mut run_status) {
        let _ = pipe.receive_buffer(Timeout::PendForever);
    }

    let _ = pipe.delete();
    es::exit_app(run_status);
}

#[cfg(not(test))]
#[panic_handler]
const fn panic(_info: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}
