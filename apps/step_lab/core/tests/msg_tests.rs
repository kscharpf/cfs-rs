//! Tests for internal Software Bus messages in `step_lab_core::msg`.

#![expect(
    unsafe_code,
    clippy::unwrap_used,
    clippy::panic,
    reason = "integration tests use standard test ergonomics and raw byte inspection"
)]

use step_lab_core::msg::{
    CmdPacketDoneMsg, CmdPacketMsg, DoneMsg, LinkLevel, MAX_EVENT_ARGS, MAX_UNIT_LEN, RecordKind,
    RecordMsg, ScnStatus, ScnStatusMsg, StartMsg, TlmPacketMsg, UnitMsg, WakeCmdMsg, WakeHkMsg,
    WakePlMsg, WakeRxMsg, WakeTxMsg,
};
use step_lab_core::proto::{EventArgs, NodeOutput};

#[test]
fn test_start_msg_round_trip() {
    let msg = StartMsg::new(LinkLevel::Packets);
    assert_eq!(msg.link_level(), Some(LinkLevel::Packets));
    // SAFETY: Struct is POD and safe to inspect as bytes.
    let bytes = unsafe {
        core::slice::from_raw_parts(
            core::ptr::from_ref(&msg).cast::<u8>(),
            core::mem::size_of::<StartMsg>(),
        )
    };
    let decoded = StartMsg::from_bytes(bytes).unwrap();
    assert_eq!(decoded.link_level(), Some(LinkLevel::Packets));

    let msg_frames = StartMsg::new(LinkLevel::Frames);
    assert_eq!(msg_frames.link_level(), Some(LinkLevel::Frames));

    let msg_cop1 = StartMsg::new(LinkLevel::Cop1);
    assert_eq!(msg_cop1.link_level(), Some(LinkLevel::Cop1));

    assert_eq!(
        LinkLevel::from_case_id("a0-empty"),
        Some(LinkLevel::Packets)
    );
    assert_eq!(
        LinkLevel::from_case_id("a1-nominal"),
        Some(LinkLevel::Packets)
    );
    assert_eq!(
        LinkLevel::from_case_id("a2-nominal"),
        Some(LinkLevel::Frames)
    );
    assert_eq!(LinkLevel::from_case_id("a3-nominal"), Some(LinkLevel::Cop1));
    assert_eq!(LinkLevel::from_case_id(""), None);
    assert_eq!(LinkLevel::from_case_id("a"), None);
    assert_eq!(LinkLevel::from_case_id("a4-nominal"), None);
    assert_eq!(LinkLevel::from_case_id("a10-extra"), None);
    assert_eq!(LinkLevel::from_case_id("b1-nominal"), None);
    assert_eq!(LinkLevel::from_case_id("unknown"), None);
}

#[test]
fn test_wake_rx_msg_round_trip() {
    let unit = [1u8, 2, 3, 4, 5];
    let msg = WakeRxMsg::new(123_456, &unit).unwrap();
    assert_eq!(msg.now, 123_456);
    assert_eq!(msg.unit_bytes(), &unit);

    // SAFETY: Struct is POD and safe to inspect as bytes.
    let bytes = unsafe {
        core::slice::from_raw_parts(
            core::ptr::from_ref(&msg).cast::<u8>(),
            core::mem::size_of::<WakeRxMsg>(),
        )
    };
    let decoded = WakeRxMsg::from_bytes(bytes).unwrap();
    assert_eq!(decoded.now, 123_456);
    assert_eq!(decoded.unit_bytes(), &unit);

    let too_large = [0u8; MAX_UNIT_LEN + 1];
    assert!(WakeRxMsg::new(0, &too_large).is_none());
}

#[test]
fn test_wake_tx_msg_round_trip() {
    let msg = WakeTxMsg::new(555_000, true);
    assert_eq!(msg.now, 555_000);
    assert!(msg.is_tx_slot());

    // SAFETY: Struct is POD and safe to inspect as bytes.
    let bytes = unsafe {
        core::slice::from_raw_parts(
            core::ptr::from_ref(&msg).cast::<u8>(),
            core::mem::size_of::<WakeTxMsg>(),
        )
    };
    let decoded = WakeTxMsg::from_bytes(bytes).unwrap();
    assert_eq!(decoded.now, 555_000);
    assert!(decoded.is_tx_slot());
}

#[test]
fn test_done_msg_round_trip() {
    let msg = DoneMsg::new(Some(999_000));
    assert_eq!(msg.deadline(), Some(999_000));

    // SAFETY: Struct is POD and safe to inspect as bytes.
    let bytes = unsafe {
        core::slice::from_raw_parts(
            core::ptr::from_ref(&msg).cast::<u8>(),
            core::mem::size_of::<DoneMsg>(),
        )
    };
    let decoded = DoneMsg::from_bytes(bytes).unwrap();
    assert_eq!(decoded.deadline(), Some(999_000));

    let msg_none = DoneMsg::new(None);
    assert_eq!(msg_none.deadline(), None);
    // SAFETY: Struct is POD and safe to inspect as bytes.
    let bytes_none = unsafe {
        core::slice::from_raw_parts(
            core::ptr::from_ref(&msg_none).cast::<u8>(),
            core::mem::size_of::<DoneMsg>(),
        )
    };
    let decoded_none = DoneMsg::from_bytes(bytes_none).unwrap();
    assert_eq!(decoded_none.deadline(), None);
}

#[test]
fn test_wake_cmd_hk_pl_msg_round_trip() {
    for (msg, expected_now) in [
        (WakeCmdMsg::new(42_000), 42_000),
        (WakeHkMsg::new(100_000), 100_000),
        (WakePlMsg::new(500_000), 500_000),
    ] {
        assert_eq!(msg.now, expected_now);

        // SAFETY: Struct is POD and safe to inspect as bytes.
        let bytes = unsafe {
            core::slice::from_raw_parts(
                core::ptr::from_ref(&msg).cast::<u8>(),
                core::mem::size_of::<WakeCmdMsg>(),
            )
        };
        let decoded = WakeCmdMsg::from_bytes(bytes).unwrap();
        assert_eq!(decoded.now, expected_now);
    }

    assert!(WakeCmdMsg::from_bytes(&[0u8; 8]).is_none());
}

#[test]
fn test_scn_status_msg_round_trip() {
    let status = ScnStatus {
        commands_executed: 12,
        commands_rejected: 3,
        queue_occupancy: 4,
        table_id: 2,
        table_crc: 0x0C4F,
        dumps_completed: 1,
        payload_mode_on: false,
    };
    let msg = ScnStatusMsg::new(status);
    assert_eq!(msg.status(), status);

    // SAFETY: Struct is POD and safe to inspect as bytes.
    let bytes = unsafe {
        core::slice::from_raw_parts(
            core::ptr::from_ref(&msg).cast::<u8>(),
            core::mem::size_of::<ScnStatusMsg>(),
        )
    };
    let decoded = ScnStatusMsg::from_bytes(bytes).unwrap();
    assert_eq!(decoded.status(), status);

    let default_status = ScnStatus::default();
    assert!(default_status.payload_mode_on);
    let default_msg = ScnStatusMsg::new(default_status);
    assert_eq!(default_msg.status(), default_status);

    // Invalid payload_mode byte is rejected.
    let mut bad_bytes = bytes.to_vec();
    if let Some(byte) = bad_bytes.get_mut(26) {
        *byte = 2;
    }
    assert!(ScnStatusMsg::from_bytes(&bad_bytes).is_none());
    assert!(ScnStatusMsg::from_bytes(bytes.get(..20).unwrap()).is_none());
}

#[test]
fn test_cmd_packet_msg_round_trip() {
    let pkt = [0x10, 0x80, 0xc0, 0x00, 0x00, 0x01];
    let msg = CmdPacketMsg::new(777_000, &pkt).unwrap();
    assert_eq!(msg.now, 777_000);
    assert_eq!(msg.packet_bytes(), &pkt);

    // SAFETY: Struct is POD and safe to inspect as bytes.
    let bytes = unsafe {
        core::slice::from_raw_parts(
            core::ptr::from_ref(&msg).cast::<u8>(),
            core::mem::size_of::<CmdPacketMsg>(),
        )
    };
    let decoded = CmdPacketMsg::from_bytes(bytes).unwrap();
    assert_eq!(decoded.now, 777_000);
    assert_eq!(decoded.packet_bytes(), &pkt);

    let too_large = [0u8; MAX_UNIT_LEN + 1];
    assert!(CmdPacketMsg::new(0, &too_large).is_none());
}

#[test]
fn test_cmd_packet_done_msg_round_trip() {
    let msg = CmdPacketDoneMsg::new();
    // SAFETY: Struct is POD and safe to inspect as bytes.
    let bytes = unsafe {
        core::slice::from_raw_parts(
            core::ptr::from_ref(&msg).cast::<u8>(),
            core::mem::size_of::<CmdPacketDoneMsg>(),
        )
    };
    let _decoded = CmdPacketDoneMsg::from_bytes(bytes).unwrap();
}

#[test]
fn test_tlm_packet_msg_round_trip() {
    let pkt = [0x08, 0x10, 0xc0, 0x00, 0x00, 0x05, 0xaa, 0xbb];
    let msg = TlmPacketMsg::new(&pkt).unwrap();
    assert_eq!(msg.packet_bytes(), &pkt);

    // SAFETY: Struct is POD and safe to inspect as bytes.
    let bytes = unsafe {
        core::slice::from_raw_parts(
            core::ptr::from_ref(&msg).cast::<u8>(),
            core::mem::size_of::<TlmPacketMsg>(),
        )
    };
    let decoded = TlmPacketMsg::from_bytes(bytes).unwrap();
    assert_eq!(decoded.packet_bytes(), &pkt);

    let too_large = [0u8; MAX_UNIT_LEN + 1];
    assert!(TlmPacketMsg::new(&too_large).is_none());
}

#[test]
fn test_unit_msg_round_trip() {
    let unit = [0xde, 0xad, 0xbe, 0xef];
    let msg = UnitMsg::new(&unit).unwrap();
    assert_eq!(msg.unit_bytes(), &unit);

    // SAFETY: Struct is POD and safe to inspect as bytes.
    let bytes = unsafe {
        core::slice::from_raw_parts(
            core::ptr::from_ref(&msg).cast::<u8>(),
            core::mem::size_of::<UnitMsg>(),
        )
    };
    let decoded = UnitMsg::from_bytes(bytes).unwrap();
    assert_eq!(decoded.unit_bytes(), &unit);

    let too_large = [0u8; MAX_UNIT_LEN + 1];
    assert!(UnitMsg::new(&too_large).is_none());
}

#[test]
fn test_record_msg_event_round_trip() {
    let args = [100u64, 200, 300];
    let msg = RecordMsg::new_event(20, &args).unwrap();
    assert_eq!(msg.kind, RecordKind::Event.to_raw());
    assert_eq!(msg.code_or_apid, 20);
    assert_eq!(msg.arg_count, 3);

    // SAFETY: Struct is POD and safe to inspect as bytes.
    let bytes = unsafe {
        core::slice::from_raw_parts(
            core::ptr::from_ref(&msg).cast::<u8>(),
            core::mem::size_of::<RecordMsg>(),
        )
    };
    let decoded = RecordMsg::from_bytes(bytes).unwrap();
    assert_eq!(decoded.kind, RecordKind::Event.to_raw());
    assert_eq!(decoded.code_or_apid, 20);
    assert_eq!(decoded.arg_count, 3);

    let node_out = decoded.to_node_output().unwrap();
    if let NodeOutput::Event(code, EventArgs::Slice(a)) = node_out {
        assert_eq!(code, 20);
        assert_eq!(a, &args);
    } else {
        panic!("expected Event");
    }

    let too_many_args = [0u64; MAX_EVENT_ARGS + 1];
    assert!(RecordMsg::new_event(1, &too_many_args).is_none());
}

#[test]
fn test_record_msg_packet_out_round_trip() {
    let pkt = [0x08, 0x20, 0xc0, 0x01, 0x00, 0x04];
    let msg = RecordMsg::new_packet_out(32, &pkt).unwrap();
    assert_eq!(msg.kind, RecordKind::PacketOut.to_raw());
    assert_eq!(msg.code_or_apid, 32);

    // SAFETY: Struct is POD and safe to inspect as bytes.
    let bytes = unsafe {
        core::slice::from_raw_parts(
            core::ptr::from_ref(&msg).cast::<u8>(),
            core::mem::size_of::<RecordMsg>(),
        )
    };
    let decoded = RecordMsg::from_bytes(bytes).unwrap();
    assert_eq!(decoded.kind, RecordKind::PacketOut.to_raw());
    assert_eq!(decoded.code_or_apid, 32);

    let node_out = decoded.to_node_output().unwrap();
    if let NodeOutput::PacketOut(apid, data) = node_out {
        assert_eq!(apid, 32);
        assert_eq!(data, &pkt);
    } else {
        panic!("expected PacketOut");
    }
}

#[test]
fn test_record_msg_packet_in_round_trip() {
    let pkt = [0x10, 0x80, 0xc0, 0x01, 0x00, 0x02];
    let msg = RecordMsg::new_packet_in(128, &pkt).unwrap();
    assert_eq!(msg.kind, RecordKind::PacketIn.to_raw());
    assert_eq!(msg.code_or_apid, 128);

    // SAFETY: Struct is POD and safe to inspect as bytes.
    let bytes = unsafe {
        core::slice::from_raw_parts(
            core::ptr::from_ref(&msg).cast::<u8>(),
            core::mem::size_of::<RecordMsg>(),
        )
    };
    let decoded = RecordMsg::from_bytes(bytes).unwrap();
    assert_eq!(decoded.kind, RecordKind::PacketIn.to_raw());
    assert_eq!(decoded.code_or_apid, 128);

    let node_out = decoded.to_node_output().unwrap();
    if let NodeOutput::PacketIn(apid, data) = node_out {
        assert_eq!(apid, 128);
        assert_eq!(data, &pkt);
    } else {
        panic!("expected PacketIn");
    }
}
