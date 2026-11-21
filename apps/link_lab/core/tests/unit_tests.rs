//! Unit tests for `link-lab-core`.
//!
//! Tests include:
//! - Bound check on `Link` struct size (<= 9728 bytes)
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

use core::net::Ipv4Addr;
use core::num::NonZeroU16;

use aldis::aos::fhec as aos_fhec;
use aldis::core::ErrorKind;
use aldis::core::crc::crc16;
use aldis::core::ids::FrameSeqNum;
use aldis::core::security::SecurityLayout;
use aldis::tc::{
    ControlCommand, FrameData, FrameFields, FrameType, MapConfig, PhysicalChannelConfig,
    SegmentHeader, SegmentHeaderUse, SequenceFlags, TcMapId, TcSpacecraftId, TcVcId, VcConfig,
    VcTable, encode,
};
use aldis::tm::{McFrameCount, TmFrame, TmPhysicalChannelConfig, VcFrameCount};
use aldis::uslp::{
    FrameKind as UslpFrameKind, MapConfig as UslpMapConfig, OcfUse,
    PhysicalChannelConfig as UslpPhysicalChannelConfig, SourceOrDestination, TfdfHeader, TfdzRule,
    TransferFrameType, Upid, UslpMapId, UslpSpacecraftId, UslpVcId, VcConfig as UslpVcConfig,
    VcParams, VcTable as UslpVcTable, VcfCountLen,
};
use link_lab_core::{
    APP_DOWNLINK_REFUSED, APP_FARM1_STATE, APP_FARM1_UNLOCK, APP_FRAME_DISCARDED,
    APP_FRAME_REJECTED, APP_GAP_DISCARD, APP_PACKET_REJECTED, APP_SEQUENCE_VIOLATION,
    APP_UNKNOWN_MAP, ApidRoute, DataVc, DownlinkSink, DownlinkState, Level, Link, LinkConfig,
    LinkMode, MAX_APID_ROUTES, MAX_REASSEMBLY_AREA_LEN, MAX_UPLINK_FRAME_LEN,
    MIN_COMMAND_PACKET_LEN, RealtimeConfig, TC_FDU_BUF_LEN, TC_FRAME_AREA_LEN, TM_FRAME_LEN,
    USLP_MAX_FRAME_LEN, USLP_MAX_PACKET_LEN, UplinkBuffers, UplinkMode, UplinkSink, UplinkState,
    Variant, is_valid_command_packet,
};
use refapp_scenario::Instant;
use refapp_scenario::generated::{SPACECRAFT_ID, aos, apid, tc, tm, uslp_up};

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
    assert!(size <= 9728, "Link size {size} exceeds 9728 bytes");
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
    assert_eq!(decoded.header().mc_frame_count, McFrameCount::new(0));
    assert_eq!(decoded.header().vc_frame_count, VcFrameCount::new(0));
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
    let mut frame_area = [0_u8; MAX_UPLINK_FRAME_LEN];
    let mut fdu_buf = [0_u8; TC_FDU_BUF_LEN];
    let mut reassembly_area = [0_u8; MAX_REASSEMBLY_AREA_LEN];

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
        ground_ipv4: Ipv4Addr::new(10, 0, 0, 2),
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
        ground_ipv4: Ipv4Addr::new(10, 0, 0, 2),
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

    // NativeRealtime / UDP config validation: invalid mode, invalid default_vc, invalid variant,
    // non-zero reserved2/reserved3, invalid AOS/USLP fields, and zero rx_port/tx_port in NativeRealtime mode.
    assert_invalid(|b| b[80] = 2);
    assert_invalid(|b| b[81] = 2);
    assert_invalid(|b| b[81] = 7);
    assert_invalid(|b| b[82] = 3);
    assert_invalid(|b| b[83] = 1);
    assert_invalid(|b| b[84..86].copy_from_slice(&222_u16.to_ne_bytes()));
    assert_invalid(|b| b[86..88].copy_from_slice(&222_u16.to_ne_bytes()));
    assert_invalid(|b| {
        b[88..90].copy_from_slice(
            &(u16::try_from(USLP_MAX_FRAME_LEN).expect("fits in u16") + 1).to_ne_bytes(),
        );
    });
    assert_invalid(|b| b[90..92].copy_from_slice(&0_u16.to_ne_bytes()));
    assert_invalid(|b| {
        b[90..92].copy_from_slice(
            &(u16::try_from(USLP_MAX_PACKET_LEN).expect("fits in u16") + 1).to_ne_bytes(),
        );
    });
    assert_invalid(|b| b[92] = 0);
    assert_invalid(|b| b[93] = 0);
    assert_invalid(|b| b[94] = 0);
    assert_invalid(|b| b[95] = 0);
    assert_invalid(|b| b[96] = 2);
    assert_invalid(|b| b[97] = 1);
    assert_invalid(|b| b[103] = 1);
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

#[test]
fn test_is_valid_command_packet() {
    // Byte 0 layout: [Version:3][Type:1][SecHdrFlag:1][APID_hi:3]
    // Version=0, Type=1 (Telecommand), SecHdrFlag=1 => 0b000_1_1_000 = 0x18.
    let min_cmd = [0x18, 0x01, 0xC0, 0x00, 0x00, 0x01, 0x00, 0x00];
    assert_eq!(min_cmd.len(), MIN_COMMAND_PACKET_LEN);
    assert!(is_valid_command_packet(&min_cmd));

    // Longer valid telecommand packet with secondary header.
    let long_cmd = [0x18, 0x01, 0xC0, 0x00, 0x00, 0x05, 0xAA, 0xBB, 1, 2, 3, 4];
    assert!(is_valid_command_packet(&long_cmd));

    // Empty and undersized packets (< 8 bytes, including 7-byte valid Space Packet).
    assert!(!is_valid_command_packet(&[]));
    assert!(!is_valid_command_packet(&min_cmd[..5]));
    assert!(!is_valid_command_packet(&min_cmd[..6]));
    assert!(!is_valid_command_packet(&min_cmd[..7]));

    // Unsupported CCSDS Version != 0 (Version = 1 => 0b001_1_1_000 = 0x38).
    let mut bad_version = min_cmd;
    bad_version[0] = 0x38;
    assert!(!is_valid_command_packet(&bad_version));

    // Telemetry packet (Type = 0, SecHdrFlag = 1 => 0b000_0_1_000 = 0x08).
    let mut tlm_pkt = min_cmd;
    tlm_pkt[0] = 0x08;
    assert!(!is_valid_command_packet(&tlm_pkt));

    // Telecommand without secondary header (Type = 1, SecHdrFlag = 0 => 0b000_1_0_000 = 0x10).
    let mut no_sec_hdr = min_cmd;
    no_sec_hdr[0] = 0x10;
    assert!(!is_valid_command_packet(&no_sec_hdr));

    // Length mismatch: header declares 6 data octets (12 total), slice is 8 bytes.
    let mut len_mismatch_short = min_cmd;
    len_mismatch_short[5] = 0x05;
    assert!(!is_valid_command_packet(&len_mismatch_short));

    // Length mismatch: header declares 2 data octets (8 total), slice is 12 bytes.
    let mut len_mismatch_long = long_cmd;
    len_mismatch_long[5] = 0x01;
    assert!(!is_valid_command_packet(&len_mismatch_long));

    // Idle packet (APID = 0x7FF) with Type = Telecommand and SecHdrFlag = 1 is invalid per CCSDS.
    let idle_with_sec_hdr = [0x1F, 0xFF, 0xC0, 0x00, 0x00, 0x01, 0x00, 0x00];
    assert!(!is_valid_command_packet(&idle_with_sec_hdr));
}

fn build_uslp_up_table() -> UslpVcTable<1, 1> {
    let scid = UslpSpacecraftId::new(SPACECRAFT_ID).expect("valid uslp scid");
    let vc = UslpVcId::new(uslp_up::VCID).expect("valid uslp vc");
    let map = UslpMapId::new(uslp_up::MAP_ID).expect("valid uslp map");
    let physical = UslpPhysicalChannelConfig::new(
        TransferFrameType::Variable,
        USLP_MAX_FRAME_LEN,
        0,
        uslp_up::FECF,
    )
    .expect("valid uslp physical");
    let params = VcParams {
        vcf_count_len_sequence: VcfCountLen::new(uslp_up::VCF_COUNT_LENGTH_SEQUENCE)
            .expect("valid seq count len"),
        vcf_count_len_expedited: VcfCountLen::new(uslp_up::VCF_COUNT_LENGTH_EXPEDITED)
            .expect("valid exp count len"),
        ocf: OcfUse::Absent,
        ..VcParams::new(TransferFrameType::Variable)
    };
    UslpVcTable::new(
        physical,
        &[UslpVcConfig::new(scid, vc, params).expect("valid uslp vc config")],
        &[UslpMapConfig::new(scid, vc, map, SecurityLayout::NONE)],
    )
    .expect("valid uslp table")
}

fn build_uslp_up_frame(
    table: &UslpVcTable<1, 1>,
    kind: UslpFrameKind,
    vcf_count: u64,
    rule: TfdzRule,
    upid: Upid,
    tfdz: &[u8],
) -> Vec<u8> {
    let scid = UslpSpacecraftId::new(SPACECRAFT_ID).expect("valid uslp scid");
    let vc = UslpVcId::new(uslp_up::VCID).expect("valid uslp vc");
    let map = UslpMapId::new(uslp_up::MAP_ID).expect("valid uslp map");
    let fields = aldis::uslp::FrameFields {
        spacecraft_id: scid,
        source_or_destination: SourceOrDestination::Destination,
        vc_id: vc,
        map_id: map,
        kind,
        vcf_count,
    };
    let content = aldis::uslp::FrameContent {
        tfdf: TfdfHeader { rule, upid },
        insert_zone: &[],
        ocf: None,
    };
    let mut out = vec![0_u8; USLP_MAX_FRAME_LEN];
    let len = aldis::uslp::encode(table, fields, content, tfdz, &mut out).expect("encode uslp");
    out.truncate(len);
    out
}

#[test]
fn test_invalid_level_variant_combinations() {
    let config = LinkConfig::from_scenario();
    for (level, variant) in [
        (Level::Packets, Variant::Aos),
        (Level::Packets, Variant::Uslp),
        (Level::Frames, Variant::Uslp),
    ] {
        assert_eq!(
            Link::new_with_variant(level, variant, &config)
                .expect_err("invalid (Level, Variant) must fail")
                .kind(),
            ErrorKind::InvalidConfig
        );
    }
    assert_eq!(
        UplinkState::new_with_variant(Variant::Uslp, &config, UplinkMode::Frames)
            .expect_err("USLP uplink requires Cop1 mode")
            .kind(),
        ErrorKind::InvalidConfig
    );

    // Valid combinations succeed and report level/variant accurately.
    for (level, variant) in [
        (Level::Packets, Variant::TmTc),
        (Level::Frames, Variant::TmTc),
        (Level::Cop1, Variant::TmTc),
        (Level::Frames, Variant::Aos),
        (Level::Cop1, Variant::Aos),
        (Level::Cop1, Variant::Uslp),
    ] {
        let link = Link::new_with_variant(level, variant, &config).expect("valid combo");
        assert_eq!(link.level(), level);
        assert_eq!(link.variant(), variant);
        assert_eq!(link.config().variant, variant);
    }
}

#[test]
fn test_aos_downlink_priority_fhec_fecf_and_vc_ocf() {
    let config = LinkConfig::from_scenario();
    let mut link = Link::new_with_variant(Level::Cop1, Variant::Aos, &config).expect("aos link");
    let mut down_sink = TestDownlinkSink::default();

    // Submit one packet on Payload (VC 1) and one on Realtime (VC 0) at t=0.
    let pl_pkt = make_space_packet(apid::PAYLOAD, false, 1, &[0x11, 0x22, 0x33, 0x44]);
    let hk_pkt = make_space_packet(apid::HOUSEKEEPING, false, 1, &[0xAA, 0xBB, 0xCC, 0xDD]);
    link.accept_packet(Instant::from_micros(0), &pl_pkt, &mut down_sink);
    link.accept_packet(Instant::from_micros(0), &hk_pkt, &mut down_sink);

    // After both deadlines (2 s), 4 slots serve VC 0, then VC 1, then VC 7 (idle M_PDU), then VC 7.
    for i in 0..4_u64 {
        let now = Instant::from_micros(2_000_000 + i * config.frame_slot_us);
        link.slot(now, true, &mut down_sink);
    }
    assert_eq!(down_sink.units.len(), 4);

    let mut vcids = Vec::new();
    for frame in &down_sink.units {
        assert_eq!(frame.len(), TM_FRAME_LEN);
        // FHEC over first 8 octets and FECF over whole frame must both verify.
        let hdr: [u8; 8] = frame[..8].try_into().expect("8-byte header");
        assert!(aos_fhec::decode(&hdr).is_ok());
        assert_eq!(crc16(frame).get(), 0);
        // TFVN = 01 (AOS Version 2) in top 2 bits of byte 0; VCID in bottom 6 bits of byte 1.
        assert_eq!(frame[0] >> 6, 0b01);
        vcids.push(frame[1] & 0x3F);
        // VC_OCF is carried on every VC (0, 1, 7) at the 4 octets before FECF.
        assert_eq!(
            &frame[TM_FRAME_LEN - 6..TM_FRAME_LEN - 2],
            &tm::OCF_INITIAL.to_be_bytes()
        );
    }
    assert_eq!(
        vcids,
        vec![
            aos::VCID_REALTIME,
            aos::VCID_PAYLOAD,
            aos::VCID_IDLE,
            aos::VCID_IDLE
        ]
    );

    // AOS config validation errors
    let mutations: [fn(&mut LinkConfig); 4] = [
        |c| c.aos_frame_length = 222,
        |c| c.aos_fhec = false,
        |c| c.aos_vc_ocf = false,
        |c| c.aos_fecf = false,
    ];
    for mutate in mutations {
        let mut bad = config;
        mutate(&mut bad);
        assert_eq!(
            DownlinkState::new_with_variant(Variant::Aos, &bad)
                .expect_err("invalid AOS config rejected")
                .kind(),
            ErrorKind::InvalidConfig
        );
    }
}

#[test]
fn test_uslp_downlink_fixed_frame_oid_and_mc_ocf() {
    let config = LinkConfig::from_scenario();
    let mut link = Link::new_with_variant(Level::Cop1, Variant::Uslp, &config).expect("uslp link");
    let mut down_sink = TestDownlinkSink::default();

    // Slot with no data produces an OID frame on VCID 63 with MC OCF and FECF.
    link.slot(Instant::from_micros(0), true, &mut down_sink);
    assert_eq!(down_sink.units.len(), 1);
    let oid = &down_sink.units[0];
    assert_eq!(oid.len(), TM_FRAME_LEN);
    assert_eq!(crc16(oid).get(), 0);
    let oid_vcid = ((oid[2] & 0x07) << 3) | (oid[3] >> 5);
    assert_eq!(oid_vcid, 63);
    assert_eq!(oid[6], 0b1000_1000); // Expedited, OCF=1, VCF Count Len=0
    assert_eq!(
        &oid[TM_FRAME_LEN - 6..TM_FRAME_LEN - 2],
        &tm::OCF_INITIAL.to_be_bytes()
    );

    // Submit a packet on VC 0 and release after deadline.
    let hk_pkt = make_space_packet(apid::HOUSEKEEPING, false, 1, &[0xDE, 0xAD, 0xBE, 0xEF]);
    link.accept_packet(Instant::from_micros(125_000), &hk_pkt, &mut down_sink);
    link.slot(Instant::from_micros(1_000_000), true, &mut down_sink);
    assert_eq!(down_sink.units.len(), 2);
    let data = &down_sink.units[1];
    assert_eq!(data.len(), TM_FRAME_LEN);
    assert_eq!(crc16(data).get(), 0);
    let data_vcid = ((data[2] & 0x07) << 3) | (data[3] >> 5);
    assert_eq!(data_vcid, 0);
    assert_eq!(data[2] & 0x08, 0); // SourceOrDestination::Source
    assert_eq!(data[6], 0b1000_1001); // Expedited, OCF=1, VCF Count Len=1
    assert_eq!(data[7], 0); // First expedited VCF count on VC 0
    // TFDZ starts at byte 11: packet followed by Encapsulation Idle Packet (PVN 111, PID 000, LoL 01).
    let tfdz = &data[11..TM_FRAME_LEN - 6];
    assert_eq!(&tfdz[..hk_pkt.len()], hk_pkt.as_slice());
    assert_eq!(tfdz[hk_pkt.len()], 0b1110_0001);

    // USLP downlink config validation errors
    let mutations: [fn(&mut LinkConfig); 2] = [
        |c| c.uslp_down_frame_length = 222,
        |c| c.uslp_down_fecf = false,
    ];
    for mutate in mutations {
        let mut bad = config;
        mutate(&mut bad);
        assert_eq!(
            DownlinkState::new_with_variant(Variant::Uslp, &bad)
                .expect_err("invalid USLP downlink config rejected")
                .kind(),
            ErrorKind::InvalidConfig
        );
    }
}

#[test]
fn test_uslp_uplink_farm1_ad_bd_lockout_and_unlock() {
    assert_eq!(APP_GAP_DISCARD, 115);

    let config = LinkConfig::from_scenario();
    let mut link = Link::new_with_variant(Level::Cop1, Variant::Uslp, &config).expect("uslp link");
    let table = build_uslp_up_table();
    let t0 = Instant::from_micros(0);
    let mut up_sink = TestUplinkSink::default();

    // 1. Sequence-controlled (Type-AD) frame seq 0 delivers command packet and advances V(R) to 1.
    let pkt0 = make_space_packet(apid::COMMANDS, true, 0, &[1, 2, 3, 4]);
    let ad0 = build_uslp_up_frame(
        &table,
        UslpFrameKind::SequenceControlled,
        0,
        TfdzRule::NoSegmentation,
        Upid::Packets,
        &pkt0,
    );
    link.accept_unit(t0, &ad0, &mut up_sink);
    assert_eq!(up_sink.packets, vec![pkt0]);
    assert_eq!(up_sink.events.len(), 0);

    // Verify CLCW in USLP downlink MC OCF reports V(R) = 1.
    let mut down_sink = TestDownlinkSink::default();
    link.slot(t0, true, &mut down_sink);
    assert_eq!(down_sink.units[0][TM_FRAME_LEN - 3], 1);

    // 2. Expedited (Type-BD) frame delivers packet without advancing V(R).
    let pkt_bd = make_space_packet(apid::COMMANDS, true, 1, &[5, 6, 7]);
    let bd = build_uslp_up_frame(
        &table,
        UslpFrameKind::Expedited,
        0,
        TfdzRule::NoSegmentation,
        Upid::Packets,
        &pkt_bd,
    );
    link.accept_unit(t0, &bd, &mut up_sink);
    assert_eq!(up_sink.packets.len(), 2);
    assert_eq!(up_sink.packets[1], pkt_bd);

    // 3. Out-of-window Type-AD triggers Lockout (APP_FARM1_STATE), then valid Type-BC Unlock recovers.
    let ad_far = build_uslp_up_frame(
        &table,
        UslpFrameKind::SequenceControlled,
        50,
        TfdzRule::NoSegmentation,
        Upid::Packets,
        &pkt_bd,
    );
    link.accept_unit(t0, &ad_far, &mut up_sink);
    assert!(
        up_sink
            .events
            .iter()
            .any(|(code, args)| *code == APP_FARM1_STATE && args == &vec![3])
    );

    let bc_unlock = build_uslp_up_frame(
        &table,
        UslpFrameKind::ProtocolControl,
        0,
        TfdzRule::NoSegmentation,
        Upid::Cop1ControlCommands,
        &[0x00],
    );
    link.accept_unit(t0, &bc_unlock, &mut up_sink);
    assert!(
        up_sink
            .events
            .iter()
            .any(|(code, _)| *code == APP_FARM1_UNLOCK)
    );

    // 4. Type-BC ControlCommand::SetVr(42) updates FARM-1 V(R) reported in CLCW MC OCF.
    let bc_set_vr = build_uslp_up_frame(
        &table,
        UslpFrameKind::ProtocolControl,
        0,
        TfdzRule::NoSegmentation,
        Upid::Cop1ControlCommands,
        &[0x82, 0x00, 42],
    );
    link.accept_unit(t0, &bc_set_vr, &mut up_sink);
    link.slot(
        Instant::from_micros(config.frame_slot_us),
        true,
        &mut down_sink,
    );
    assert_eq!(
        down_sink.units.last().expect("downlink frame")[TM_FRAME_LEN - 3],
        42
    );
}

#[test]
fn test_uslp_uplink_fecf_and_bc_rejections() {
    let config = LinkConfig::from_scenario();
    let mut link = Link::new_with_variant(Level::Cop1, Variant::Uslp, &config).expect("uslp link");
    let table = build_uslp_up_table();
    let t0 = Instant::from_micros(0);
    let mut up_sink = TestUplinkSink::default();

    // 1. Corrupted FECF is rejected with APP_FRAME_REJECTED.
    let pkt0 = make_space_packet(apid::COMMANDS, true, 0, &[1, 2, 3, 4]);
    let mut bad_crc = build_uslp_up_frame(
        &table,
        UslpFrameKind::SequenceControlled,
        0,
        TfdzRule::NoSegmentation,
        Upid::Packets,
        &pkt0,
    );
    let last = bad_crc.len() - 1;
    bad_crc[last] ^= 0xFF;
    link.accept_unit(t0, &bad_crc, &mut up_sink);
    assert_eq!(
        up_sink.events.last().map(|(c, _)| *c),
        Some(APP_FRAME_REJECTED)
    );

    // 2. Type-BC rejection: wrong UPID (Packets instead of Cop1ControlCommands).
    let bc_wrong_upid = build_uslp_up_frame(
        &table,
        UslpFrameKind::ProtocolControl,
        0,
        TfdzRule::NoSegmentation,
        Upid::Packets,
        &[0x00],
    );
    link.accept_unit(t0, &bc_wrong_upid, &mut up_sink);
    assert_eq!(
        up_sink.events.last(),
        Some(&(
            APP_FRAME_REJECTED,
            vec![u64::from(ErrorKind::ReservedValue.code())]
        ))
    );

    // 3. Type-BC rejection: valid UPID (Cop1ControlCommands), but non-NoSegmentation TFDZ rule.
    let bc_wrong_rule = build_uslp_up_frame(
        &table,
        UslpFrameKind::ProtocolControl,
        0,
        TfdzRule::StartingSegment,
        Upid::Cop1ControlCommands,
        &[0x00],
    );
    link.accept_unit(t0, &bc_wrong_rule, &mut up_sink);
    assert_eq!(
        up_sink.events.last(),
        Some(&(
            APP_FRAME_REJECTED,
            vec![u64::from(ErrorKind::ReservedValue.code())]
        ))
    );

    // 4. Type-BC rejection: valid UPID and rule, but unrecognized control command bytes.
    let bc_bad_cmd = build_uslp_up_frame(
        &table,
        UslpFrameKind::ProtocolControl,
        0,
        TfdzRule::NoSegmentation,
        Upid::Cop1ControlCommands,
        &[0xAA, 0xBB],
    );
    link.accept_unit(t0, &bc_bad_cmd, &mut up_sink);
    assert_eq!(
        up_sink.events.last(),
        Some(&(
            APP_FRAME_REJECTED,
            vec![u64::from(ErrorKind::ReservedValue.code())]
        ))
    );
    assert_eq!(up_sink.packets.len(), 0);
}

#[test]
fn test_link_config_extended_table_variants_round_trip() {
    for variant in [Variant::TmTc, Variant::Aos, Variant::Uslp] {
        let mut cfg = LinkConfig::from_scenario();
        cfg.variant = variant;
        let bytes = cfg.to_table_bytes();
        assert_eq!(bytes.len(), LinkConfig::TABLE_SIZE);
        let decoded = LinkConfig::from_table_bytes(&bytes).expect("valid variant table");
        assert_eq!(decoded, cfg);
        assert_eq!(decoded.variant, variant);
    }

    // uslp_up_max_packet_length exceeding uslp_up_max_frame_length - overhead (11 B with FECF, 9 B without)
    let valid = LinkConfig::from_scenario().to_table_bytes();
    for (frame_len, pkt_len, fecf, expect_ok) in [
        (128_u16, 200_u16, true, false),
        (128_u16, 118_u16, true, false),  // 118 + 11 = 129 > 128
        (128_u16, 117_u16, true, true),   // 117 + 11 = 128
        (128_u16, 120_u16, false, false), // 120 + 9 = 129 > 128
        (128_u16, 119_u16, false, true),  // 119 + 9 = 128
    ] {
        let mut buf = valid;
        buf[88..90].copy_from_slice(&frame_len.to_ne_bytes());
        buf[90..92].copy_from_slice(&pkt_len.to_ne_bytes());
        buf[96] = u8::from(fecf);
        assert_eq!(LinkConfig::from_table_bytes(&buf).is_ok(), expect_ok);
    }
}
