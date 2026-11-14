//! Differential tests driving `Link` with golden traces `a1-*`, `a2-*`, and `a3-*`.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    clippy::panic,
    clippy::uninlined_format_args,
    reason = "integration tests use standard test ergonomics (assertions, panics, indexing, unwrap)"
)]

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use link_lab_core::{
    APP_DOWNLINK_REFUSED, APP_FARM1_STATE, APP_FARM1_UNLOCK, APP_FRAME_DISCARDED,
    APP_FRAME_REJECTED, APP_PACKET_REJECTED, APP_SDU_TOO_LONG, APP_SEQUENCE_VIOLATION,
    APP_UNKNOWN_MAP, DownlinkSink, Level, Link, LinkConfig, UplinkSink,
};
use refapp_scenario::Instant;
use serde_json::Value;

#[derive(Default)]
struct TestUplinkSink {
    command_packets: Vec<(u64, Vec<u8>)>,
    events: Vec<(u64, u16, Vec<u64>)>,
    current_time: u64,
}

impl UplinkSink for TestUplinkSink {
    fn command_packet(&mut self, packet: &[u8]) {
        self.command_packets
            .push((self.current_time, packet.to_vec()));
    }

    fn event(&mut self, code: u16, args: &[u64]) {
        self.events.push((self.current_time, code, args.to_vec()));
    }
}

#[derive(Default)]
struct TestDownlinkSink {
    units: Vec<(u64, Vec<u8>)>,
    events: Vec<(u64, u16, Vec<u64>)>,
    current_time: u64,
}

impl DownlinkSink for TestDownlinkSink {
    fn unit(&mut self, unit: &[u8]) {
        self.units.push((self.current_time, unit.to_vec()));
    }

    fn event(&mut self, code: u16, args: &[u64]) {
        self.events.push((self.current_time, code, args.to_vec()));
    }
}

fn unhex(s: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(s.len() / 2);
    let mut chars = s.chars();
    while let (Some(hi), Some(lo)) = (chars.next(), chars.next()) {
        let h = hi.to_digit(16).expect("invalid hex hi");
        let l = lo.to_digit(16).expect("invalid hex lo");
        #[expect(clippy::as_conversions, reason = "u32 to u8 is safe")]
        bytes.push(((h << 4) | l) as u8);
    }
    bytes
}

#[derive(Debug)]
enum Fault {
    Flip { octet: usize, bit: u8 },
    Truncate { octets: usize },
}

#[expect(
    clippy::too_many_lines,
    reason = "trace interpreter loop handles multiple step record types and assertions"
)]
fn run_trace(case_name: &str) {
    let trace_path = format!("tests/golden/{case_name}.jsonl");
    let file = File::open(Path::new(&trace_path)).expect("trace file should exist");
    let reader = BufReader::new(file);

    let mut lines = reader.lines();
    let header_line = lines
        .next()
        .expect("header line should exist")
        .expect("header line read");
    let header: Value = serde_json::from_str(&header_line).expect("header parse");

    let case_id = header["case"].as_str().expect("case id in header");
    let level = if case_id.starts_with("a1-") {
        Level::Packets
    } else if case_id.starts_with("a2-") {
        Level::Frames
    } else if case_id.starts_with("a3-") {
        Level::Cop1
    } else {
        panic!("unexpected case: {case_id}");
    };

    let mut up_faults: BTreeMap<u64, Vec<Fault>> = BTreeMap::new();
    let mut up_sent: BTreeMap<u64, Vec<u8>> = BTreeMap::new();
    let mut up_delivered: BTreeMap<u64, Vec<Vec<u8>>> = BTreeMap::new();
    let mut packet_out_flight: BTreeMap<u64, Vec<Vec<u8>>> = BTreeMap::new();
    let mut expected_down_units: Vec<(u64, Vec<u8>)> = Vec::new();
    let mut expected_link_events: Vec<(u64, u16, Vec<u64>)> = Vec::new();
    let mut expected_app_intake: Vec<(u64, u16, Vec<u64>)> = Vec::new();
    let mut max_t_us = 0u64;

    for line_res in lines {
        let line = line_res.expect("line read");
        let rec: Value = serde_json::from_str(&line).expect("json parse");
        let t_us = rec.get("t_us").and_then(Value::as_u64).unwrap_or(0);
        if t_us > max_t_us {
            max_t_us = t_us;
        }

        let kind = rec["kind"].as_str().unwrap_or_default();
        match kind {
            "fault" => {
                if rec["dir"].as_str() == Some("up") {
                    let index = rec["index"].as_u64().expect("index in fault");
                    let fault_name = rec["fault"].as_str().unwrap_or_default();
                    match fault_name {
                        "flip" => {
                            let octet = rec["args"][0].as_u64().expect("octet in flip") as usize;
                            let bit = rec["args"][1].as_u64().expect("bit in flip") as u8;
                            up_faults
                                .entry(index)
                                .or_default()
                                .push(Fault::Flip { octet, bit });
                        }
                        "truncate" => {
                            let octets =
                                rec["args"][0].as_u64().expect("octets in truncate") as usize;
                            up_faults
                                .entry(index)
                                .or_default()
                                .push(Fault::Truncate { octets });
                        }
                        _ => {}
                    }
                }
            }
            "unit_sent" => {
                let dir = rec["dir"].as_str().unwrap_or_default();
                let hex_str = rec["hex"].as_str().expect("hex in unit_sent");
                let mut octets = unhex(hex_str);
                let index = rec["index"].as_u64().expect("index in unit_sent");

                if dir == "up" {
                    if let Some(faults) = up_faults.get(&index) {
                        for fault in faults {
                            match fault {
                                Fault::Flip { octet, bit } => {
                                    if let Some(byte) = octets.get_mut(*octet) {
                                        *byte ^= 0x80 >> bit;
                                    }
                                }
                                Fault::Truncate { octets: n } => {
                                    octets.truncate(*n);
                                }
                            }
                        }
                    }
                    up_sent.insert(index, octets);
                } else if dir == "down" {
                    expected_down_units.push((t_us, octets));
                }
            }
            "unit_delivered" => {
                if rec["dir"].as_str() == Some("up") {
                    let index = rec["index"].as_u64().expect("index in unit_delivered");
                    let octets = up_sent
                        .get(&index)
                        .expect("delivered unit should have been sent")
                        .clone();
                    up_delivered.entry(t_us).or_default().push(octets);
                }
            }
            "packet_out" => {
                if rec["node"].as_str() == Some("flight") {
                    let hex_str = rec["hex"].as_str().expect("hex in packet_out");
                    packet_out_flight
                        .entry(t_us)
                        .or_default()
                        .push(unhex(hex_str));
                }
            }
            "app_event" if rec["node"].as_str() == Some("flight") => {
                let code = rec["code"].as_u64().expect("code in app_event") as u16;
                let args: Vec<u64> = rec["args"]
                    .as_array()
                    .expect("args array")
                    .iter()
                    .map(|v| v.as_u64().expect("u64 arg"))
                    .collect();

                if matches!(
                    code,
                    APP_PACKET_REJECTED
                        | APP_FRAME_REJECTED
                        | APP_FRAME_DISCARDED
                        | APP_SEQUENCE_VIOLATION
                        | APP_UNKNOWN_MAP
                        | APP_SDU_TOO_LONG
                        | APP_DOWNLINK_REFUSED
                        | APP_FARM1_STATE
                        | APP_FARM1_UNLOCK
                ) {
                    expected_link_events.push((t_us, code, args));
                } else if matches!(code, 20 | 21) {
                    expected_app_intake.push((t_us, code, args));
                }
            }
            _ => {}
        }
    }

    // Build timeline of instants to step Link.
    let mut timeline: BTreeMap<u64, bool> = BTreeMap::new();
    if level == Level::Packets {
        for t in up_delivered.keys().chain(packet_out_flight.keys()) {
            timeline.insert(*t, false);
        }
    } else {
        // Multiples of 125_000 up to max_t_us
        let mut t = 0u64;
        while t <= max_t_us {
            timeline.insert(t, true);
            t = t.saturating_add(125_000);
        }
        for t in up_delivered.keys().chain(packet_out_flight.keys()) {
            timeline.entry(*t).or_insert(t % 125_000 == 0);
        }
    }

    let config = LinkConfig::from_scenario();
    let mut link = Link::new(level, &config).expect("Link::new should succeed");

    let mut uplink_sink = TestUplinkSink::default();
    let mut downlink_sink = TestDownlinkSink::default();

    for (&t_us, &is_tx_slot) in &timeline {
        let now = Instant::from_micros(t_us);
        uplink_sink.current_time = t_us;
        downlink_sink.current_time = t_us;

        // 1. Deliver any units
        if let Some(units) = up_delivered.get(&t_us) {
            for unit in units {
                link.accept_unit(now, unit, &mut uplink_sink);
            }
        }

        // 2. Accept any flight telemetry packets
        if let Some(packets) = packet_out_flight.get(&t_us) {
            for packet in packets {
                link.accept_packet(now, packet, &mut downlink_sink);
            }
        }

        // 3. Transmit slot
        link.slot(now, is_tx_slot, &mut downlink_sink);
    }

    // Assert byte-for-byte reproduction of emitted downlink units
    assert_eq!(
        downlink_sink.units.len(),
        expected_down_units.len(),
        "case {case_name}: emitted downlink unit count mismatch (got {}, expected {})",
        downlink_sink.units.len(),
        expected_down_units.len()
    );
    for (i, (actual, expected)) in downlink_sink
        .units
        .iter()
        .zip(expected_down_units.iter())
        .enumerate()
    {
        assert_eq!(
            actual.0, expected.0,
            "case {case_name}: unit {i} timestamp mismatch (got {}, expected {})",
            actual.0, expected.0
        );
        assert_eq!(
            actual.1, expected.1,
            "case {case_name}: unit {i} (t={}) octets mismatch",
            actual.0
        );
    }

    // Assert reproduction of link events
    let mut actual_link_events = uplink_sink.events;
    actual_link_events.extend(downlink_sink.events);
    assert_eq!(
        actual_link_events.len(),
        expected_link_events.len(),
        "case {case_name}: link events count mismatch (got {:?}, expected {:?})",
        actual_link_events,
        expected_link_events
    );
    for (i, (actual, expected)) in actual_link_events
        .iter()
        .zip(expected_link_events.iter())
        .enumerate()
    {
        assert_eq!(
            actual, expected,
            "case {case_name}: link event {i} mismatch (got {:?}, expected {:?})",
            actual, expected
        );
    }

    // Assert that every extracted command packet maps to expected application intake
    // (cmd_received: function and sequence count; table_received: length)
    assert_eq!(
        uplink_sink.command_packets.len(),
        expected_app_intake.len(),
        "case {case_name}: extracted command packets count mismatch (got {}, expected {})",
        uplink_sink.command_packets.len(),
        expected_app_intake.len()
    );
    for (i, (actual, expected)) in uplink_sink
        .command_packets
        .iter()
        .zip(expected_app_intake.iter())
        .enumerate()
    {
        assert_eq!(
            actual.0, expected.0,
            "case {case_name}: extracted packet {i} timestamp mismatch",
        );
        let packet = aldis::spp::SpacePacket::decode(&actual.1)
            .expect("extracted packet should be valid SpacePacket");
        let apid = packet.apid().get();
        if expected.1 == 20 {
            // cmd_received: [function, seq]
            assert_eq!(apid, 128, "case {case_name}: expected command APID 128");
            let data = packet.data_field();
            let function = data[0];
            let seq = packet.sequence_count();
            assert_eq!(
                u64::from(function),
                expected.2[0],
                "case {case_name}: packet {i} function mismatch"
            );
            assert_eq!(
                u64::from(seq.get()),
                expected.2[1],
                "case {case_name}: packet {i} seq mismatch"
            );
        } else if expected.1 == 21 {
            // table_received: [table_id, length]
            assert_eq!(apid, 129, "case {case_name}: expected table APID 129");
            let data = packet.data_field();
            let table_id = data[0];
            let table_len = u16::from_be_bytes([data[1], data[2]]);
            assert_eq!(
                u64::from(table_id),
                expected.2[0],
                "case {case_name}: table ID mismatch"
            );
            assert_eq!(
                u64::from(table_len),
                expected.2[1],
                "case {case_name}: table length mismatch"
            );
        }
    }
}

#[test]
fn diff_a1_nominal() {
    run_trace("a1-nominal");
}

#[test]
fn diff_a1_downlink_faults() {
    run_trace("a1-downlink-faults");
}

#[test]
fn diff_a1_immediate() {
    run_trace("a1-immediate");
}

#[test]
fn diff_a1_rejects() {
    run_trace("a1-rejects");
}

#[test]
fn diff_a2_nominal() {
    run_trace("a2-nominal");
}

#[test]
fn diff_a2_downlink_corrupt() {
    run_trace("a2-downlink-corrupt");
}

#[test]
fn diff_a2_downlink_loss() {
    run_trace("a2-downlink-loss");
}

#[test]
fn diff_a2_uplink_faults() {
    run_trace("a2-uplink-faults");
}

#[test]
fn diff_a3_nominal() {
    run_trace("a3-nominal");
}

#[test]
fn diff_a3_uplink_disorder() {
    run_trace("a3-uplink-disorder");
}

#[test]
fn diff_a3_uplink_limit() {
    run_trace("a3-uplink-limit");
}

#[test]
fn diff_a3_uplink_lockout() {
    run_trace("a3-uplink-lockout");
}

#[test]
fn diff_a3_uplink_loss() {
    run_trace("a3-uplink-loss");
}
