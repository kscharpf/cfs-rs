//! `link_lab`: cFS application carrying Space Packets over CCSDS TM and TC frames.

#![cfg_attr(not(test), no_std)]
#![deny(unsafe_code)]

use core::net::Ipv4Addr;

use cfe::es::{self, ExitStatus, RunStatus};
use cfe::evs::{self, EventType};
use cfe::osal::{self, UdpSocket};
use cfe::sb::{MsgId, Pipe, Qos, Timeout};
use cfe::tbl::{Table, TableOptions};
use link_lab_core::{
    DownlinkSink, Instant, Level, Link, LinkConfig, LinkMode, MIN_COMMAND_PACKET_LEN,
    RealtimeConfig, TC_FRAME_AREA_LEN, UplinkSink, is_valid_command_packet, packet_apid,
};
use node_glue::{
    PeerWaitGuard, PipeRecvGuard, RecvAction, send_done, send_record_event, send_sb_msg,
};
use step_lab_core::msg::{
    CmdPacketMsg, LinkLevel, MAX_UNIT_LEN, RecordMsg, StartMsg, TlmPacketMsg, UnitMsg, WakeRxMsg,
    WakeTxMsg,
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

/// Sends a `CMD_PACKET` message to `cmd_lab`.
fn send_cmd_packet(now: u64, packet: &[u8]) -> Result<(), ()> {
    let Some(mut msg) = CmdPacketMsg::new(now, packet) else {
        let _ = evs::send_event(
            6,
            EventType::Critical,
            "LINK_LAB: Packet too long for CmdPacketMsg",
        );
        return Err(());
    };
    send_sb_msg(
        &mut msg,
        7,
        "LINK_LAB: Failed to init CMD_PACKET msg",
        8,
        "LINK_LAB: Failed to transmit CMD_PACKET msg",
    )
    .map_err(|_| ())
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
    let _ = send_sb_msg(
        &mut msg,
        10,
        "LINK_LAB: Failed to init UNIT msg",
        11,
        "LINK_LAB: Failed to transmit UNIT msg",
    );
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
    let _ = send_sb_msg(
        &mut msg,
        16,
        "LINK_LAB: Failed to init RECORD msg",
        17,
        "LINK_LAB: Failed to transmit RECORD msg",
    );
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

/// Uplink sink exchanging command packets with `cmd_lab`.
struct LinkUplinkSink<'a> {
    pipe: &'a mut Pipe,
    run_status: &'a mut RunStatus,
    now: Instant,
    pending_tlm: &'a mut [Option<PendingTlm>; MAX_PENDING_TLM],
    pending_count: &'a mut usize,
}

impl UplinkSink for LinkUplinkSink<'_> {
    fn command_packet(&mut self, packet: &[u8]) {
        if *self.run_status != RunStatus::AppRun {
            return;
        }
        if send_cmd_packet(self.now.as_micros(), packet).is_err() {
            *self.run_status = RunStatus::AppError;
            return;
        }
        let mut wait_guard = PeerWaitGuard::new();
        let mut completed = false;
        while es::run_loop(self.run_status) {
            let buf = match self.pipe.receive_buffer(PeerWaitGuard::POLL_TIMEOUT) {
                Ok(buf) => {
                    wait_guard.on_ok();
                    buf
                }
                Err(status) => {
                    if wait_guard.on_err(
                        status,
                        38,
                        "LINK_LAB: Timed out waiting for CMD_PACKET_DONE",
                        39,
                        "LINK_LAB: Pipe error waiting for CMD_PACKET_DONE",
                    ) == RecvAction::Abort
                    {
                        *self.run_status = RunStatus::AppError;
                        break;
                    }
                    continue;
                }
            };
            let Ok(mid) = buf.msg_id() else {
                continue;
            };
            match mid.as_u32() {
                cfe_sys::NODE_CMD_PACKET_DONE_MID => {
                    completed = true;
                    break;
                }
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
        if !completed && *self.run_status == RunStatus::AppRun {
            *self.run_status = RunStatus::AppError;
        }
    }

    fn event(&mut self, code: u16, args: &[u64]) {
        if *self.run_status == RunStatus::AppRun {
            send_record_event(code, args);
        }
    }
}

/// Handles `WAKE_RX` turn from `step_lab`.
fn handle_wake_rx(
    link: &mut Link,
    pipe: &mut Pipe,
    run_status: &mut RunStatus,
    wake_rx: Option<&WakeRxMsg>,
    pending_tlm: &mut [Option<PendingTlm>; MAX_PENDING_TLM],
    pending_count: &mut usize,
) {
    if let Some(wake_rx) = wake_rx {
        let now = Instant::from_micros(wake_rx.now);
        let unit = wake_rx.unit_bytes();
        let mut rx_sink = LinkUplinkSink {
            pipe,
            run_status,
            now,
            pending_tlm,
            pending_count,
        };
        link.accept_unit(now, unit, &mut rx_sink);
    }
    if *run_status == RunStatus::AppRun {
        send_done(None);
    }
}

/// Handles incoming `TLM_PACKET` from scenario apps, buffering it for `WAKE_TX`.
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

/// Subscribes the input pipe to native cFE telemetry MIDs and `NODE_WAKE_TX_MID` for `NativeRealtime` mode.
fn subscribe_realtime(pipe: &Pipe) -> Result<(), ()> {
    let mids = [
        (cfe_sys::NODE_WAKE_TX_MID, 4_u16),
        (cfe_sys::CFE_EVS_LONG_EVENT_MSG_MID, 16),
        (cfe_sys::CFE_EVS_SHORT_EVENT_MSG_MID, 16),
        (cfe_sys::CFE_ES_HK_TLM_MID, 8),
        (cfe_sys::CFE_EVS_HK_TLM_MID, 8),
        (cfe_sys::CFE_SB_HK_TLM_MID, 8),
        (cfe_sys::CFE_TIME_HK_TLM_MID, 8),
        (cfe_sys::CFE_TBL_HK_TLM_MID, 8),
    ];
    for (mid, limit) in mids {
        if pipe
            .subscribe_ex(MsgId::from_u32(mid), Qos::DEFAULT, limit)
            .is_err()
        {
            return Err(());
        }
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

/// Initializes `link_lab`: registers with EVS, creates pipe, loads `LinkCfg`, subscribes, and sends start-up event.
fn init_app() -> Result<(Pipe, Table, LinkConfig), ()> {
    if evs::register().is_err() {
        return Err(());
    }

    let Ok(pipe) = Pipe::create(32, "LINK_LAB_PIPE") else {
        let _ = evs::send_event(2, EventType::Critical, "LINK_LAB: Failed to create SB pipe");
        return Err(());
    };

    let Some(table_size) = core::num::NonZeroUsize::new(LinkConfig::TABLE_SIZE) else {
        return Err(());
    };
    let Ok(mut table) = Table::register(LINK_CFG_TABLE_NAME, table_size, TableOptions::Default)
    else {
        let _ = evs::send_event(
            25,
            EventType::Critical,
            "LINK_LAB: Failed to register LinkCfg table",
        );
        return Err(());
    };

    if table.load_from_file(LINK_CFG_TABLE_PATH).is_err() {
        let _ = evs::send_event(
            26,
            EventType::Critical,
            "LINK_LAB: Failed to load /cf/link_lab_cfg.tbl",
        );
        return Err(());
    }

    if table.manage().is_err() {
        let _ = evs::send_event(
            27,
            EventType::Critical,
            "LINK_LAB: Failed to manage LinkCfg table",
        );
        return Err(());
    }

    let Ok(config) = read_link_config(&mut table) else {
        return Err(());
    };

    let sub_result = match config.mode {
        LinkMode::Lockstep => subscribe_turns(&pipe),
        LinkMode::NativeRealtime(_) => subscribe_realtime(&pipe),
    };
    if sub_result.is_err() {
        let _ = evs::send_event(
            3,
            EventType::Critical,
            "LINK_LAB: Failed to subscribe to pipe MIDs",
        );
        return Err(());
    }

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
    *link = None;
    if let Some(lvl) = StartMsg::from_bytes(bytes).and_then(|m| m.link_level()) {
        let level = match lvl {
            LinkLevel::Packets => Level::Packets,
            LinkLevel::Frames => Level::Frames,
            LinkLevel::Cop1 => Level::Cop1,
        };
        let active_config = candidate.as_ref().unwrap_or(config);
        *link = Link::new(level, active_config).ok();
        if link.is_some() {
            if let Some(updated) = candidate {
                *config = updated;
            }
        } else {
            let _ = evs::send_event(
                20,
                EventType::Critical,
                "LINK_LAB: Failed to initialize Link",
            );
        }
    } else {
        let _ = evs::send_event(
            24,
            EventType::Critical,
            "LINK_LAB: Invalid or malformed StartMsg",
        );
    }
    *pending_count = 0;
    send_done(None);
}

/// Runs the lockstep `step_lab` dispatch loop (`a0`–`a3`).
#[inline(never)]
fn run_lockstep(pipe: &mut Pipe, table: &mut Table, config: &mut LinkConfig) -> ExitStatus {
    let mut link: Option<Link> = None;
    #[expect(
        clippy::large_stack_arrays,
        reason = "cFS app task stack is sized to 64 KiB in cpu1_cfe_es_startup.scr; 16.5 KiB fits safely"
    )]
    let mut pending_tlm = [const { None }; MAX_PENDING_TLM];
    let mut pending_count = 0usize;

    let mut run_status = RunStatus::AppRun;
    let mut pipe_guard = PipeRecvGuard::new();
    while es::run_loop(&mut run_status) {
        let buf = match pipe.receive_buffer(PipeRecvGuard::RECV_TIMEOUT) {
            Ok(buf) => {
                pipe_guard.on_ok();
                buf
            }
            Err(status) => {
                if pipe_guard.on_err(status, "LINK_LAB: Pipe receive failed") == RecvAction::Abort {
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
                handle_start(table, config, buf.as_bytes(), &mut link, &mut pending_count);
            }
            cfe_sys::NODE_WAKE_RX_MID => {
                let wake_rx = WakeRxMsg::from_bytes(buf.as_bytes());
                if let Some(link) = link.as_mut() {
                    handle_wake_rx(
                        link,
                        pipe,
                        &mut run_status,
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
                        pipe,
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
    ExitStatus::from_run_status(run_status)
}

const _: () =
    assert!(MIN_COMMAND_PACKET_LEN == core::mem::size_of::<cfe_sys::CFE_MSG_CommandHeader_t>());

const RX_BUF_LEN: usize = TC_FRAME_AREA_LEN.saturating_add(1);
const _: () = assert!(RX_BUF_LEN == 129);
const _: () = assert!(RX_BUF_LEN > TC_FRAME_AREA_LEN);

struct RealtimeUplinkSink;

impl UplinkSink for RealtimeUplinkSink {
    fn command_packet(&mut self, packet: &[u8]) {
        if !is_valid_command_packet(packet) {
            let _ = evs::send_event(
                32,
                EventType::Error,
                "LINK_LAB: Rejected non-command or undersized uplink packet",
            );
            return;
        }
        let Ok(mut msg_buf) = cfe::sb::allocate_message_buffer(packet.len()) else {
            let _ = evs::send_event(
                33,
                EventType::Error,
                "LINK_LAB: Failed to allocate SB buffer for command packet",
            );
            return;
        };
        let Some(dest) = msg_buf.as_bytes_mut().get_mut(..packet.len()) else {
            let _ = evs::send_event(
                33,
                EventType::Error,
                "LINK_LAB: Allocated SB buffer smaller than command packet",
            );
            return;
        };
        dest.copy_from_slice(packet);
        if cfe::sb::transmit_buffer(msg_buf, false).is_err() {
            let _ = evs::send_event(
                34,
                EventType::Error,
                "LINK_LAB: Failed to transmit uplink command packet on SB",
            );
        }
    }
}

struct RealtimeDownlinkSink<'a> {
    udp: &'a UdpSocket,
    ground_ipv4: Ipv4Addr,
    tx_port: core::num::NonZeroU16,
    send_err_logged: bool,
}

impl DownlinkSink for RealtimeDownlinkSink<'_> {
    fn unit(&mut self, unit: &[u8]) {
        match self
            .udp
            .send_to_ipv4(unit, self.ground_ipv4, self.tx_port.get())
        {
            Ok(_) => self.send_err_logged = false,
            Err(status) => {
                if !self.send_err_logged {
                    self.send_err_logged = true;
                    let _ = evs::send_event_with_status(
                        35,
                        EventType::Error,
                        "LINK_LAB: UDP send_to_ipv4 failed for TM frame",
                        status,
                    );
                }
            }
        }
    }
}

/// Polls up to 64 pending UDP TC datagrams and feeds valid frames to the link engine.
fn drain_udp_uplink(
    udp: &UdpSocket,
    rx_buf: &mut [u8; RX_BUF_LEN],
    link: &mut Link,
    now: Instant,
    up_sink: &mut RealtimeUplinkSink,
    recv_err_logged: &mut bool,
) {
    for _ in 0..64 {
        match udp.recv_from(rx_buf, osal::Timeout::Poll) {
            Ok(Some(n)) if n > TC_FRAME_AREA_LEN => {
                *recv_err_logged = false;
                let _ = evs::send_event(
                    31,
                    EventType::Error,
                    "LINK_LAB: Rejected oversized UDP TC datagram",
                );
            }
            Ok(Some(n)) => {
                *recv_err_logged = false;
                if let Some(frame) = rx_buf.get(..n) {
                    link.accept_unit(now, frame, up_sink);
                }
            }
            Ok(None) => {
                *recv_err_logged = false;
                break;
            }
            Err(status) => {
                if !*recv_err_logged {
                    *recv_err_logged = true;
                    let _ = evs::send_event_with_status(
                        36,
                        EventType::Error,
                        "LINK_LAB: UDP recv_from failed",
                        status,
                    );
                }
                break;
            }
        }
    }
}

/// Reads monotonic OSAL local time, preserving `last_now` across errors or backward jumps.
fn sample_monotonic_now(last_now: &mut Instant, time_err_logged: &mut bool) -> Instant {
    match osal::local_time_us() {
        Ok(us) => {
            *time_err_logged = false;
            let sample = Instant::from_micros(us);
            if sample >= *last_now {
                *last_now = sample;
            }
        }
        Err(status) => {
            if !*time_err_logged {
                *time_err_logged = true;
                let _ = evs::send_event_with_status(
                    37,
                    EventType::Error,
                    "LINK_LAB: Failed to read OSAL local time",
                    status,
                );
            }
        }
    }
    *last_now
}

/// Runs the native real-time UDP TM/TC dispatch loop.
#[inline(never)]
fn run_native_realtime(pipe: &mut Pipe, config: &LinkConfig, rt: RealtimeConfig) -> ExitStatus {
    let Ok(udp) = UdpSocket::bind("0.0.0.0", rt.rx_port.get()) else {
        let _ = evs::send_event(
            30,
            EventType::Critical,
            "LINK_LAB: Failed to bind UDP rx_port",
        );
        return ExitStatus::AppError;
    };

    let Ok(mut link) = Link::new(Level::Frames, config) else {
        let _ = evs::send_event(
            20,
            EventType::Critical,
            "LINK_LAB: Failed to initialize Link",
        );
        return ExitStatus::AppError;
    };

    let mut down_sink = RealtimeDownlinkSink {
        udp: &udp,
        ground_ipv4: rt.ground_ipv4,
        tx_port: rt.tx_port,
        send_err_logged: false,
    };
    let mut up_sink = RealtimeUplinkSink;
    let mut rx_buf = [0u8; RX_BUF_LEN];
    let mut time_err_logged = false;
    let mut recv_err_logged = false;
    let mut last_now = Instant::from_micros(0);

    let mut run_status = RunStatus::AppRun;
    let mut pipe_guard = PipeRecvGuard::new();
    while es::run_loop(&mut run_status) {
        let buf = match pipe.receive_buffer(PipeRecvGuard::RECV_TIMEOUT) {
            Ok(buf) => {
                pipe_guard.on_ok();
                buf
            }
            Err(status) => {
                if pipe_guard.on_err(status, "LINK_LAB: Pipe receive failed") == RecvAction::Abort {
                    run_status = RunStatus::AppError;
                    break;
                }
                continue;
            }
        };
        let Ok(mid) = buf.msg_id() else {
            continue;
        };
        let now = sample_monotonic_now(&mut last_now, &mut time_err_logged);

        match mid.as_u32() {
            cfe_sys::NODE_WAKE_TX_MID => {
                drain_udp_uplink(
                    &udp,
                    &mut rx_buf,
                    &mut link,
                    now,
                    &mut up_sink,
                    &mut recv_err_logged,
                );
                link.slot(now, true, &mut down_sink);
            }
            cfe_sys::CFE_EVS_LONG_EVENT_MSG_MID
            | cfe_sys::CFE_EVS_SHORT_EVENT_MSG_MID
            | cfe_sys::CFE_ES_HK_TLM_MID
            | cfe_sys::CFE_EVS_HK_TLM_MID
            | cfe_sys::CFE_SB_HK_TLM_MID
            | cfe_sys::CFE_TIME_HK_TLM_MID
            | cfe_sys::CFE_TBL_HK_TLM_MID => {
                link.accept_packet(now, buf.as_bytes(), &mut down_sink);
            }
            _ => {}
        }
    }
    ExitStatus::from_run_status(run_status)
}

/// Runs the `link_lab` initialization, Software Bus dispatch loop, and teardown.
fn app_main() {
    let Ok((mut pipe, mut table, mut config)) = init_app() else {
        es::exit_app(ExitStatus::AppError);
        return;
    };

    let exit_status = match config.mode {
        LinkMode::Lockstep => run_lockstep(&mut pipe, &mut table, &mut config),
        LinkMode::NativeRealtime(rt) => run_native_realtime(&mut pipe, &config, rt),
    };

    let _ = table.unregister();
    let _ = pipe.delete();
    es::exit_app(exit_status);
}

/// Application entry point exported to cFE.
#[expect(unsafe_code, reason = "cFS entry point must be extern \"C\"")]
#[unsafe(no_mangle)]
pub extern "C" fn LINK_LAB_AppMain() {
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
