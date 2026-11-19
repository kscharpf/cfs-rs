//! `step_lab`: bridge and sequencer for the harness.

#![cfg_attr(not(test), no_std)]
#![deny(unsafe_code)]

use cfe::es::{self, ExitStatus, RunStatus};
use cfe::evs::{self, EventType};
use cfe::osal::{self, TcpStream};
use cfe::sb::{MsgId, Pipe, Qos, check_pipe_depth};
use node_glue::{PeerWaitGuard, RecvAction, send_sb_msg, send_wake_turn};
use step_lab_core::msg::{
    DoneMsg, LinkLevel, RecordMsg, StartMsg, UnitMsg, WakeRxMsg, WakeTurn, WakeTxMsg,
};
use step_lab_core::proto::{
    self, CodecError, EventArgs, Message, NodeOutput, Outputs, PROTOCOL_VERSION, Role, UnitsIter,
    decode_body, encode_frame,
};

/// Event ID for `step_lab` start-up event.
pub const STEP_LAB_STARTUP_EID: u16 = 1;

/// Frame buffer capacity for socket communication.
const FRAME_BUF_LEN: usize = 4096;

/// Maximum number of outputs captured per step.
const MAX_OUTPUTS: usize = 32;

/// Buffer capacity for payload data in a single step.
const MAX_PAYLOAD_BYTES: usize = 8192;

/// Maximum number of stored event arguments in a single step.
const MAX_STORED_ARGS: usize = 64;

/// Harness port as `u16`, derived from node headers.
const HARNESS_PORT: u16 = {
    let [b0, b1, b2, b3] = cfe_sys::NODE_HARNESS_PORT.to_le_bytes();
    assert!(b2 == 0 && b3 == 0, "NODE_HARNESS_PORT must fit in u16");
    u16::from_le_bytes([b0, b1])
};

#[derive(Debug, Clone, Copy)]
enum StoredOutput {
    Unit {
        offset: usize,
        len: usize,
    },
    PacketOut {
        apid: u16,
        offset: usize,
        len: usize,
    },
    PacketIn {
        apid: u16,
        offset: usize,
        len: usize,
    },
    Event {
        code: u16,
        args_offset: usize,
        args_len: usize,
    },
}

struct StepCollector {
    outputs: [Option<StoredOutput>; MAX_OUTPUTS],
    output_count: usize,
    payload_buf: [u8; MAX_PAYLOAD_BYTES],
    payload_len: usize,
    args_buf: [u64; MAX_STORED_ARGS],
    args_len: usize,
}

impl StepCollector {
    const fn new() -> Self {
        Self {
            outputs: [None; MAX_OUTPUTS],
            output_count: 0,
            payload_buf: [0; MAX_PAYLOAD_BYTES],
            payload_len: 0,
            args_buf: [0; MAX_STORED_ARGS],
            args_len: 0,
        }
    }

    fn push_unit(&mut self, unit: &[u8]) -> bool {
        if self.output_count >= MAX_OUTPUTS {
            return false;
        }
        let end = match self.payload_len.checked_add(unit.len()) {
            Some(e) if e <= MAX_PAYLOAD_BYTES => e,
            _ => return false,
        };
        let offset = self.payload_len;
        if let Some(dest) = self.payload_buf.get_mut(offset..end) {
            dest.copy_from_slice(unit);
            self.payload_len = end;
            if let Some(slot) = self.outputs.get_mut(self.output_count) {
                *slot = Some(StoredOutput::Unit {
                    offset,
                    len: unit.len(),
                });
                self.output_count = self.output_count.saturating_add(1);
                return true;
            }
        }
        false
    }

    fn push_record(&mut self, record: &RecordMsg) -> bool {
        if self.output_count >= MAX_OUTPUTS {
            return false;
        }
        match record.to_node_output() {
            Some(NodeOutput::Event(code, EventArgs::Slice(args))) => {
                let end = match self.args_len.checked_add(args.len()) {
                    Some(e) if e <= MAX_STORED_ARGS => e,
                    _ => return false,
                };
                let args_offset = self.args_len;
                if let Some(dest) = self.args_buf.get_mut(args_offset..end) {
                    dest.copy_from_slice(args);
                    self.args_len = end;
                    if let Some(slot) = self.outputs.get_mut(self.output_count) {
                        *slot = Some(StoredOutput::Event {
                            code,
                            args_offset,
                            args_len: args.len(),
                        });
                        self.output_count = self.output_count.saturating_add(1);
                        return true;
                    }
                }
            }
            Some(NodeOutput::PacketOut(apid, data)) => {
                let end = match self.payload_len.checked_add(data.len()) {
                    Some(e) if e <= MAX_PAYLOAD_BYTES => e,
                    _ => return false,
                };
                let offset = self.payload_len;
                if let Some(dest) = self.payload_buf.get_mut(offset..end) {
                    dest.copy_from_slice(data);
                    self.payload_len = end;
                    if let Some(slot) = self.outputs.get_mut(self.output_count) {
                        *slot = Some(StoredOutput::PacketOut {
                            apid,
                            offset,
                            len: data.len(),
                        });
                        self.output_count = self.output_count.saturating_add(1);
                        return true;
                    }
                }
            }
            Some(NodeOutput::PacketIn(apid, data)) => {
                let end = match self.payload_len.checked_add(data.len()) {
                    Some(e) if e <= MAX_PAYLOAD_BYTES => e,
                    _ => return false,
                };
                let offset = self.payload_len;
                if let Some(dest) = self.payload_buf.get_mut(offset..end) {
                    dest.copy_from_slice(data);
                    self.payload_len = end;
                    if let Some(slot) = self.outputs.get_mut(self.output_count) {
                        *slot = Some(StoredOutput::PacketIn {
                            apid,
                            offset,
                            len: data.len(),
                        });
                        self.output_count = self.output_count.saturating_add(1);
                        return true;
                    }
                }
            }
            _ => return false,
        }
        false
    }

    fn build_outputs<'a>(&'a self, dest: &mut [NodeOutput<'a>; MAX_OUTPUTS]) -> usize {
        let mut count = 0;
        for i in 0..self.output_count {
            let Some(Some(item)) = self.outputs.get(i) else {
                break;
            };
            let out = match *item {
                StoredOutput::Unit { offset, len } => {
                    let end = offset.saturating_add(len);
                    let bytes = self.payload_buf.get(offset..end).unwrap_or(&[]);
                    NodeOutput::Unit(bytes)
                }
                StoredOutput::PacketOut { apid, offset, len } => {
                    let end = offset.saturating_add(len);
                    let bytes = self.payload_buf.get(offset..end).unwrap_or(&[]);
                    NodeOutput::PacketOut(apid, bytes)
                }
                StoredOutput::PacketIn { apid, offset, len } => {
                    let end = offset.saturating_add(len);
                    let bytes = self.payload_buf.get(offset..end).unwrap_or(&[]);
                    NodeOutput::PacketIn(apid, bytes)
                }
                StoredOutput::Event {
                    code,
                    args_offset,
                    args_len,
                } => {
                    let end = args_offset.saturating_add(args_len);
                    let args = self.args_buf.get(args_offset..end).unwrap_or(&[]);
                    NodeOutput::Event(code, EventArgs::Slice(args))
                }
            };
            if let Some(slot) = dest.get_mut(count) {
                *slot = out;
                count = count.saturating_add(1);
            }
        }
        count
    }
}

/// Poll interval for harness TCP reads so `es::run_loop` stays responsive while idle.
const HARNESS_POLL_TIMEOUT: osal::Timeout = osal::Timeout::Millis(500);

/// Reads `buf.len()` bytes from `stream`, polling `es::run_loop` across idle socket timeouts.
fn read_exact_polling(
    stream: &mut TcpStream,
    mut buf: &mut [u8],
    run_status: &mut RunStatus,
) -> Result<(), CodecError> {
    while !buf.is_empty() {
        let n = match stream.recv(buf, HARNESS_POLL_TIMEOUT) {
            Ok(0) => return Err(CodecError::UnexpectedEof),
            Ok(n) => n,
            Err(status) if osal::is_timeout(status) => {
                if !es::run_loop(run_status) {
                    return Err(CodecError::UnexpectedEof);
                }
                continue;
            }
            Err(_) => return Err(CodecError::UnexpectedEof),
        };
        let Some(rest) = buf.get_mut(n..) else {
            return Err(CodecError::BufferTooSmall);
        };
        buf = rest;
    }
    Ok(())
}

/// Reads one framed message from the TCP stream.
fn read_msg_frame<'a>(
    stream: &mut TcpStream,
    buf: &'a mut [u8; FRAME_BUF_LEN],
    run_status: &mut RunStatus,
) -> Result<Message<'a>, CodecError> {
    let mut len_bytes = [0u8; 4];
    read_exact_polling(stream, &mut len_bytes, run_status)?;
    let body_len =
        usize::try_from(u32::from_be_bytes(len_bytes)).map_err(|_| CodecError::CountOverflow)?;
    if body_len == 0 || body_len > proto::MAX_BODY {
        return Err(CodecError::InvalidBodyLength(body_len));
    }
    let body_slice = buf.get_mut(..body_len).ok_or(CodecError::BufferTooSmall)?;
    read_exact_polling(stream, body_slice, run_status)?;
    decode_body(body_slice)
}

/// Encodes and writes one framed message to the TCP stream.
fn write_msg_frame(
    stream: &mut TcpStream,
    msg: &Message<'_>,
    buf: &mut [u8; FRAME_BUF_LEN],
) -> Result<(), CodecError> {
    let frame_len = encode_frame(msg, buf)?;
    let slice = buf.get(..frame_len).ok_or(CodecError::BufferTooSmall)?;
    stream
        .write_all(slice)
        .map_err(|_| CodecError::UnexpectedEof)
}

/// Handles a single `RecordMsg` or `UnitMsg` during turn collection.
fn handle_collected_msg(mid: u32, bytes: &[u8], collector: &mut StepCollector) -> Result<(), ()> {
    match mid {
        cfe_sys::NODE_RECORD_MID => {
            let Some(record) = RecordMsg::from_bytes(bytes) else {
                let _ = evs::send_event(
                    22,
                    EventType::Error,
                    "STEP_LAB: Malformed RecordMsg received",
                );
                return Err(());
            };
            if !collector.push_record(&record) {
                let _ = evs::send_event(
                    20,
                    EventType::Critical,
                    "STEP_LAB: Failed to push record (collector overflow)",
                );
                return Err(());
            }
        }
        cfe_sys::NODE_UNIT_MID => {
            let Some(unit_msg) = UnitMsg::from_bytes(bytes) else {
                let _ =
                    evs::send_event(23, EventType::Error, "STEP_LAB: Malformed UnitMsg received");
                return Err(());
            };
            if !collector.push_unit(unit_msg.unit_bytes()) {
                let _ = evs::send_event(
                    21,
                    EventType::Critical,
                    "STEP_LAB: Failed to push unit (collector overflow)",
                );
                return Err(());
            }
        }
        _ => {}
    }
    Ok(())
}

/// Waits on the input pipe until a `DONE` message arrives, optionally collecting turn outputs.
fn wait_turn_done(
    pipe: &mut Pipe,
    mut collector: Option<&mut StepCollector>,
    earliest_deadline: &mut Option<u64>,
    run_status: &mut RunStatus,
) -> Result<(), ()> {
    let mut wait_guard = PeerWaitGuard::new();
    while es::run_loop(run_status) {
        let buf = match pipe.receive_buffer(PeerWaitGuard::POLL_TIMEOUT) {
            Ok(buf) => {
                wait_guard.on_ok();
                buf
            }
            Err(status) => {
                if wait_guard.on_err(
                    status,
                    25,
                    "STEP_LAB: Timed out waiting for peer DONE",
                    26,
                    "STEP_LAB: Pipe error waiting for peer DONE",
                ) == RecvAction::Abort
                {
                    *run_status = RunStatus::AppError;
                    return Err(());
                }
                continue;
            }
        };
        let Ok(mid) = buf.msg_id() else {
            continue;
        };
        let raw_mid = mid.as_u32();
        if raw_mid == cfe_sys::NODE_DONE_MID {
            if let Some(done) = DoneMsg::from_bytes(buf.as_bytes())
                && let Some(d) = done.deadline()
            {
                *earliest_deadline = Some(earliest_deadline.map_or(d, |prev| prev.min(d)));
            }
            return Ok(());
        }
        if let Some(c) = collector.as_deref_mut() {
            handle_collected_msg(raw_mid, buf.as_bytes(), c)?;
        }
    }
    Err(())
}

/// Waits on the input pipe until a `DONE` message arrives, collecting outputs.
fn wait_collect_turn(
    pipe: &mut Pipe,
    collector: &mut StepCollector,
    earliest_deadline: &mut Option<u64>,
    run_status: &mut RunStatus,
) -> Result<(), ()> {
    wait_turn_done(pipe, Some(collector), earliest_deadline, run_status)
}

/// Waits on the input pipe until a `DONE` message arrives, updating `earliest_deadline`.
fn wait_for_done(
    pipe: &mut Pipe,
    earliest_deadline: &mut Option<u64>,
    run_status: &mut RunStatus,
) -> Result<(), ()> {
    wait_turn_done(pipe, None, earliest_deadline, run_status)
}

/// Connects to harness over loopback TCP with retries.
fn connect_harness() -> Result<TcpStream, ()> {
    for _ in 0..50 {
        if let Ok(s) = TcpStream::connect("127.0.0.1", HARNESS_PORT) {
            return Ok(s);
        }
        let _ = cfe::osal::task_delay(10);
    }
    Err(())
}

/// Performs the initial HELLO / `HELLO_ACK` handshake with the harness.
fn perform_handshake(
    stream: &mut TcpStream,
    rx_buf: &mut [u8; FRAME_BUF_LEN],
    tx_buf: &mut [u8; FRAME_BUF_LEN],
    run_status: &mut RunStatus,
) -> Result<LinkLevel, ()> {
    let Ok(hello_msg) = read_msg_frame(stream, rx_buf, run_status) else {
        if *run_status == RunStatus::AppRun {
            let _ = evs::send_event(5, EventType::Critical, "STEP_LAB: Failed to read HELLO");
        }
        return Err(());
    };

    let Message::Hello {
        version,
        role,
        case_id,
        ..
    } = hello_msg
    else {
        let _ = evs::send_event(
            7,
            EventType::Critical,
            "STEP_LAB: Expected HELLO as first message",
        );
        return Err(());
    };

    if version != PROTOCOL_VERSION || role != Role::Flight {
        let _ = evs::send_event(
            6,
            EventType::Critical,
            "STEP_LAB: Incompatible HELLO protocol version or role",
        );
        return Err(());
    }

    let Some(level) = LinkLevel::from_case_id(case_id) else {
        let _ = evs::send_event(
            24,
            EventType::Critical,
            "STEP_LAB: Unsupported case_id prefix in HELLO",
        );
        return Err(());
    };

    if write_msg_frame(
        stream,
        &Message::HelloAck {
            version: PROTOCOL_VERSION,
        },
        tx_buf,
    )
    .is_err()
    {
        return Err(());
    }

    Ok(level)
}

/// Sends a `WakeTimeMsg` turn (`WAKE_CMD`, `WAKE_HK`, or `WAKE_PL`) and collects outputs until `DONE`.
fn wake_time_turn(
    pipe: &mut Pipe,
    collector: &mut StepCollector,
    earliest_deadline: &mut Option<u64>,
    now: u64,
    turn: WakeTurn,
    run_status: &mut RunStatus,
) -> Result<(), ()> {
    send_wake_turn(
        turn,
        now,
        17,
        "STEP_LAB: Failed to init WakeTimeMsg",
        18,
        "STEP_LAB: Failed to transmit WakeTimeMsg",
    )
    .map_err(|_| ())?;
    wait_collect_turn(pipe, collector, earliest_deadline, run_status)
}

/// Handles a single STEP from the harness according to docs/design/link-lab.md §4.5.
fn handle_step(
    stream: &mut TcpStream,
    pipe: &mut Pipe,
    now: u64,
    tx_slot: bool,
    units: UnitsIter<'_>,
    tx_buf: &mut [u8; FRAME_BUF_LEN],
    run_status: &mut RunStatus,
) -> Result<(), ()> {
    let mut earliest_deadline = None;
    let mut collector = StepCollector::new();

    // 1. For each delivered unit: WAKE_RX to link_lab -> collect until DONE.
    for unit in units {
        let Some(mut wake_rx) = WakeRxMsg::new(now, unit) else {
            let _ = evs::send_event(
                10,
                EventType::Critical,
                "STEP_LAB: Failed to create WakeRxMsg",
            );
            return Err(());
        };
        send_sb_msg(
            &mut wake_rx,
            11,
            "STEP_LAB: Failed to init WakeRxMsg",
            12,
            "STEP_LAB: Failed to transmit WakeRxMsg",
        )
        .map_err(|_| ())?;
        wait_collect_turn(pipe, &mut collector, &mut earliest_deadline, run_status)?;
    }

    // 2–4. WAKE_CMD -> cmd_lab, WAKE_HK -> hk_lab, WAKE_PL -> pl_lab in exact §4.5 order.
    for turn in WakeTurn::ORDER {
        wake_time_turn(
            pipe,
            &mut collector,
            &mut earliest_deadline,
            now,
            turn,
            run_status,
        )?;
    }

    // 5. WAKE_TX to link_lab -> collect until DONE.
    let mut wake_tx = WakeTxMsg::new(now, tx_slot);
    send_sb_msg(
        &mut wake_tx,
        13,
        "STEP_LAB: Failed to init WakeTxMsg",
        14,
        "STEP_LAB: Failed to transmit WakeTxMsg",
    )
    .map_err(|_| ())?;
    wait_collect_turn(pipe, &mut collector, &mut earliest_deadline, run_status)?;

    // 6. Reply STEP_RESULT with all outputs in arrival order and earliest next_deadline.
    let mut output_storage = [NodeOutput::Unit(&[]); MAX_OUTPUTS];
    let count = collector.build_outputs(&mut output_storage);
    let result_msg = Message::StepResult {
        outputs: Outputs::Slice(output_storage.get(..count).unwrap_or(&[])),
        next_deadline: earliest_deadline,
    };
    write_msg_frame(stream, &result_msg, tx_buf).map_err(|_| ())
}

/// Runs the main sequencer loop after handshake and returns the final [`ExitStatus`].
fn run_sequencer(
    mut stream: TcpStream,
    mut pipe: Pipe,
    level: LinkLevel,
    mut run_status: RunStatus,
) -> ExitStatus {
    let mut rx_buf = [0u8; FRAME_BUF_LEN];
    let mut tx_buf = [0u8; FRAME_BUF_LEN];

    // Send START to all apps with configured LinkLevel
    let mut start_msg = StartMsg::new(level);
    if send_sb_msg(
        &mut start_msg,
        15,
        "STEP_LAB: Failed to init StartMsg",
        16,
        "STEP_LAB: Failed to transmit StartMsg",
    )
    .is_err()
    {
        let _ = stream.close();
        let _ = pipe.delete();
        return ExitStatus::AppError;
    }

    // Wait for DONE from link_lab, cmd_lab, hk_lab, and pl_lab
    let mut unused_deadline = None;
    for _ in 0..4 {
        if wait_for_done(&mut pipe, &mut unused_deadline, &mut run_status).is_err() {
            let _ = stream.close();
            let _ = pipe.delete();
            return ExitStatus::from_run_status(run_status);
        }
    }

    while es::run_loop(&mut run_status) {
        let Ok(msg) = read_msg_frame(&mut stream, &mut rx_buf, &mut run_status) else {
            if run_status == RunStatus::AppRun {
                let _ =
                    evs::send_event(27, EventType::Error, "STEP_LAB: Harness frame read failed");
                run_status = RunStatus::AppError;
            }
            break;
        };

        match msg {
            Message::Step {
                now,
                tx_slot,
                units,
                ..
            } => {
                if handle_step(
                    &mut stream,
                    &mut pipe,
                    now,
                    tx_slot,
                    units,
                    &mut tx_buf,
                    &mut run_status,
                )
                .is_err()
                {
                    if run_status == RunStatus::AppRun {
                        run_status = RunStatus::AppError;
                    }
                    break;
                }
            }
            Message::End => {
                if write_msg_frame(&mut stream, &Message::EndAck, &mut tx_buf).is_err() {
                    let _ = evs::send_event(
                        27,
                        EventType::Error,
                        "STEP_LAB: Failed to write EndAck frame",
                    );
                    run_status = RunStatus::AppError;
                } else {
                    run_status = RunStatus::AppExit;
                }
                break;
            }
            Message::Hello { .. }
            | Message::HelloAck { .. }
            | Message::StepResult { .. }
            | Message::EndAck => {
                let _ =
                    evs::send_event(27, EventType::Error, "STEP_LAB: Unexpected harness message");
                run_status = RunStatus::AppError;
                break;
            }
        }
    }

    let _ = stream.close();
    let _ = pipe.delete();
    ExitStatus::from_run_status(run_status)
}

/// Runs the `step_lab` initialization, harness handshake, and sequencer loop.
fn app_main() {
    // 1. Register with EVS.
    if evs::register().is_err() {
        es::exit_app(ExitStatus::AppError);
        return;
    }

    // 2. Perform pipe-depth check (link-lab.md §9).
    if check_pipe_depth(32, MsgId::from_u32(cfe_sys::NODE_TEST_MID)).is_err() {
        let _ = evs::send_event(
            2,
            EventType::Critical,
            "STEP_LAB: Pipe depth check failed: OSAL queue truncation detected",
        );
        es::exit_app(ExitStatus::AppError);
        return;
    }

    // 3. Create Software Bus input pipe.
    let Ok(pipe) = Pipe::create(32, "STEP_LAB_PIPE") else {
        let _ = evs::send_event(3, EventType::Critical, "STEP_LAB: Failed to create SB pipe");
        es::exit_app(ExitStatus::AppError);
        return;
    };

    if pipe
        .subscribe_ex(MsgId::from_u32(cfe_sys::NODE_DONE_MID), Qos::DEFAULT, 32)
        .is_err()
        || pipe
            .subscribe_ex(MsgId::from_u32(cfe_sys::NODE_RECORD_MID), Qos::DEFAULT, 32)
            .is_err()
        || pipe
            .subscribe_ex(MsgId::from_u32(cfe_sys::NODE_UNIT_MID), Qos::DEFAULT, 32)
            .is_err()
    {
        let _ = evs::send_event(
            8,
            EventType::Critical,
            "STEP_LAB: Failed to subscribe to node messages",
        );
        let _ = pipe.delete();
        es::exit_app(ExitStatus::AppError);
        return;
    }

    // 4. Send start-up event.
    let _ = evs::send_event(
        STEP_LAB_STARTUP_EID,
        EventType::Information,
        "STEP_LAB App Initialized. Version 0.1.0",
    );

    // 5. Connect to harness over loopback TCP.
    let Ok(mut stream) = connect_harness() else {
        let _ = evs::send_event(
            4,
            EventType::Critical,
            "STEP_LAB: Failed to connect to harness over TCP",
        );
        let _ = pipe.delete();
        es::exit_app(ExitStatus::AppError);
        return;
    };

    let mut rx_buf = [0u8; FRAME_BUF_LEN];
    let mut tx_buf = [0u8; FRAME_BUF_LEN];

    // 6. HELLO / HELLO_ACK handshake.
    let mut run_status = RunStatus::AppRun;
    let Ok(level) = perform_handshake(&mut stream, &mut rx_buf, &mut tx_buf, &mut run_status)
    else {
        let _ = stream.close();
        let _ = pipe.delete();
        es::exit_app(ExitStatus::from_run_status(run_status));
        return;
    };

    // 7. Sequencing loop.
    let exit_status = run_sequencer(stream, pipe, level, run_status);

    es::exit_app(exit_status);
}

/// Application entry point exported to cFE.
#[expect(unsafe_code, reason = "cFS entry point must be extern \"C\"")]
#[unsafe(no_mangle)]
pub extern "C" fn STEP_LAB_AppMain() {
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
