//! Tests for the node protocol codec.

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "integration tests use standard test ergonomics (assertions, panics, indexing, unwrap)"
)]

use step_lab_core::proto::{
    CodecError, EventArgs, Message, NodeOutput, Outputs, PROTOCOL_VERSION, Role, decode_body,
    decode_frame, encode_body, encode_frame,
};

#[test]
fn test_hello_round_trip() {
    let msg = Message::Hello {
        version: 1,
        scenario_version: 1,
        role: Role::Flight,
        case_id: "a0-empty",
    };
    let mut buf = [0u8; 128];
    let len = encode_frame(&msg, &mut buf).unwrap();
    assert_eq!(len, 4 + 1 + 2 + 2 + 1 + 1 + 8); // 4 (len) + 1 (T) + 2 + 2 + 1 + 1 + 8 = 19

    let decoded = decode_frame(&buf[..len]).unwrap();
    match decoded {
        Message::Hello {
            version,
            scenario_version,
            role,
            case_id,
        } => {
            assert_eq!(version, 1);
            assert_eq!(scenario_version, 1);
            assert_eq!(role, Role::Flight);
            assert_eq!(case_id, "a0-empty");
        }
        _ => panic!("wrong message variant"),
    }
}

#[test]
fn test_hello_ack_round_trip() {
    let msg = Message::HelloAck {
        version: PROTOCOL_VERSION,
    };
    let mut buf = [0u8; 32];
    let len = encode_frame(&msg, &mut buf).unwrap();
    assert_eq!(len, 4 + 3);

    let decoded = decode_frame(&buf[..len]).unwrap();
    match decoded {
        Message::HelloAck { version } => assert_eq!(version, 1),
        _ => panic!("wrong message variant"),
    }
}

#[test]
fn test_end_and_end_ack_round_trip() {
    let mut buf = [0u8; 32];
    let len = encode_frame(&Message::End, &mut buf).unwrap();
    assert_eq!(len, 5); // 4 + 1
    assert_eq!(decode_frame(&buf[..len]).unwrap(), Message::End);

    let len = encode_frame(&Message::EndAck, &mut buf).unwrap();
    assert_eq!(len, 5);
    assert_eq!(decode_frame(&buf[..len]).unwrap(), Message::EndAck);
}

#[test]
fn test_step_and_step_result_round_trip() {
    let mut buf = [0u8; 512];

    // Encode a STEP message
    let step_body = [
        0x02, // T_STEP
        0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x86, 0xa0, // now = 100_000
        0x01, // tx_slot = true
        0x00, 0x02, // 2 units
        0x00, 0x03, 0xaa, 0xbb, 0xcc, // unit 1: len 3, [0xaa, 0xbb, 0xcc]
        0x00, 0x01, 0xdd, // unit 2: len 1, [0xdd]
        0x00, 0x02, // 2 ops
        0x00, 0x0a, // op 1 = 10
        0x00, 0x14, // op 2 = 20
    ];

    let decoded_step = decode_body(&step_body).unwrap();
    match decoded_step {
        Message::Step {
            now,
            tx_slot,
            units,
            ops,
        } => {
            assert_eq!(now, 100_000);
            assert!(tx_slot);
            assert_eq!(units.count(), 2);
            let unit_vec: Vec<&[u8]> = units.iter().collect();
            assert_eq!(unit_vec.len(), 2);
            assert_eq!(unit_vec[0], &[0xaa, 0xbb, 0xcc]);
            assert_eq!(unit_vec[1], &[0xdd]);

            assert_eq!(ops.count(), 2);
            let op_vec: Vec<u16> = ops.iter().collect();
            assert_eq!(op_vec, vec![10, 20]);
        }
        _ => panic!("wrong message variant"),
    }

    // STEP_RESULT with outputs
    let unit_bytes = [1, 2, 3];
    let pkt_out_bytes = [4, 5, 6, 7];
    let pkt_in_bytes = [8, 9];
    let event_args = [100u64, 200u64];
    let outputs = [
        NodeOutput::Unit(&unit_bytes),
        NodeOutput::PacketOut(0x010, &pkt_out_bytes),
        NodeOutput::PacketIn(0x020, &pkt_in_bytes),
        NodeOutput::Event(42, EventArgs::Slice(&event_args)),
    ];
    let step_res = Message::StepResult {
        outputs: Outputs::Slice(&outputs),
        next_deadline: Some(150_000),
    };
    let res_len = encode_frame(&step_res, &mut buf).unwrap();

    let decoded_res = decode_frame(&buf[..res_len]).unwrap();
    match decoded_res {
        Message::StepResult {
            outputs,
            next_deadline,
        } => {
            assert_eq!(next_deadline, Some(150_000));
            assert_eq!(outputs.len(), 4);
            let Outputs::Iter(iter) = outputs else {
                panic!("expected Outputs::Iter");
            };
            let decoded_outputs: Vec<NodeOutput> = iter.iter().collect();
            assert_eq!(decoded_outputs.len(), 4);
            assert_eq!(decoded_outputs[0], NodeOutput::Unit(&unit_bytes));
            assert_eq!(
                decoded_outputs[1],
                NodeOutput::PacketOut(0x010, &pkt_out_bytes)
            );
            assert_eq!(
                decoded_outputs[2],
                NodeOutput::PacketIn(0x020, &pkt_in_bytes)
            );
            match decoded_outputs[3] {
                NodeOutput::Event(code, args) => {
                    assert_eq!(code, 42);
                    assert_eq!(args.len(), 2);
                    let arg_vec: Vec<u64> = args.iter().collect();
                    assert_eq!(arg_vec, vec![100, 200]);
                }
                _ => panic!("expected Event"),
            }
        }
        _ => panic!("wrong message variant"),
    }
}

#[test]
fn test_strict_codec_errors() {
    // Zero length frame
    assert_eq!(
        decode_frame(&[0, 0, 0, 0]),
        Err(CodecError::InvalidBodyLength(0))
    );

    // Frame too short
    assert_eq!(decode_frame(&[0, 0, 0]), Err(CodecError::UnexpectedEof));

    // Unknown message type
    assert_eq!(
        decode_body(&[0x99]),
        Err(CodecError::UnknownMessageType(0x99))
    );

    // Unknown role
    let bad_role_hello = [
        0x01, 0x00, 0x01, 0x00, 0x01, 0x05, 0x02, b'a', b'1', // role = 5
    ];
    assert_eq!(
        decode_body(&bad_role_hello),
        Err(CodecError::UnknownRole(5))
    );

    // Trailing octets
    let trailing_end = [0x03, 0xff];
    assert_eq!(
        decode_body(&trailing_end),
        Err(CodecError::TrailingOctets(1))
    );

    // Invalid tx_slot
    let bad_tx_slot = [
        0x02, 0, 0, 0, 0, 0, 0, 0, 0, 2, // tx_slot = 2
        0, 0, 0, 0,
    ];
    assert_eq!(decode_body(&bad_tx_slot), Err(CodecError::InvalidTxSlot(2)));

    // Non-ASCII case_id in encode
    let mut buf = [0u8; 64];
    assert_eq!(
        encode_body(
            &Message::Hello {
                version: 1,
                scenario_version: 1,
                role: Role::Flight,
                case_id: "café",
            },
            &mut buf
        ),
        Err(CodecError::InvalidCaseId)
    );
}

#[test]
fn test_msg_struct_sizes_and_helpers() {
    use step_lab_core::msg::{DoneMsg, LinkLevel, MAX_UNIT_LEN, StartMsg, WakeRxMsg, WakeTxMsg};

    assert_eq!(LinkLevel::from_case_id("a1-nominal"), LinkLevel::Packets);
    assert_eq!(LinkLevel::from_case_id("a2-nominal"), LinkLevel::Frames);
    assert_eq!(LinkLevel::from_case_id("a3-nominal"), LinkLevel::Cop1);
    assert_eq!(LinkLevel::from_case_id("a0-empty"), LinkLevel::Packets);

    let start = StartMsg::new(LinkLevel::Packets);
    assert_eq!(start.level, 0);

    let wake_rx = WakeRxMsg::new(1000, &[1, 2, 3, 4]).unwrap();
    assert_eq!(wake_rx.now, 1000);
    assert_eq!(wake_rx.unit_len, 4);
    assert_eq!(wake_rx.unit_bytes(), &[1, 2, 3, 4]);

    assert!(WakeRxMsg::new(1000, &[0u8; MAX_UNIT_LEN + 1]).is_none());

    let wake_tx = WakeTxMsg::new(2000, true);
    assert_eq!(wake_tx.now, 2000);
    assert!(wake_tx.is_tx_slot());

    let done = DoneMsg::new(Some(3000));
    assert_eq!(done.deadline(), Some(3000));

    let done_none = DoneMsg::new(None);
    assert_eq!(done_none.deadline(), None);
}
