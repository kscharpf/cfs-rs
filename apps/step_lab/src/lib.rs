//! `step_lab`: bridge and sequencer for the harness.

#![cfg_attr(not(test), no_std)]
#![deny(unsafe_code)]

use cfe::es::{self, RunStatus};
use cfe::evs::{self, EventType};
use cfe::osal::TcpStream;
use cfe::sb::{MsgId, Pipe, Qos, Timeout, check_pipe_depth, transmit_msg};
use step_lab_core::msg::{DoneMsg, LinkLevel, StartMsg, WakeRxMsg, WakeTxMsg};
use step_lab_core::proto::{
    self, CodecError, Message, Outputs, PROTOCOL_VERSION, Role, UnitsIter, decode_body,
    encode_frame,
};

/// Event ID for `step_lab` start-up event.
pub const STEP_LAB_STARTUP_EID: u16 = 1;

/// Frame buffer capacity for socket communication.
const FRAME_BUF_LEN: usize = 4096;

/// Harness port as `u16`, derived from node headers.
const HARNESS_PORT: u16 = {
    let [b0, b1, b2, b3] = cfe_sys::NODE_HARNESS_PORT.to_le_bytes();
    assert!(b2 == 0 && b3 == 0, "NODE_HARNESS_PORT must fit in u16");
    u16::from_le_bytes([b0, b1])
};

/// Reads one framed message from the TCP stream.
fn read_msg_frame<'a>(
    stream: &mut TcpStream,
    buf: &'a mut [u8; FRAME_BUF_LEN],
) -> Result<Message<'a>, CodecError> {
    let mut len_bytes = [0u8; 4];
    stream
        .read_exact(&mut len_bytes)
        .map_err(|_| CodecError::UnexpectedEof)?;
    let body_len =
        usize::try_from(u32::from_be_bytes(len_bytes)).map_err(|_| CodecError::CountOverflow)?;
    if body_len == 0 || body_len > proto::MAX_BODY {
        return Err(CodecError::InvalidBodyLength(body_len));
    }
    let body_slice = buf.get_mut(..body_len).ok_or(CodecError::BufferTooSmall)?;
    stream
        .read_exact(body_slice)
        .map_err(|_| CodecError::UnexpectedEof)?;
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

/// Waits on the input pipe until a `DONE` message arrives, updating `earliest_deadline`.
fn wait_for_done(pipe: &mut Pipe, earliest_deadline: &mut Option<u64>) {
    while let Ok(buf) = pipe.receive_buffer(Timeout::PendForever) {
        let Ok(mid) = buf.msg_id() else {
            continue;
        };
        if mid.as_u32() == cfe_sys::NODE_DONE_MID {
            if let Some(done) = DoneMsg::from_bytes(buf.as_bytes())
                && let Some(d) = done.deadline()
            {
                *earliest_deadline = Some(earliest_deadline.map_or(d, |prev| prev.min(d)));
            }
            break;
        }
    }
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
) -> Result<LinkLevel, ()> {
    let Ok(hello_msg) = read_msg_frame(stream, rx_buf) else {
        let _ = evs::send_event(5, EventType::Critical, "STEP_LAB: Failed to read HELLO");
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

    Ok(LinkLevel::from_case_id(case_id))
}

/// Handles a single STEP from the harness.
fn handle_step(
    stream: &mut TcpStream,
    pipe: &mut Pipe,
    now: u64,
    tx_slot: bool,
    units: UnitsIter<'_>,
    tx_buf: &mut [u8; FRAME_BUF_LEN],
) -> Result<(), ()> {
    let mut earliest_deadline = None;

    // Process delivered units
    for unit in units {
        let Some(mut wake_rx) = WakeRxMsg::new(now, unit) else {
            let _ = evs::send_event(
                10,
                EventType::Critical,
                "STEP_LAB: Failed to create WakeRxMsg",
            );
            return Err(());
        };
        if cfe::msg::init(
            wake_rx.msg_mut(),
            MsgId::from_u32(cfe_sys::NODE_WAKE_RX_MID),
            core::mem::size_of::<WakeRxMsg>(),
        )
        .is_err()
        {
            let _ = evs::send_event(
                11,
                EventType::Critical,
                "STEP_LAB: Failed to init WakeRxMsg",
            );
            return Err(());
        }
        if transmit_msg(wake_rx.msg(), true).is_err() {
            let _ = evs::send_event(
                12,
                EventType::Critical,
                "STEP_LAB: Failed to transmit WakeRxMsg",
            );
            return Err(());
        }
        wait_for_done(pipe, &mut earliest_deadline);
    }

    // Process transmit slot
    let mut wake_tx = WakeTxMsg::new(now, tx_slot);
    if cfe::msg::init(
        wake_tx.msg_mut(),
        MsgId::from_u32(cfe_sys::NODE_WAKE_TX_MID),
        core::mem::size_of::<WakeTxMsg>(),
    )
    .is_err()
    {
        let _ = evs::send_event(
            13,
            EventType::Critical,
            "STEP_LAB: Failed to init WakeTxMsg",
        );
        return Err(());
    }
    if transmit_msg(wake_tx.msg(), true).is_err() {
        let _ = evs::send_event(
            14,
            EventType::Critical,
            "STEP_LAB: Failed to transmit WakeTxMsg",
        );
        return Err(());
    }
    wait_for_done(pipe, &mut earliest_deadline);

    // Reply STEP_RESULT
    let result_msg = Message::StepResult {
        outputs: Outputs::Slice(&[]),
        next_deadline: earliest_deadline,
    };
    write_msg_frame(stream, &result_msg, tx_buf).map_err(|_| ())
}

/// Runs the main sequencer loop after handshake.
fn run_sequencer(mut stream: TcpStream, mut pipe: Pipe, level: LinkLevel) {
    let mut rx_buf = [0u8; FRAME_BUF_LEN];
    let mut tx_buf = [0u8; FRAME_BUF_LEN];

    // Send START to link_lab with configured LinkLevel
    let mut start_msg = StartMsg::new(level);
    if cfe::msg::init(
        start_msg.msg_mut(),
        MsgId::from_u32(cfe_sys::NODE_START_MID),
        core::mem::size_of::<StartMsg>(),
    )
    .is_err()
    {
        let _ = evs::send_event(15, EventType::Critical, "STEP_LAB: Failed to init StartMsg");
        let _ = stream.close();
        let _ = pipe.delete();
        return;
    }
    if transmit_msg(start_msg.msg(), true).is_err() {
        let _ = evs::send_event(
            16,
            EventType::Critical,
            "STEP_LAB: Failed to transmit StartMsg",
        );
        let _ = stream.close();
        let _ = pipe.delete();
        return;
    }
    let mut unused_deadline = None;
    wait_for_done(&mut pipe, &mut unused_deadline);

    let mut run_status = RunStatus::AppRun;
    while es::run_loop(&mut run_status) {
        let Ok(msg) = read_msg_frame(&mut stream, &mut rx_buf) else {
            break;
        };

        match msg {
            Message::Step {
                now,
                tx_slot,
                units,
                ..
            } => {
                if handle_step(&mut stream, &mut pipe, now, tx_slot, units, &mut tx_buf).is_err() {
                    break;
                }
            }
            Message::End => {
                let _ = write_msg_frame(&mut stream, &Message::EndAck, &mut tx_buf);
                break;
            }
            Message::Hello { .. }
            | Message::HelloAck { .. }
            | Message::StepResult { .. }
            | Message::EndAck => break,
        }
    }

    let _ = stream.close();
    let _ = pipe.delete();
}

/// Application entry point exported to cFE.
#[expect(unsafe_code, reason = "cFS entry point must be extern \"C\"")]
#[unsafe(no_mangle)]
pub extern "C" fn STEP_LAB_AppMain() {
    // 1. Register with EVS.
    if evs::register().is_err() {
        es::exit_app(RunStatus::AppError);
        return;
    }

    // 2. Perform pipe-depth check (link-lab.md §9).
    if check_pipe_depth(16, MsgId::from_u32(cfe_sys::NODE_TEST_MID)).is_err() {
        let _ = evs::send_event(
            2,
            EventType::Critical,
            "STEP_LAB: Pipe depth check failed: OSAL queue truncation detected",
        );
        es::exit_app(RunStatus::AppError);
        return;
    }

    // 3. Create Software Bus input pipe.
    let Ok(pipe) = Pipe::create(16, "STEP_LAB_PIPE") else {
        let _ = evs::send_event(3, EventType::Critical, "STEP_LAB: Failed to create SB pipe");
        es::exit_app(RunStatus::AppError);
        return;
    };

    if pipe
        .subscribe_ex(MsgId::from_u32(cfe_sys::NODE_DONE_MID), Qos::DEFAULT, 16)
        .is_err()
    {
        let _ = evs::send_event(
            3,
            EventType::Critical,
            "STEP_LAB: Failed to subscribe to DONE",
        );
        let _ = pipe.delete();
        es::exit_app(RunStatus::AppError);
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
        es::exit_app(RunStatus::AppError);
        return;
    };

    let mut rx_buf = [0u8; FRAME_BUF_LEN];
    let mut tx_buf = [0u8; FRAME_BUF_LEN];

    // 6. HELLO / HELLO_ACK handshake.
    let Ok(level) = perform_handshake(&mut stream, &mut rx_buf, &mut tx_buf) else {
        let _ = stream.close();
        let _ = pipe.delete();
        es::exit_app(RunStatus::AppError);
        return;
    };

    // 7. Sequencing loop.
    run_sequencer(stream, pipe, level);

    es::exit_app(RunStatus::AppRun);
}

#[cfg(not(test))]
#[panic_handler]
const fn panic(_info: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}
