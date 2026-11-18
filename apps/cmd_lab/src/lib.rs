//! `cmd_lab`: scenario command handler, time-tag queue, table store, and memory dump app.

#![cfg_attr(not(test), no_std)]
#![deny(unsafe_code)]

use aldis::core::ErrorKind;
use aldis::spp::{PacketType, SequenceFlags, SpacePacket};
use cfe::es::{self, RunStatus};
use cfe::evs::{self, EventType};
use cfe::sb::{MsgId, Pipe, Qos, Timeout};
use node_glue::{send_done, send_record_event, send_sb_msg, send_tlm_packet};
use refapp_flight::command::{self, Call, Command, Reject, Timing};
use refapp_flight::dump::Dump;
use refapp_flight::event::{APP_CMD_RECEIVED, APP_PACKET_REJECTED, APP_TABLE_RECEIVED, Event};
use refapp_flight::layout::{PayloadMode, Status};
use refapp_flight::queue::TimeTagQueue;
use refapp_flight::table::{Activation, TableStore};
use refapp_flight::{Downlink, Stream};
use refapp_scenario::generated::apid;
use refapp_scenario::{Apid, Instant, Output};
use step_lab_core::msg::{
    CmdPacketDoneMsg, CmdPacketMsg, ScnPayloadMode, ScnStatus, ScnStatusMsg, WakeTurn,
};

/// Event ID for `cmd_lab` start-up event.
pub const CMD_LAB_STARTUP_EID: u16 = 1;

/// Sends a `CMD_PACKET_DONE` message to `link_lab` signaling command packet processing complete.
fn send_cmd_packet_done() {
    let mut done = CmdPacketDoneMsg::new();
    let _ = send_sb_msg(
        &mut done,
        6,
        "CMD_LAB: Failed to init CMD_PACKET_DONE msg",
        7,
        "CMD_LAB: Failed to transmit CMD_PACKET_DONE msg",
    );
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
        payload_mode: match status.payload_mode {
            PayloadMode::Off => ScnPayloadMode::Off,
            PayloadMode::On => ScnPayloadMode::On,
        },
    });
    let _ = send_sb_msg(
        &mut msg,
        14,
        "CMD_LAB: Failed to init SCN_STATUS msg",
        15,
        "CMD_LAB: Failed to transmit SCN_STATUS msg",
    );
}

/// Output sink for command/dump execution inside `cmd_lab`.
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

/// Command handler, time-tag queue, table store, and memory dump state for `cmd_lab`.
#[derive(Default)]
struct CmdApp {
    downlink: Downlink,
    status: Status,
    queue: TimeTagQueue,
    table: TableStore,
    dump: Option<Dump>,
}

impl CmdApp {
    /// Handles one delivered command or table-upload Space Packet.
    fn handle_packet(&mut self, now: Instant, unit: &[u8], out: &mut dyn Output) {
        let packet = match SpacePacket::decode_exact(unit) {
            Ok(packet) => packet,
            Err(e) => return reject_unit(out, e.kind()),
        };
        if packet.packet_type() != PacketType::Telecommand
            || packet.sequence_flags() != SequenceFlags::Unsegmented
            || packet.has_secondary_header()
        {
            return reject_unit(out, ErrorKind::InvalidField);
        }
        let data = packet.data_field();
        match packet.apid().get() {
            apid::COMMANDS => {
                self.deliver_command(now, command::parse(packet.sequence_count(), data), out);
            }
            apid::TABLE_UPLOAD => self.deliver_table(data, out),
            _ => reject_unit(out, ErrorKind::UnknownChannel),
        }
    }

    fn deliver_command(&mut self, now: Instant, command: Command, out: &mut dyn Output) {
        out.app_event(
            APP_CMD_RECEIVED,
            &[u64::from(command.function), u64::from(command.seq)],
        );
        match command.timing {
            Timing::Immediate => self.execute(now, command, out),
            Timing::BeforeRun => self.reject(now, &command, Reject::TimeTagPast, out),
            Timing::At(at) if at < now => {
                self.reject(now, &command, Reject::TimeTagPast, out);
            }
            Timing::At(at) => {
                if let Err(reason) = self.queue.push(at, command) {
                    self.reject(now, &command, reason, out);
                }
            }
        }
    }

    fn deliver_table(&mut self, data: &[u8], out: &mut dyn Output) {
        match self.table.store(data) {
            Ok((table_id, length)) => {
                out.app_event(
                    APP_TABLE_RECEIVED,
                    &[u64::from(table_id.get()), u64::from(length.get())],
                );
            }
            Err(e) => reject_unit(out, e.kind()),
        }
    }

    /// Executes due time-tagged commands and updates queue occupancy in `status`.
    fn execute_due(&mut self, now: Instant, out: &mut dyn Output) {
        while let Some(command) = self.queue.pop_due(now) {
            self.execute(now, command, out);
        }
        self.status.queue_occupancy = self.queue.occupancy();
    }

    fn execute(&mut self, now: Instant, command: Command, out: &mut dyn Output) {
        let call = match command.call {
            Ok(call) => call,
            Err(reason) => return self.reject(now, &command, reason, out),
        };
        if matches!(call, Call::DumpMemory(_)) && self.dump.is_some() {
            return self.reject(now, &command, Reject::DumpInProgress, out);
        }
        self.status.commands_executed = self.status.commands_executed.wrapping_add(1);
        self.report(
            now,
            Event::CmdExecuted {
                function: command.function,
                seq: command.seq,
            },
            out,
        );
        match call {
            Call::Noop => {}
            Call::DumpMemory(region) => match Dump::start(region, now) {
                Ok(dump) => self.dump = Some(dump),
                Err(e) => reject_unit(out, e.kind()),
            },
            Call::ActivateTable { table_id, crc } => {
                let event = match self.table.activate(table_id, crc) {
                    Activation::NoTable => Event::TableRejected {
                        table_id: u16::from(table_id),
                        reason: 1,
                    },
                    Activation::CrcMismatch => Event::TableRejected {
                        table_id: u16::from(table_id),
                        reason: 2,
                    },
                    Activation::Activated { length, crc } => {
                        self.status.table_id = table_id;
                        self.status.table_crc = crc.get();
                        Event::TableActivated {
                            table_id: u16::from(table_id),
                            length: length.get(),
                            crc: crc.get(),
                        }
                    }
                };
                self.report(now, event, out);
            }
            Call::SetPayloadMode(mode) => {
                self.status.payload_mode = mode;
                self.report(now, Event::PayloadMode { mode }, out);
            }
        }
    }

    fn reject(&mut self, now: Instant, command: &Command, reason: Reject, out: &mut dyn Output) {
        self.status.commands_rejected = self.status.commands_rejected.wrapping_add(1);
        self.report(
            now,
            Event::CmdRejected {
                function: command.function,
                seq: command.seq,
                reason: reason.reason(),
            },
            out,
        );
    }

    /// Emits all memory dump segments due up to `now`.
    fn dump_segments(&mut self, now: Instant, out: &mut dyn Output) {
        let Some(mut dump) = self.dump.take() else {
            return;
        };
        loop {
            if dump.due() > now {
                self.dump = Some(dump);
                return;
            }
            match dump.send(&mut self.downlink, out) {
                Ok(segment) if segment.last => {
                    self.status.dumps_completed = self.status.dumps_completed.wrapping_add(1);
                    let region = dump.region();
                    self.report(
                        segment.at,
                        Event::DumpCompleted {
                            address: region.address(),
                            length: region.length(),
                        },
                        out,
                    );
                    return;
                }
                Ok(_) => {}
                Err(e) => return reject_unit(out, e.kind()),
            }
        }
    }

    fn report(&mut self, now: Instant, event: Event, out: &mut dyn Output) {
        if let Err(e) = self.downlink.send(
            Stream::Events,
            SequenceFlags::Unsegmented,
            now,
            &event.user_data(),
            out,
        ) {
            reject_unit(out, e.kind());
        }
    }

    /// Returns the earliest next deadline from the time-tag queue or active dump.
    fn next_deadline(&self) -> Option<u64> {
        [self.queue.next_due(), self.dump.as_ref().map(Dump::due)]
            .into_iter()
            .flatten()
            .min()
            .map(Instant::as_micros)
    }
}

fn reject_unit(out: &mut dyn Output, kind: ErrorKind) {
    out.app_event(APP_PACKET_REJECTED, &[u64::from(kind.code())]);
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

    // 5. Initialize command/dump state.
    let mut cmd_app = CmdApp::default();

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
                cmd_app = CmdApp::default();
                send_scn_status(&cmd_app.status);
                send_done(None);
            }
            cfe_sys::NODE_CMD_PACKET_MID => {
                if let Some(cmd) = CmdPacketMsg::from_bytes(buf.as_bytes()) {
                    let now = Instant::from_micros(cmd.now);
                    let pkt = cmd.packet_bytes();
                    let mut sink = CmdSink;
                    cmd_app.handle_packet(now, pkt, &mut sink);
                } else {
                    let _ = evs::send_event(
                        16,
                        EventType::Error,
                        "CMD_LAB: Malformed CmdPacketMsg received",
                    );
                }
                send_cmd_packet_done();
            }
            cfe_sys::NODE_WAKE_CMD_MID => {
                if let Some(wake) = WakeTurn::Cmd.decode(mid.as_u32(), buf.as_bytes()) {
                    let now = Instant::from_micros(wake.now);
                    let mut sink = CmdSink;
                    cmd_app.execute_due(now, &mut sink);
                    cmd_app.dump_segments(now, &mut sink);
                    send_scn_status(&cmd_app.status);
                    send_done(cmd_app.next_deadline());
                } else {
                    let _ = evs::send_event(
                        17,
                        EventType::Error,
                        "CMD_LAB: Malformed WakeTimeMsg received",
                    );
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
