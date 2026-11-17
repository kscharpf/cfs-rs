//! Ground UDP verification test for `link_lab` `NativeRealtime` mode.
//!
//! Marked `#[ignore]` so `cargo test --workspace` (`make check`) skips it on the host;
//! `scripts/cfs-realtime.sh` (`make realtime`) runs it against a live `core-cpu1` node.

#![expect(
    clippy::expect_used,
    reason = "integration test fails loudly with diagnostic messages"
)]

use std::net::UdpSocket;
use std::time::{Duration, Instant};

use aldis::core::ids::FrameSeqNum;
use aldis::core::security::SecurityLayout;
use aldis::link::common::ValidPvns;
use aldis::link::extractor::{
    ExtractorInput, ExtractorOutput, FieldIn, PacketExtractor, PacketExtractorConfig, PacketRun,
};
use aldis::link::protocol::{Tm, TmParams};
use aldis::tc::{
    Clcw, FrameData, FrameFields, FrameType, MapConfig, PhysicalChannelConfig, SegmentHeader,
    SegmentHeaderUse, SequenceFlags, TcMapId, TcSpacecraftId, TcVcId, VcConfig, VcTable, encode,
};
use aldis::tm::{TmFrame, TmPhysicalChannelConfig};
use link_lab_core::{TM_DATA_FIELD_LEN, TM_FRAME_LEN};
use refapp_scenario::generated::{SPACECRAFT_ID, tc, tm};

const CFE_ES_CMD_MID: u16 = 0x1806;
const CFE_ES_HK_TLM_MID: u16 = 0x0800;
const CFE_EVS_HK_TLM_MID: u16 = 0x0801;
const CFE_SB_HK_TLM_MID: u16 = 0x0803;
const CFE_TBL_HK_TLM_MID: u16 = 0x0804;
const CFE_TIME_HK_TLM_MID: u16 = 0x0805;
const CFE_EVS_LONG_EVENT_MSG_MID: u16 = 0x0808;
const CFE_EVS_SHORT_EVENT_MSG_MID: u16 = 0x0809;
const CFE_ES_NOOP_INF_EID: u16 = 3;

const TM_MAX_PACKET_LEN: usize = 512;
#[expect(
    clippy::as_conversions,
    reason = "usize::from is not const in stable Rust"
)]
const _: () = assert!(TM_MAX_PACKET_LEN == tm::MAX_PACKET_LENGTH as usize);
const TM_EXTRACTOR_AREA_LEN: usize = TM_MAX_PACKET_LEN.saturating_add(TM_DATA_FIELD_LEN);

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
    .expect("valid tc table")
}

/// Builds an 8-byte `CFE_ES_NoopCmd_t` Space Packet (`MID = CFE_ES_CMD_MID`, func code `0`).
const fn build_cfe_es_noop_packet(seq: u16) -> [u8; 8] {
    let stream_id = CFE_ES_CMD_MID.to_be_bytes();
    let seq_ctrl = (0xC000_u16 | (seq & 0x3FFF)).to_be_bytes();
    // Packet Data Length = (total_len - 7) = 8 - 7 = 1
    let data_len = 1_u16.to_be_bytes();
    [
        stream_id[0],
        stream_id[1],
        seq_ctrl[0],
        seq_ctrl[1],
        data_len[0],
        data_len[1],
        0, // FunctionCode = CFE_ES_NOOP_CC (0)
        0, // Checksum
    ]
}

fn encode_tc_frame(table: &VcTable<1, 2>, packet: &[u8]) -> Vec<u8> {
    let scid = TcSpacecraftId::new(SPACECRAFT_ID).expect("valid scid");
    let vc = TcVcId::new(tc::VCID).expect("valid vc");
    let fields = FrameFields {
        frame_type: FrameType::Bd,
        spacecraft_id: scid,
        vc_id: vc,
        sequence_number: FrameSeqNum::new(0),
    };
    let data = FrameData::Fdu {
        segment_header: Some(SegmentHeader {
            sequence_flags: SequenceFlags::Unsegmented,
            map_id: TcMapId::new(tc::MAP_COMMANDS).expect("valid map"),
        }),
        user_data: packet,
    };
    let mut out = vec![0_u8; usize::from(tc::MAX_FRAME_LENGTH)];
    let len = encode(table, fields, data, &mut out).expect("encode tc frame");
    out.truncate(len);
    out
}

fn packet_mid(pkt: &[u8]) -> Option<u16> {
    let b0 = *pkt.first()?;
    let b1 = *pkt.get(1)?;
    Some(u16::from_be_bytes([b0, b1]))
}

const fn is_native_cfe_tlm_mid(mid: u16) -> bool {
    matches!(
        mid,
        CFE_ES_HK_TLM_MID
            | CFE_EVS_HK_TLM_MID
            | CFE_SB_HK_TLM_MID
            | CFE_TBL_HK_TLM_MID
            | CFE_TIME_HK_TLM_MID
            | CFE_EVS_LONG_EVENT_MSG_MID
            | CFE_EVS_SHORT_EVENT_MSG_MID
    )
}

fn is_es_noop_event(pkt: &[u8]) -> bool {
    if packet_mid(pkt) != Some(CFE_EVS_LONG_EVENT_MSG_MID) {
        return false;
    }
    // CFE_EVS_LongEventTlm_t layout:
    //   0..16:  CFE_MSG_TelemetryHeader_t
    //  16..36:  AppName[20]
    //  36..38:  EventID (native u16)
    //  38..48:  EventType, SpacecraftID, ProcessorID
    //  48..170: Message[122]
    let Some(app_name) = pkt.get(16..36) else {
        return false;
    };
    let Some(eid_bytes) = pkt.get(36..38) else {
        return false;
    };
    let Ok(eid_arr) = <[u8; 2]>::try_from(eid_bytes) else {
        return false;
    };
    let event_id = u16::from_ne_bytes(eid_arr);
    let Some(msg_bytes) = pkt.get(48..170) else {
        return false;
    };
    app_name.starts_with(b"CFE_ES\0")
        && event_id == CFE_ES_NOOP_INF_EID
        && msg_bytes.windows(13).any(|w| w == b"No-op command")
}

#[test]
#[ignore = "requires live core-cpu1 realtime node"]
fn realtime_tm_tc_roundtrip() {
    let downlink_sock = UdpSocket::bind("127.0.0.1:2234").expect("bind UDP 2234 for TM frames");
    downlink_sock
        .set_read_timeout(Some(Duration::from_millis(500)))
        .expect("set read timeout");

    let uplink_sock = UdpSocket::bind("127.0.0.1:0").expect("bind ephemeral UDP socket for TC");
    let tc_table = build_tc_table();
    let noop_pkt = build_cfe_es_noop_packet(1);
    let tc_frame = encode_tc_frame(&tc_table, &noop_pkt);

    let pc = TmPhysicalChannelConfig::new(TM_FRAME_LEN, true).expect("valid TM physical config");
    let ext_cfg = PacketExtractorConfig::<Tm>::new(
        TmParams,
        ValidPvns::SPACE,
        usize::from(tm::MAX_PACKET_LENGTH),
    )
    .expect("valid extractor config");
    let mut extractor = PacketExtractor::<Tm, TM_MAX_PACKET_LEN, TM_DATA_FIELD_LEN>::new(ext_cfg)
        .expect("create packet extractor");
    let mut area = [0_u8; TM_EXTRACTOR_AREA_LEN];

    let mut frame_buf = [0_u8; 512];
    let mut valid_tm_frames = 0_usize;
    let mut saw_native_packet = false;
    let mut sent_noop = false;
    let mut saw_noop_event = false;

    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        let recv_res = downlink_sock.recv_from(&mut frame_buf);
        if let Err(ref err) = recv_res
            && (err.kind() == std::io::ErrorKind::WouldBlock
                || err.kind() == std::io::ErrorKind::TimedOut)
        {
            continue;
        }
        let (n, _) = recv_res.expect("UDP recv on 2234 failed");
        let raw = frame_buf.get(..n).expect("valid recv length");
        assert_eq!(n, TM_FRAME_LEN, "TM frame length mismatch");

        let frame = TmFrame::decode(raw, &pc).expect("TM frame decode + FECF valid");
        let ocf = frame.ocf().expect("OCF present in TM frame");
        let clcw = Clcw::decode(ocf.to_octets()).expect("valid CLCW in OCF");
        assert_eq!(clcw.vc_id.get(), tc::VCID);
        valid_tm_frames = valid_tm_frames.saturating_add(1);

        if frame.header().vcid.get() == tm::VCID_REALTIME {
            let actions = extractor
                .step(
                    ExtractorInput::DataField(FieldIn {
                        field: frame.data_field(),
                        header_pointer: Some(frame.header().first_header_pointer.get()),
                        gap: false,
                    }),
                    &mut area,
                )
                .expect("extractor step");

            for action in actions.iter() {
                if let ExtractorOutput::Packets { run, .. } = action {
                    for pkt in PacketRun::new(*run, &area).expect("valid packet run") {
                        if let Some(mid) = packet_mid(pkt)
                            && is_native_cfe_tlm_mid(mid)
                        {
                            saw_native_packet = true;
                        }
                        if is_es_noop_event(pkt) {
                            saw_noop_event = true;
                        }
                    }
                }
            }
        }

        // Wait until we've seen at least one native TM packet (or a few frames)
        // before sending CFE_ES_NOOP_CC so startup burst has drained.
        if !sent_noop && (saw_native_packet || valid_tm_frames >= 10) {
            let sent = uplink_sock
                .send_to(&tc_frame, "127.0.0.1:1234")
                .expect("send TC frame to UDP 1234");
            assert_eq!(sent, tc_frame.len());
            sent_noop = true;
        }

        if saw_native_packet && saw_noop_event {
            break;
        }
    }

    assert!(
        valid_tm_frames > 0,
        "received zero valid TM frames on UDP 2234"
    );
    assert!(
        saw_native_packet,
        "did not observe any native cFE EVS/HK Space Packet inside TM frames"
    );
    assert!(sent_noop, "never sent CFE_ES_NOOP_CC TC frame");
    assert!(
        saw_noop_event,
        "did not receive CFE_ES_NOOP_INF_EID EVS event packet in TM frame after sending CFE_ES_NOOP_CC"
    );
}
