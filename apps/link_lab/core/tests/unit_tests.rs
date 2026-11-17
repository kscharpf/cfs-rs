//! Unit tests for `link-lab-core`.
//!
//! Tests include:
//! - Bound check on `Link` struct size (<= 8192 bytes)
//! - Hand-built packets at `Level::Packets`
//! - Hand-built frames at `Level::Frames` (Type-BD)
//! - Hand-built frames and COP-1 sequence / lockout / unlock at `Level::Cop1` (Type-AD)
//! - Round-trip packet -> frame -> packet tests

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    reason = "integration tests use standard test ergonomics (assertions, panics, indexing, unwrap)"
)]

use aldis::core::ErrorKind;
use aldis::core::ids::FrameSeqNum;
use aldis::core::security::SecurityLayout;
use aldis::tc::{
    ControlCommand, FrameData, FrameFields, FrameType, MapConfig, PhysicalChannelConfig,
    SegmentHeader, SegmentHeaderUse, SequenceFlags, TcMapId, TcSpacecraftId, TcVcId, VcConfig,
    VcTable, encode,
};
use aldis::tm::{TmFrame, TmPhysicalChannelConfig};
use link_lab_core::{
    APP_DOWNLINK_REFUSED, APP_FARM1_STATE, APP_FARM1_UNLOCK, APP_FRAME_DISCARDED,
    APP_FRAME_REJECTED, APP_PACKET_REJECTED, APP_SEQUENCE_VIOLATION, APP_UNKNOWN_MAP, DownlinkSink,
    Level, Link, LinkConfig, TC_FRAME_AREA_LEN, TC_REASSEMBLY_AREA_LEN, TM_FRAME_LEN,
    UplinkBuffers, UplinkMode, UplinkSink, UplinkState,
};
use refapp_scenario::Instant;
use refapp_scenario::generated::{SPACECRAFT_ID, apid, tc, tm};

#[derive(Default)]
struct TestUplinkSink {
    packets: Vec<Vec<u8>>,
    events: Vec<(u16, Vec<u64>)>,
}

impl UplinkSink for TestUplinkSink {
    fn command_packet(&mut self, packet: &[u8]) {
        self.packets.push(packet.to_vec());
    }

    fn event(&mut self, code: u16, args: &[u64]) {
        self.events.push((code, args.to_vec()));
    }
}

#[derive(Default)]
struct TestDownlinkSink {
    units: Vec<Vec<u8>>,
    events: Vec<(u16, Vec<u64>)>,
}

impl DownlinkSink for TestDownlinkSink {
    fn unit(&mut self, unit: &[u8]) {
        self.units.push(unit.to_vec());
    }

    fn event(&mut self, code: u16, args: &[u64]) {
        self.events.push((code, args.to_vec()));
    }
}

#[expect(
    clippy::as_conversions,
    reason = "test helpers use integer casting for byte packing"
)]
fn make_space_packet(apid_val: u16, is_cmd: bool, seq: u16, payload: &[u8]) -> Vec<u8> {
    let mut pkt = Vec::with_capacity(6 + payload.len());
    let type_bit = if is_cmd { 1 << 4 } else { 0 };
    let b0 = type_bit | ((apid_val >> 8) & 0x07) as u8;
    let b1 = (apid_val & 0xFF) as u8;
    pkt.push(b0);
    pkt.push(b1);
    let b2 = 0xC0 | ((seq >> 8) & 0x3F) as u8;
    let b3 = (seq & 0xFF) as u8;
    pkt.push(b2);
    pkt.push(b3);
    let data_len = payload.len() as u16;
    let len_minus_1 = data_len.saturating_sub(1);
    pkt.push((len_minus_1 >> 8) as u8);
    pkt.push((len_minus_1 & 0xFF) as u8);
    pkt.extend_from_slice(payload);
    pkt
}

fn build_tc_table() -> VcTable<1, 2> {
    let scid = TcSpacecraftId::new(SPACECRAFT_ID).expect("valid scid");
    let vc = TcVcId::new(tc::VCID).expect("valid vc");
    let frame_len = usize::from(tc::MAX_FRAME_LENGTH);
    let physical = PhysicalChannelConfig::new(frame_len, tc::FECF).expect("valid physical");
    let map = |id: u8| {
        let map_id = TcMapId::new(id).expect("valid map");
        MapConfig::new(scid, vc, map_id, SecurityLayout::NONE)
    };
    let header = if tc::SEGMENT_HEADER {
        SegmentHeaderUse::Present
    } else {
        SegmentHeaderUse::Absent(SecurityLayout::NONE)
    };
    VcTable::new(
        physical,
        &[VcConfig::new(scid, vc, frame_len, header).expect("valid vc config")],
        &[map(tc::MAP_COMMANDS), map(tc::MAP_TABLE_UPLOAD)],
    )
    .expect("valid table")
}

fn build_tc_frame(
    table: &VcTable<1, 2>,
    frame_type: FrameType,
    seq: u8,
    map_id: u8,
    packet: &[u8],
) -> Vec<u8> {
    let scid = TcSpacecraftId::new(SPACECRAFT_ID).expect("valid scid");
    let vc = TcVcId::new(tc::VCID).expect("valid vc");
    let fields = FrameFields {
        frame_type,
        spacecraft_id: scid,
        vc_id: vc,
        sequence_number: FrameSeqNum::new(seq),
    };
    let segment_header = Some(SegmentHeader {
        sequence_flags: SequenceFlags::Unsegmented,
        map_id: TcMapId::new(map_id).expect("valid map"),
    });
    let data = FrameData::Fdu {
        segment_header,
        user_data: packet,
    };
    let mut out = vec![0_u8; usize::from(tc::MAX_FRAME_LENGTH)];
    let len = encode(table, fields, data, &mut out).expect("encode tc frame");
    out.truncate(len);
    out
}

fn build_tc_unlock(table: &VcTable<1, 2>) -> Vec<u8> {
    let scid = TcSpacecraftId::new(SPACECRAFT_ID).expect("valid scid");
    let vc = TcVcId::new(tc::VCID).expect("valid vc");
    let fields = FrameFields {
        frame_type: FrameType::Bc,
        spacecraft_id: scid,
        vc_id: vc,
        sequence_number: FrameSeqNum::new(0),
    };
    let data = FrameData::Control(ControlCommand::Unlock);
    let mut out = vec![0_u8; usize::from(tc::MAX_FRAME_LENGTH)];
    let len = encode(table, fields, data, &mut out).expect("encode tc unlock");
    out.truncate(len);
    out
}

#[test]
fn test_link_size_bounded() {
    let size = core::mem::size_of::<Link>();
    assert!(size <= 8192, "Link size {size} exceeds 8192 bytes");
}

#[test]
fn test_packets_level_nominal_and_rejects() {
    let config = LinkConfig::default();
    let mut link = Link::new(Level::Packets, &config).expect("create link");
    assert_eq!(link.level(), Level::Packets);
    assert_eq!(link.config(), &config);

    let t0 = Instant::from_micros(0);
    let mut up_sink = TestUplinkSink::default();
    let mut down_sink = TestDownlinkSink::default();

    // Nominal uplink command packet
    let cmd_pkt = make_space_packet(apid::COMMANDS, true, 1, &[0xAA, 0xBB, 0xCC]);
    link.accept_unit(t0, &cmd_pkt, &mut up_sink);
    assert_eq!(up_sink.packets.len(), 1);
    assert_eq!(up_sink.packets[0], cmd_pkt);
    assert_eq!(up_sink.events.len(), 0);

    // Truncated uplink packet rejected
    let bad_pkt = [0x18, 0x01]; // only 2 bytes
    link.accept_unit(t0, &bad_pkt, &mut up_sink);
    assert_eq!(up_sink.packets.len(), 1);
    assert_eq!(up_sink.events.len(), 1);
    assert_eq!(up_sink.events[0].0, APP_PACKET_REJECTED);

    // Nominal downlink telemetry packet
    let tm_pkt = make_space_packet(apid::HOUSEKEEPING, false, 1, &[1, 2, 3, 4, 5]);
    link.accept_packet(t0, &tm_pkt, &mut down_sink);
    assert_eq!(down_sink.units.len(), 1);
    assert_eq!(down_sink.units[0], tm_pkt);
    assert_eq!(down_sink.events.len(), 0);

    // Malformed downlink packet refused
    link.accept_packet(t0, &bad_pkt, &mut down_sink);
    assert_eq!(down_sink.units.len(), 1);
    assert_eq!(down_sink.events.len(), 1);
    assert_eq!(down_sink.events[0].0, APP_DOWNLINK_REFUSED);

    // Slot is no-op at Packets level
    link.slot(t0, true, &mut down_sink);
    assert_eq!(down_sink.units.len(), 1);

    // Deadline is None at Packets level
    assert_eq!(link.next_deadline(t0), None);
}

#[test]
fn test_frames_level_downlink_nominal_and_idle() {
    let config = LinkConfig::default();
    let mut link = Link::new(Level::Frames, &config).expect("create link");
    assert_eq!(link.level(), Level::Frames);

    let t0 = Instant::from_micros(1_000_000);
    let mut down_sink = TestDownlinkSink::default();

    // Submit telemetry packet (Housekeeping -> VC 0)
    let tm_pkt = make_space_packet(apid::HOUSEKEEPING, false, 1, &[0xDE, 0xAD, 0xBE, 0xEF]);
    link.accept_packet(t0, &tm_pkt, &mut down_sink);
    assert_eq!(down_sink.units.len(), 0); // queued, not emitted until slot
    assert_eq!(down_sink.events.len(), 0);

    // Release deadline should now be set
    let deadline = link.next_deadline(t0);
    assert!(deadline.is_some());
    let deadline = deadline.expect("deadline");
    assert!(deadline > t0);

    // Transmit slot at deadline
    link.slot(deadline, true, &mut down_sink);
    assert_eq!(down_sink.units.len(), 1);
    assert_eq!(down_sink.units[0].len(), TM_FRAME_LEN);

    // Verify TM frame decode
    let pc = TmPhysicalChannelConfig::new(TM_FRAME_LEN, true).expect("valid pc");
    let decoded = TmFrame::decode(&down_sink.units[0], &pc).expect("decode tm frame");
    assert_eq!(decoded.header().scid.get(), SPACECRAFT_ID);
    assert_eq!(decoded.header().vcid.get(), tm::VCID_REALTIME);
    assert_eq!(decoded.header().mc_frame_count, 0);
    assert_eq!(decoded.header().vc_frame_count, 0);
    assert!(decoded.ocf().is_some());

    // Next slot without pending packets produces an idle frame (VC 7, OID)
    let t2 = Instant::from_micros(deadline.as_micros().saturating_add(config.frame_slot_us));
    link.slot(t2, true, &mut down_sink);
    assert_eq!(down_sink.units.len(), 2);
    let idle_decoded = TmFrame::decode(&down_sink.units[1], &pc).expect("decode idle frame");
    assert_eq!(idle_decoded.header().vcid.get(), tm::VCID_IDLE);
}

#[test]
fn test_frames_level_downlink_unmapped_apid() {
    let config = LinkConfig::default();
    let mut link = Link::new(Level::Frames, &config).expect("create link");

    let t0 = Instant::from_micros(0);
    let mut down_sink = TestDownlinkSink::default();

    // APID 999 is not mapped to any VC
    let pkt = make_space_packet(999, false, 1, &[1, 2, 3]);
    link.accept_packet(t0, &pkt, &mut down_sink);
    assert_eq!(down_sink.events.len(), 1);
    assert_eq!(down_sink.events[0].0, APP_DOWNLINK_REFUSED);
    assert_eq!(down_sink.units.len(), 0);
}

#[test]
fn test_frames_level_uplink_type_bd_and_faults() {
    let config = LinkConfig::default();
    let mut link = Link::new(Level::Frames, &config).expect("create link");
    let table = build_tc_table();

    let t0 = Instant::from_micros(0);
    let mut up_sink = TestUplinkSink::default();

    // Nominal Type-BD frame
    let cmd_pkt = make_space_packet(apid::COMMANDS, true, 10, &[0x11, 0x22, 0x33, 0x44]);
    let frame = build_tc_frame(&table, FrameType::Bd, 0, tc::MAP_COMMANDS, &cmd_pkt);
    link.accept_unit(t0, &frame, &mut up_sink);
    assert_eq!(up_sink.packets.len(), 1);
    assert_eq!(up_sink.packets[0], cmd_pkt);
    assert_eq!(up_sink.events.len(), 0);

    // Corrupted CRC rejects frame
    let mut corrupt_frame = frame.clone();
    let len = corrupt_frame.len();
    corrupt_frame[len - 1] ^= 0xFF;
    link.accept_unit(t0, &corrupt_frame, &mut up_sink);
    assert_eq!(up_sink.packets.len(), 1); // no new packet
    assert_eq!(up_sink.events.len(), 1);
    assert_eq!(up_sink.events[0].0, APP_FRAME_REJECTED);

    // Frame with unknown MAP: flip MAP bits in segment header (byte 5) to 15 (unmapped)
    let mut bad_map_frame = frame;
    bad_map_frame[5] = (bad_map_frame[5] & 0xC0) | 15;
    aldis::core::crc::write_fecf(&mut bad_map_frame).expect("write fecf");
    link.accept_unit(t0, &bad_map_frame, &mut up_sink);
    assert_eq!(up_sink.packets.len(), 1);
    assert_eq!(up_sink.events.len(), 2);
    assert_eq!(up_sink.events[1].0, APP_UNKNOWN_MAP);
}

#[test]
fn test_frames_mode_rejects_type_ad_and_bc_without_stepping_farm() {
    let config = LinkConfig::default();
    let mut uplink = UplinkState::new(&config, UplinkMode::Frames).expect("create uplink");
    assert_eq!(uplink.mode(), UplinkMode::Frames);

    let table = build_tc_table();
    let t0 = Instant::from_micros(0);
    let mut up_sink = TestUplinkSink::default();
    let mut frame_area = [0_u8; TC_FRAME_AREA_LEN];
    let mut fdu_buf = [0_u8; TC_FRAME_AREA_LEN];
    let mut reassembly_area = [0_u8; TC_REASSEMBLY_AREA_LEN];

    let initial_clcw = uplink.clcw();

    // Type-AD frame must be rejected cleanly without stepping FARM-1.
    let cmd_pkt = make_space_packet(apid::COMMANDS, true, 0, &[0xAA, 0xBB]);
    let sequence_frame = build_tc_frame(&table, FrameType::Ad, 0, tc::MAP_COMMANDS, &cmd_pkt);
    {
        let mut buffers = UplinkBuffers {
            frame_area: &mut frame_area,
            fdu_buf: &mut fdu_buf,
            reassembly_area: &mut reassembly_area,
        };
        uplink.accept(t0, &sequence_frame, &mut buffers, &mut up_sink);
    }
    assert_eq!(up_sink.packets.len(), 0);
    assert_eq!(
        up_sink.events,
        vec![(
            APP_FRAME_REJECTED,
            vec![u64::from(ErrorKind::InvalidField.code())],
        )]
    );
    assert_eq!(uplink.clcw(), initial_clcw);

    // Type-BC Unlock frame must also be rejected cleanly without stepping FARM-1.
    let unlock_ctrl = build_tc_unlock(&table);
    {
        let mut buffers = UplinkBuffers {
            frame_area: &mut frame_area,
            fdu_buf: &mut fdu_buf,
            reassembly_area: &mut reassembly_area,
        };
        uplink.accept(t0, &unlock_ctrl, &mut buffers, &mut up_sink);
    }
    assert_eq!(up_sink.packets.len(), 0);
    assert_eq!(
        up_sink.events,
        vec![
            (
                APP_FRAME_REJECTED,
                vec![u64::from(ErrorKind::InvalidField.code())],
            ),
            (
                APP_FRAME_REJECTED,
                vec![u64::from(ErrorKind::InvalidField.code())],
            ),
        ]
    );
    assert_eq!(uplink.clcw(), initial_clcw);

    // Type-BD frame still succeeds in Frames mode.
    let bypass_frame = build_tc_frame(&table, FrameType::Bd, 0, tc::MAP_COMMANDS, &cmd_pkt);
    {
        let mut buffers = UplinkBuffers {
            frame_area: &mut frame_area,
            fdu_buf: &mut fdu_buf,
            reassembly_area: &mut reassembly_area,
        };
        uplink.accept(t0, &bypass_frame, &mut buffers, &mut up_sink);
    }
    assert_eq!(up_sink.packets, vec![cmd_pkt]);
    assert_eq!(up_sink.events.len(), 2);
}

#[test]
fn test_cop1_uplink_sequence_lockout_and_unlock() {
    let config = LinkConfig::default();
    let mut link = Link::new(Level::Cop1, &config).expect("create link");
    let table = build_tc_table();

    let t0 = Instant::from_micros(0);
    let mut up_sink = TestUplinkSink::default();

    // Seq 0: accepted, V(R) -> 1
    let pkt0 = make_space_packet(apid::COMMANDS, true, 0, &[1, 2, 3]);
    let frame0 = build_tc_frame(&table, FrameType::Ad, 0, tc::MAP_COMMANDS, &pkt0);
    link.accept_unit(t0, &frame0, &mut up_sink);
    assert_eq!(up_sink.packets.len(), 1);
    assert_eq!(up_sink.packets[0], pkt0);

    // Check CLCW in downlink reflects V(R) = 1
    let mut down_sink = TestDownlinkSink::default();
    link.slot(t0, true, &mut down_sink);
    assert_eq!(down_sink.units.len(), 1);
    let pc = TmPhysicalChannelConfig::new(TM_FRAME_LEN, true).expect("valid pc");
    let tm_frame = TmFrame::decode(&down_sink.units[0], &pc).expect("decode tm");
    let ocf = tm_frame.ocf().expect("ocf present");
    // CLCW report value V(R) is in octet 3 of CLCW
    assert_eq!(ocf.to_octets()[3], 1);

    // Seq 1: accepted, V(R) -> 2
    let pkt1 = make_space_packet(apid::COMMANDS, true, 1, &[4, 5, 6]);
    let frame1 = build_tc_frame(&table, FrameType::Ad, 1, tc::MAP_COMMANDS, &pkt1);
    link.accept_unit(t0, &frame1, &mut up_sink);
    assert_eq!(up_sink.packets.len(), 2);
    assert_eq!(up_sink.packets[1], pkt1);

    // Retransmit Seq 0: discarded (sequence violation, V(R)=2, seq=0 is behind V(R))
    link.accept_unit(t0, &frame0, &mut up_sink);
    assert_eq!(up_sink.packets.len(), 2); // packet not delivered
    assert!(
        up_sink
            .events
            .iter()
            .any(|(code, _)| *code == APP_FRAME_DISCARDED || *code == APP_SEQUENCE_VIOLATION)
    );

    // Send Seq far ahead outside window (W=10, V(R)=2, so seq=50 is outside window) -> Lockout
    let pkt_far = make_space_packet(apid::COMMANDS, true, 50, &[7, 8, 9]);
    let frame_far = build_tc_frame(&table, FrameType::Ad, 50, tc::MAP_COMMANDS, &pkt_far);
    link.accept_unit(t0, &frame_far, &mut up_sink);
    assert_eq!(up_sink.packets.len(), 2);
    assert!(
        up_sink
            .events
            .iter()
            .any(|(code, _)| *code == APP_FARM1_STATE)
    );

    // Send Unlock command (Type-BC)
    let unlock_frame = build_tc_unlock(&table);
    link.accept_unit(t0, &unlock_frame, &mut up_sink);
    assert!(
        up_sink
            .events
            .iter()
            .any(|(code, _)| *code == APP_FARM1_UNLOCK)
    );
}

#[test]
fn test_round_trip_packet_to_frame_to_packet() {
    let config = LinkConfig::default();
    let mut link = Link::new(Level::Frames, &config).expect("create link");
    let table = build_tc_table();

    // 1. Uplink round-trip: Space Packet -> TC Frame -> accept_unit -> command_packet
    let orig_cmd = make_space_packet(apid::COMMANDS, true, 42, &[0xAA, 0x55, 0x12, 0x34, 0x56]);
    let tc_frame = build_tc_frame(&table, FrameType::Bd, 0, tc::MAP_COMMANDS, &orig_cmd);
    let mut up_sink = TestUplinkSink::default();
    link.accept_unit(Instant::from_micros(0), &tc_frame, &mut up_sink);
    assert_eq!(up_sink.packets.len(), 1);
    assert_eq!(up_sink.packets[0], orig_cmd);

    // 2. Downlink round-trip: Space Packet -> accept_packet -> slot -> TM Frame
    let orig_tm = make_space_packet(
        apid::HOUSEKEEPING,
        false,
        99,
        &[0xCA, 0xFE, 0xBA, 0xBE, 0x01, 0x02],
    );
    let mut down_sink = TestDownlinkSink::default();
    let t0 = Instant::from_micros(10_000);
    link.accept_packet(t0, &orig_tm, &mut down_sink);

    let deadline = link.next_deadline(t0).expect("deadline");
    link.slot(deadline, true, &mut down_sink);
    assert_eq!(down_sink.units.len(), 1);
    let tm_unit = &down_sink.units[0];

    let pc = TmPhysicalChannelConfig::new(TM_FRAME_LEN, true).expect("valid pc");
    let frame = TmFrame::decode(tm_unit, &pc).expect("decode tm");
    assert_eq!(frame.header().vcid.get(), tm::VCID_REALTIME);

    // Verify the packet is in the TM frame data field at pointer offset 0
    assert_eq!(frame.header().first_header_pointer.get(), 0);
    let data_field = frame.data_field();
    let packet_in_frame = &data_field[..orig_tm.len()];
    assert_eq!(packet_in_frame, orig_tm.as_slice());
}
