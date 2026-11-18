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

use core::num::NonZeroU16;

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
    APP_FRAME_REJECTED, APP_PACKET_REJECTED, APP_SEQUENCE_VIOLATION, APP_UNKNOWN_MAP, ApidRoute,
    DataVc, DownlinkSink, DownlinkState, Level, Link, LinkConfig, LinkMode, MAX_APID_ROUTES,
    RealtimeConfig, TC_FRAME_AREA_LEN, TC_REASSEMBLY_AREA_LEN, TM_FRAME_LEN, UplinkBuffers,
    UplinkMode, UplinkSink, UplinkState,
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

#[test]
fn test_link_config_from_table_bytes_round_trip_and_routing() {
    let scenario_cfg = LinkConfig::from_scenario();
    let bytes = scenario_cfg.to_table_bytes();
    assert_eq!(bytes.len(), LinkConfig::TABLE_SIZE);

    let decoded = LinkConfig::from_table_bytes(&bytes).expect("valid scenario table bytes");
    assert_eq!(decoded, scenario_cfg);
    assert_eq!(
        decoded.vc_for_apid(apid::HOUSEKEEPING),
        Some(DataVc::Realtime)
    );
    assert_eq!(decoded.vc_for_apid(apid::EVENTS), Some(DataVc::Realtime));
    assert_eq!(decoded.vc_for_apid(apid::PAYLOAD), Some(DataVc::Payload));
    assert_eq!(decoded.vc_for_apid(apid::DUMP), Some(DataVc::Payload));
    assert_eq!(decoded.vc_for_apid(999), None);

    // Custom routing overrides default APID mapping.
    let mut custom = scenario_cfg;
    custom.apid_routes = [
        Some(ApidRoute {
            apid: 0x0123,
            vc: DataVc::Payload,
        }),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    ];
    let custom_bytes = custom.to_table_bytes();
    let decoded_custom =
        LinkConfig::from_table_bytes(&custom_bytes).expect("valid custom table bytes");
    assert_eq!(decoded_custom.vc_for_apid(0x0123), Some(DataVc::Payload));
    assert_eq!(decoded_custom.vc_for_apid(apid::HOUSEKEEPING), None);

    // Lockstep mode has no default_vc and ignores unmapped APIDs (and ignores any UDP fields in table bytes).
    assert_eq!(scenario_cfg.vc_for_apid(0x0008), None);
    let mut lockstep_with_table_rt_fields = bytes;
    lockstep_with_table_rt_fields[72..74].copy_from_slice(&1234_u16.to_ne_bytes());
    lockstep_with_table_rt_fields[74..76].copy_from_slice(&2234_u16.to_ne_bytes());
    lockstep_with_table_rt_fields[76..80].copy_from_slice(&[127, 0, 0, 1]);
    lockstep_with_table_rt_fields[81] = DataVc::Realtime.vcid();
    let decoded_lockstep =
        LinkConfig::from_table_bytes(&lockstep_with_table_rt_fields).expect("valid lockstep table");
    assert_eq!(decoded_lockstep, scenario_cfg);
    assert_eq!(decoded_lockstep.vc_for_apid(0x0008), None);

    // NativeRealtime mode falls back to default_vc when APID is not in apid_routes.
    let mut realtime = scenario_cfg;
    realtime.mode = LinkMode::NativeRealtime(RealtimeConfig {
        default_vc: Some(DataVc::Realtime),
        rx_port: NonZeroU16::new(11234).expect("non-zero rx_port"),
        tx_port: NonZeroU16::new(12234).expect("non-zero tx_port"),
        ground_ipv4: [10, 0, 0, 2],
    });
    let rt_bytes = realtime.to_table_bytes();
    let decoded_rt = LinkConfig::from_table_bytes(&rt_bytes).expect("valid realtime table bytes");
    assert_eq!(decoded_rt, realtime);
    assert_eq!(decoded_rt.vc_for_apid(0x0000), Some(DataVc::Realtime));
    assert_eq!(decoded_rt.vc_for_apid(0x0008), Some(DataVc::Realtime));
    assert_eq!(decoded_rt.vc_for_apid(apid::PAYLOAD), Some(DataVc::Payload));

    // NativeRealtime with default_vc = None refuses unmapped APIDs.
    let mut rt_no_default = realtime;
    rt_no_default.mode = LinkMode::NativeRealtime(RealtimeConfig {
        default_vc: None,
        rx_port: NonZeroU16::new(11234).expect("non-zero rx_port"),
        tx_port: NonZeroU16::new(12234).expect("non-zero tx_port"),
        ground_ipv4: [10, 0, 0, 2],
    });
    let rt_no_def_bytes = rt_no_default.to_table_bytes();
    let decoded_rt_no_def = LinkConfig::from_table_bytes(&rt_no_def_bytes)
        .expect("valid realtime table without default");
    assert_eq!(decoded_rt_no_def.vc_for_apid(0x0008), None);
}

#[test]
fn test_link_config_from_table_bytes_validation_errors() {
    let valid = LinkConfig::from_scenario().to_table_bytes();

    // Wrong length (too short / too long)
    assert_eq!(
        LinkConfig::from_table_bytes(&valid[..LinkConfig::TABLE_SIZE - 1]),
        Err(ErrorKind::InvalidConfig)
    );
    let mut too_long = valid.to_vec();
    too_long.push(0);
    assert_eq!(
        LinkConfig::from_table_bytes(&too_long),
        Err(ErrorKind::InvalidConfig)
    );

    let assert_invalid = |mutate: fn(&mut [u8; LinkConfig::TABLE_SIZE])| {
        let mut buf = valid;
        mutate(&mut buf);
        assert_eq!(
            LinkConfig::from_table_bytes(&buf),
            Err(ErrorKind::InvalidConfig)
        );
    };

    // Invalid SCID (> 10-bit 0x03FF)
    assert_invalid(|b| b[24..26].copy_from_slice(&0x0400_u16.to_ne_bytes()));

    // Invalid frame lengths (tm_frame_length != TM_FRAME_LEN; tc_max_frame_length out of bounds)
    assert_invalid(|b| b[26..28].copy_from_slice(&0_u16.to_ne_bytes()));
    assert_invalid(|b| {
        b[26..28].copy_from_slice(
            &(u16::try_from(TM_FRAME_LEN).expect("fits in u16") - 1).to_ne_bytes(),
        );
    });
    assert_invalid(|b| b[28..30].copy_from_slice(&0_u16.to_ne_bytes()));
    assert_invalid(|b| b[28..30].copy_from_slice(&8_u16.to_ne_bytes()));
    assert_invalid(|b| {
        b[28..30].copy_from_slice(&6_u16.to_ne_bytes());
        b[31] = 0; // tc_fecf = false requires >= 7 bytes with SegmentHeaderUse::Present
    });
    assert_invalid(|b| {
        b[28..30].copy_from_slice(
            &(u16::try_from(TC_FRAME_AREA_LEN).expect("fits in u16") + 1).to_ne_bytes(),
        );
    });

    // Minimum valid tc_max_frame_length (9 with tc_fecf = true, 7 with tc_fecf = false)
    // must be accepted by both LinkConfig::from_table_bytes and UplinkState::new.
    for (min_len, fecf) in [(9_u16, true), (7_u16, false)] {
        let mut min_buf = valid;
        min_buf[28..30].copy_from_slice(&min_len.to_ne_bytes());
        min_buf[31] = u8::from(fecf);
        let cfg = LinkConfig::from_table_bytes(&min_buf).expect("min tc_max_frame_length accepted");
        assert!(UplinkState::new(&cfg, UplinkMode::Frames).is_ok());
    }

    // tm_fecf must be true (1), and booleans must be 0 or 1
    assert_invalid(|b| b[30] = 0);
    assert_invalid(|b| b[30] = 2);
    assert_invalid(|b| b[31] = 0xFF);

    // Invalid COP-1 W (must be even in 2..=254) or status field > 7
    for bad_w_val in [0_u8, 1, 3, 255] {
        let mut bad_w = valid;
        bad_w[32] = bad_w_val;
        assert_eq!(
            LinkConfig::from_table_bytes(&bad_w),
            Err(ErrorKind::InvalidConfig)
        );
    }
    assert_invalid(|b| b[33] = 8);

    // Too many routes (> MAX_APID_ROUTES) & non-zero reserved header bytes
    assert_invalid(|b| b[35] = u8::try_from(MAX_APID_ROUTES).expect("fits in u8") + 1);
    assert_invalid(|b| b[36] = 1);

    // Route validation: APID > 0x07FF, invalid VCID, non-zero reserved, duplicate APID
    assert_invalid(|b| b[40..42].copy_from_slice(&0x0800_u16.to_ne_bytes()));
    assert_invalid(|b| b[42] = 7);
    assert_invalid(|b| b[43] = 1);
    assert_invalid(|b| b[44..46].copy_from_slice(&apid::HOUSEKEEPING.to_ne_bytes()));

    // NativeRealtime / UDP config validation: invalid mode, invalid default_vc, non-zero reserved2,
    // and zero rx_port/tx_port in NativeRealtime mode.
    assert_invalid(|b| b[80] = 2);
    assert_invalid(|b| b[81] = 2);
    assert_invalid(|b| b[81] = 7);
    assert_invalid(|b| b[82] = 1);
    assert_invalid(|b| b[87] = 1);
    assert_invalid(|b| {
        b[80] = 1;
        b[72..74].copy_from_slice(&0_u16.to_ne_bytes());
        b[74..76].copy_from_slice(&2234_u16.to_ne_bytes());
    });
    assert_invalid(|b| {
        b[80] = 1;
        b[72..74].copy_from_slice(&1234_u16.to_ne_bytes());
        b[74..76].copy_from_slice(&0_u16.to_ne_bytes());
    });
}

#[test]
fn test_downlink_validation_and_zero_slot_period() {
    let mut bad_len_cfg = LinkConfig::from_scenario();
    bad_len_cfg.tm_frame_length = u16::try_from(TM_FRAME_LEN).expect("fits in u16") - 1;
    assert_eq!(
        DownlinkState::new(&bad_len_cfg)
            .expect_err("reject wrong tm_frame_length")
            .kind(),
        ErrorKind::InvalidConfig
    );
    assert_eq!(
        Link::new(Level::Frames, &bad_len_cfg)
            .expect_err("reject wrong tm_frame_length")
            .kind(),
        ErrorKind::InvalidConfig
    );

    let mut bad_fecf_cfg = LinkConfig::from_scenario();
    bad_fecf_cfg.tm_fecf = false;
    assert_eq!(
        DownlinkState::new(&bad_fecf_cfg)
            .expect_err("reject tm_fecf == false")
            .kind(),
        ErrorKind::InvalidConfig
    );
    assert_eq!(
        Link::new(Level::Frames, &bad_fecf_cfg)
            .expect_err("reject tm_fecf == false")
            .kind(),
        ErrorKind::InvalidConfig
    );

    // Uplink tc_max_frame_length > TC_FRAME_AREA_LEN is rejected
    let mut bad_tc_len_cfg = LinkConfig::from_scenario();
    bad_tc_len_cfg.tc_max_frame_length = u16::try_from(TC_FRAME_AREA_LEN).expect("fits in u16") + 1;
    assert_eq!(
        UplinkState::new(&bad_tc_len_cfg, UplinkMode::Frames)
            .expect_err("reject tc_max_frame_length > TC_FRAME_AREA_LEN")
            .kind(),
        ErrorKind::InvalidConfig
    );
    assert_eq!(
        Link::new(Level::Frames, &bad_tc_len_cfg)
            .expect_err("reject tc_max_frame_length > TC_FRAME_AREA_LEN")
            .kind(),
        ErrorKind::InvalidConfig
    );

    // Zero slot period disables next_deadline via NonZeroU64
    let mut zero_slot_cfg = LinkConfig::from_scenario();
    zero_slot_cfg.frame_slot_us = 0;
    let mut link = Link::new(Level::Frames, &zero_slot_cfg).expect("create link with zero slot");
    let mut down_sink = TestDownlinkSink::default();
    let pkt = make_space_packet(apid::HOUSEKEEPING, false, 1, &[1, 2, 3]);
    link.accept_packet(Instant::from_micros(0), &pkt, &mut down_sink);
    assert_eq!(link.next_deadline(Instant::from_micros(0)), None);
}
