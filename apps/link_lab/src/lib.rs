//! `link_lab`: cFS application carrying Space Packets over CCSDS TM and TC frames.

#![cfg_attr(not(test), no_std)]
#![deny(unsafe_code)]

use cfe::es::{self, RunStatus};
use cfe::evs::{self, EventType};
use cfe::sb::{MsgId, Pipe, Qos, Timeout, transmit_msg};
use cfe::tbl::{Table, TableOptions};
use link_lab_core::{DownlinkSink, Instant, Level, Link, LinkConfig, UplinkSink, packet_apid};
use step_lab_core::msg::{
    CmdPacketMsg, DoneMsg, LinkLevel, MAX_UNIT_LEN, RecordMsg, StartMsg, TlmPacketMsg, UnitMsg,
    WakeRxMsg, WakeTxMsg,
};

/// Event ID for `link_lab` start-up event.
pub const LINK_LAB_STARTUP_EID: u16 = 1;

/// Raw cFE table name registered by `link_lab` (`LINK_LAB.LinkCfg`).
const LINK_CFG_TABLE_NAME: &str = "LinkCfg";

/// Default path to the compiled `LinkCfg` table image.
const LINK_CFG_TABLE_PATH: &str = "/cf/link_lab_cfg.tbl";

/// Maximum number of pending telemetry packets to buffer during a step.
const MAX_PENDING_TLM: usize = 16;

#[derive(Clone, Copy)]
struct PendingTlm {
    len: u16,
    data: [u8; MAX_UNIT_LEN],
}

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
        let _ = evs::send_event(4, EventType::Critical, "LINK_LAB: Failed to init DONE msg");
        return;
    }
    if transmit_msg(done.msg(), true).is_err() {
        let _ = evs::send_event(
            5,
            EventType::Critical,
            "LINK_LAB: Failed to transmit DONE msg",
        );
    }
}

/// Sends a `CMD_PACKET` message to `scn_lab`.
fn send_cmd_packet(now: u64, packet: &[u8]) {
    let Some(mut msg) = CmdPacketMsg::new(now, packet) else {
        let _ = evs::send_event(
            6,
            EventType::Critical,
            "LINK_LAB: Packet too long for CmdPacketMsg",
        );
        return;
    };
    if prepare_msg(
        msg.msg_mut(),
        cfe_sys::NODE_CMD_PACKET_MID,
        core::mem::size_of::<CmdPacketMsg>(),
    )
    .is_err()
    {
        let _ = evs::send_event(
            7,
            EventType::Critical,
            "LINK_LAB: Failed to init CMD_PACKET msg",
        );
        return;
    }
    if transmit_msg(msg.msg(), true).is_err() {
        let _ = evs::send_event(
            8,
            EventType::Critical,
            "LINK_LAB: Failed to transmit CMD_PACKET msg",
        );
    }
}

/// Sends a `UNIT` message to `step_lab`.
fn send_unit(unit: &[u8]) {
    let Some(mut msg) = UnitMsg::new(unit) else {
        let _ = evs::send_event(
            9,
            EventType::Critical,
            "LINK_LAB: Unit too long for UnitMsg",
        );
        return;
    };
    if prepare_msg(
        msg.msg_mut(),
        cfe_sys::NODE_UNIT_MID,
        core::mem::size_of::<UnitMsg>(),
    )
    .is_err()
    {
        let _ = evs::send_event(10, EventType::Critical, "LINK_LAB: Failed to init UNIT msg");
        return;
    }
    if transmit_msg(msg.msg(), true).is_err() {
        let _ = evs::send_event(
            11,
            EventType::Critical,
            "LINK_LAB: Failed to transmit UNIT msg",
        );
    }
}

/// Sends a `RECORD` event message to `step_lab`.
fn send_record_event(code: u16, args: &[u64]) {
    let Some(mut msg) = RecordMsg::new_event(code, args) else {
        let _ = evs::send_event(
            12,
            EventType::Critical,
            "LINK_LAB: Too many args for RecordMsg",
        );
        return;
    };
    if prepare_msg(
        msg.msg_mut(),
        cfe_sys::NODE_RECORD_MID,
        core::mem::size_of::<RecordMsg>(),
    )
    .is_err()
    {
        let _ = evs::send_event(
            13,
            EventType::Critical,
            "LINK_LAB: Failed to init RECORD msg",
        );
        return;
    }
    if transmit_msg(msg.msg(), true).is_err() {
        let _ = evs::send_event(
            14,
            EventType::Critical,
            "LINK_LAB: Failed to transmit RECORD msg",
        );
    }
}

/// Sends a `RECORD` `packet_out` message to `step_lab`.
fn send_record_packet_out(apid: u16, packet: &[u8]) {
    let Some(mut msg) = RecordMsg::new_packet_out(apid, packet) else {
        let _ = evs::send_event(
            15,
            EventType::Critical,
            "LINK_LAB: Packet too long for RecordMsg",
        );
        return;
    };
    if prepare_msg(
        msg.msg_mut(),
        cfe_sys::NODE_RECORD_MID,
        core::mem::size_of::<RecordMsg>(),
    )
    .is_err()
    {
        let _ = evs::send_event(
            16,
            EventType::Critical,
            "LINK_LAB: Failed to init RECORD msg",
        );
        return;
    }
    if transmit_msg(msg.msg(), true).is_err() {
        let _ = evs::send_event(
            17,
            EventType::Critical,
            "LINK_LAB: Failed to transmit RECORD msg",
        );
    }
}

/// Downlink sink emitting units and events to `step_lab`.
struct LinkDownlinkSink;

impl DownlinkSink for LinkDownlinkSink {
    fn unit(&mut self, unit: &[u8]) {
        send_unit(unit);
    }

    fn event(&mut self, code: u16, args: &[u64]) {
        send_record_event(code, args);
    }
}

/// Uplink sink exchanging command packets with `scn_lab`.
struct LinkUplinkSink<'a> {
    pipe: &'a mut Pipe,
    now: Instant,
    pending_tlm: &'a mut [Option<PendingTlm>; MAX_PENDING_TLM],
    pending_count: &'a mut usize,
}

impl UplinkSink for LinkUplinkSink<'_> {
    fn command_packet(&mut self, packet: &[u8]) {
        send_cmd_packet(self.now.as_micros(), packet);
        while let Ok(buf) = self.pipe.receive_buffer(Timeout::PendForever) {
            let Ok(mid) = buf.msg_id() else {
                continue;
            };
            match mid.as_u32() {
                cfe_sys::NODE_CMD_PACKET_DONE_MID => break,
                cfe_sys::NODE_TLM_PACKET_MID => {
                    handle_tlm_packet(buf.as_bytes(), self.pending_tlm, self.pending_count);
                }
                _ => {
                    let _ = evs::send_event(
                        22,
                        EventType::Error,
                        "LINK_LAB: Unexpected MID during command packet exchange",
                    );
                }
            }
        }
    }

    fn event(&mut self, code: u16, args: &[u64]) {
        send_record_event(code, args);
    }
}

/// Handles `WAKE_RX` turn from `step_lab`.
fn handle_wake_rx(
    link: &mut Link,
    pipe: &mut Pipe,
    wake_rx: Option<&WakeRxMsg>,
    pending_tlm: &mut [Option<PendingTlm>; MAX_PENDING_TLM],
    pending_count: &mut usize,
) {
    if let Some(wake_rx) = wake_rx {
        let now = Instant::from_micros(wake_rx.now);
        let unit = wake_rx.unit_bytes();
        let mut rx_sink = LinkUplinkSink {
            pipe,
            now,
            pending_tlm,
            pending_count,
        };
        link.accept_unit(now, unit, &mut rx_sink);
    }
    send_done(None);
}

/// Handles incoming `TLM_PACKET` from `scn_lab`, buffering it for `WAKE_TX`.
fn handle_tlm_packet(
    bytes: &[u8],
    pending_tlm: &mut [Option<PendingTlm>; MAX_PENDING_TLM],
    pending_count: &mut usize,
) {
    if let Some(tlm) = TlmPacketMsg::from_bytes(bytes) {
        let pkt = tlm.packet_bytes();
        if pkt.len() > MAX_UNIT_LEN {
            let _ = evs::send_event(
                19,
                EventType::Critical,
                "LINK_LAB: Telemetry packet exceeds MAX_UNIT_LEN",
            );
            return;
        }
        if *pending_count < MAX_PENDING_TLM {
            let mut p = PendingTlm {
                len: tlm.packet_len,
                data: [0; MAX_UNIT_LEN],
            };
            let copy_len = pkt.len().min(MAX_UNIT_LEN);
            if let Some(src) = pkt.get(..copy_len)
                && let Some(dest) = p.data.get_mut(..copy_len)
            {
                dest.copy_from_slice(src);
                p.len = u16::try_from(copy_len).unwrap_or(0);
                if let Some(slot) = pending_tlm.get_mut(*pending_count) {
                    *slot = Some(p);
                    *pending_count = pending_count.saturating_add(1);
                }
            }
        } else {
            let _ = evs::send_event(
                18,
                EventType::Critical,
                "LINK_LAB: pending_tlm queue overflow",
            );
        }
    }
}

/// Records and submits a telemetry packet to the link layer.
fn accept_tlm_packet(link: &mut Link, tx_sink: &mut LinkDownlinkSink, now: Instant, pkt: &[u8]) {
    let apid = packet_apid(pkt).unwrap_or(0);
    send_record_packet_out(apid, pkt);
    link.accept_packet(now, pkt, tx_sink);
}

/// Handles `WAKE_TX` turn from `step_lab`.
fn handle_wake_tx(
    link: &mut Link,
    pipe: &mut Pipe,
    wake_tx: Option<&WakeTxMsg>,
    pending_tlm: &mut [Option<PendingTlm>; MAX_PENDING_TLM],
    pending_count: &mut usize,
) {
    if let Some(wake_tx) = wake_tx {
        let now = Instant::from_micros(wake_tx.now);
        let mut tx_sink = LinkDownlinkSink;

        // 1. Drain pending TLM packets buffered before WAKE_TX
        for slot in pending_tlm.iter_mut().take(*pending_count) {
            if let Some(p) = slot.take() {
                let pkt = p.data.get(..usize::from(p.len)).unwrap_or(&[]);
                accept_tlm_packet(link, &mut tx_sink, now, pkt);
            }
        }
        *pending_count = 0;

        // 2. Also drain any extra TLM packets in pipe
        while let Ok(extra_buf) = pipe.receive_buffer(Timeout::Poll) {
            let Ok(mid) = extra_buf.msg_id() else {
                continue;
            };
            if mid.as_u32() == cfe_sys::NODE_TLM_PACKET_MID {
                if let Some(tlm) = TlmPacketMsg::from_bytes(extra_buf.as_bytes()) {
                    let pkt = tlm.packet_bytes();
                    if pkt.len() > MAX_UNIT_LEN {
                        let _ = evs::send_event(
                            19,
                            EventType::Critical,
                            "LINK_LAB: Telemetry packet exceeds MAX_UNIT_LEN",
                        );
                    } else {
                        accept_tlm_packet(link, &mut tx_sink, now, pkt);
                    }
                }
            } else {
                let _ = evs::send_event(
                    21,
                    EventType::Error,
                    "LINK_LAB: Unexpected MID in pipe during WAKE_TX drain",
                );
            }
        }

        // 3. Transmit slot
        link.slot(now, wake_tx.is_tx_slot(), &mut tx_sink);

        // 4. Send DONE with next deadline
        let next_d = link.next_deadline(now).map(Instant::as_micros);
        send_done(next_d);
    } else {
        send_done(None);
    }
}

/// Subscribes the input pipe to all relevant Software Bus messages.
fn subscribe_turns(pipe: &Pipe) -> Result<(), ()> {
    if pipe
        .subscribe_ex(MsgId::from_u32(cfe_sys::NODE_START_MID), Qos::DEFAULT, 4)
        .is_err()
        || pipe
            .subscribe_ex(MsgId::from_u32(cfe_sys::NODE_WAKE_RX_MID), Qos::DEFAULT, 32)
            .is_err()
        || pipe
            .subscribe_ex(MsgId::from_u32(cfe_sys::NODE_WAKE_TX_MID), Qos::DEFAULT, 4)
            .is_err()
        || pipe
            .subscribe_ex(
                MsgId::from_u32(cfe_sys::NODE_CMD_PACKET_DONE_MID),
                Qos::DEFAULT,
                4,
            )
            .is_err()
        || pipe
            .subscribe_ex(
                MsgId::from_u32(cfe_sys::NODE_TLM_PACKET_MID),
                Qos::DEFAULT,
                32,
            )
            .is_err()
    {
        return Err(());
    }
    Ok(())
}

/// Reads and decodes [`LinkConfig`] from the registered cFE table.
fn read_link_config(table: &mut Table) -> Result<LinkConfig, ()> {
    let Ok(parsed) = table.read_bytes(LinkConfig::from_table_bytes) else {
        let _ = evs::send_event(
            28,
            EventType::Critical,
            "LINK_LAB: Failed to read LinkCfg table bytes",
        );
        return Err(());
    };
    let Ok(config) = parsed else {
        let _ = evs::send_event(
            29,
            EventType::Critical,
            "LINK_LAB: Invalid LinkCfg table contents",
        );
        return Err(());
    };
    Ok(config)
}

/// Initializes `link_lab`: registers with EVS, creates pipe, subscribes, loads `LinkCfg`, and sends start-up event.
fn init_app() -> Result<(Pipe, Table, LinkConfig), ()> {
    if evs::register().is_err() {
        return Err(());
    }

    let Ok(pipe) = Pipe::create(32, "LINK_LAB_PIPE") else {
        let _ = evs::send_event(2, EventType::Critical, "LINK_LAB: Failed to create SB pipe");
        return Err(());
    };

    if subscribe_turns(&pipe).is_err() {
        let _ = evs::send_event(
            3,
            EventType::Critical,
            "LINK_LAB: Failed to subscribe to turns",
        );
        let _ = pipe.delete();
        return Err(());
    }

    let Some(table_size) = core::num::NonZeroUsize::new(LinkConfig::TABLE_SIZE) else {
        let _ = pipe.delete();
        return Err(());
    };
    let Ok(mut table) = Table::register(LINK_CFG_TABLE_NAME, table_size, TableOptions::Default)
    else {
        let _ = evs::send_event(
            25,
            EventType::Critical,
            "LINK_LAB: Failed to register LinkCfg table",
        );
        let _ = pipe.delete();
        return Err(());
    };

    if table.load_from_file(LINK_CFG_TABLE_PATH).is_err() {
        let _ = evs::send_event(
            26,
            EventType::Critical,
            "LINK_LAB: Failed to load /cf/link_lab_cfg.tbl",
        );
        let _ = pipe.delete();
        return Err(());
    }

    if table.manage().is_err() {
        let _ = evs::send_event(
            27,
            EventType::Critical,
            "LINK_LAB: Failed to manage LinkCfg table",
        );
        let _ = pipe.delete();
        return Err(());
    }

    let Ok(config) = read_link_config(&mut table) else {
        let _ = pipe.delete();
        return Err(());
    };

    let _ = evs::send_event(
        LINK_LAB_STARTUP_EID,
        EventType::Information,
        "LINK_LAB App Initialized. Version 0.1.0",
    );

    Ok((pipe, table, config))
}

/// Handles `NODE_START` turn: refreshes `LinkCfg` if updated and initializes `Link`.
fn handle_start(
    table: &mut Table,
    config: &mut LinkConfig,
    bytes: &[u8],
    link: &mut Option<Link>,
    pending_count: &mut usize,
) {
    let candidate = match table.manage() {
        Ok(true) => read_link_config(table).ok(),
        Ok(false) => None,
        Err(_) => {
            let _ = evs::send_event(
                27,
                EventType::Error,
                "LINK_LAB: Failed to manage LinkCfg table",
            );
            None
        }
    };
    if let Some(lvl) = StartMsg::from_bytes(bytes).and_then(|m| m.link_level()) {
        let level = match lvl {
            LinkLevel::Packets => Level::Packets,
            LinkLevel::Frames => Level::Frames,
            LinkLevel::Cop1 => Level::Cop1,
        };
        let active_config = candidate.as_ref().unwrap_or(config);
        if let Ok(new_link) = Link::new(level, active_config) {
            if let Some(updated) = candidate {
                *config = updated;
            }
            *link = Some(new_link);
        } else {
            *link = None;
            let _ = evs::send_event(
                20,
                EventType::Critical,
                "LINK_LAB: Failed to initialize Link",
            );
        }
    } else {
        *link = None;
        let _ = evs::send_event(
            24,
            EventType::Critical,
            "LINK_LAB: Invalid or malformed StartMsg",
        );
    }
    *pending_count = 0;
    send_done(None);
}

/// Runs the `link_lab` initialization, Software Bus dispatch loop, and teardown.
fn app_main() {
    let Ok((mut pipe, mut table, mut config)) = init_app() else {
        es::exit_app(RunStatus::AppError);
        return;
    };

    let mut link: Option<Link> = None;
    #[expect(
        clippy::large_stack_arrays,
        reason = "cFS app task stack is sized to 64 KiB in cpu1_cfe_es_startup.scr; 16.5 KiB fits safely"
    )]
    let mut pending_tlm = [const { None }; MAX_PENDING_TLM];
    let mut pending_count = 0usize;

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
                handle_start(
                    &mut table,
                    &mut config,
                    buf.as_bytes(),
                    &mut link,
                    &mut pending_count,
                );
            }
            cfe_sys::NODE_WAKE_RX_MID => {
                let wake_rx = WakeRxMsg::from_bytes(buf.as_bytes());
                if let Some(link) = link.as_mut() {
                    handle_wake_rx(
                        link,
                        &mut pipe,
                        wake_rx.as_ref(),
                        &mut pending_tlm,
                        &mut pending_count,
                    );
                } else {
                    send_done(None);
                }
            }
            cfe_sys::NODE_TLM_PACKET_MID => {
                handle_tlm_packet(buf.as_bytes(), &mut pending_tlm, &mut pending_count);
            }
            cfe_sys::NODE_WAKE_TX_MID => {
                let wake_tx = WakeTxMsg::from_bytes(buf.as_bytes());
                if let Some(link) = link.as_mut() {
                    handle_wake_tx(
                        link,
                        &mut pipe,
                        wake_tx.as_ref(),
                        &mut pending_tlm,
                        &mut pending_count,
                    );
                } else {
                    send_done(None);
                }
            }
            _ => {
                let _ = evs::send_event(
                    23,
                    EventType::Error,
                    "LINK_LAB: Unexpected MID received on main pipe",
                );
            }
        }
    }

    let _ = table.unregister();
    let _ = pipe.delete();
    es::exit_app(run_status);
}

/// Application entry point exported to cFE.
#[expect(unsafe_code, reason = "cFS entry point must be extern \"C\"")]
#[unsafe(no_mangle)]
pub extern "C" fn LINK_LAB_AppMain() {
    app_main();
}

#[cfg(not(test))]
#[panic_handler]
const fn panic(_info: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}
