//! `link-lab-core`: Sans-I/O data link logic.
//!
//! Specification: `docs/design/link-lab.md` §4.3 and reference application §4.
//! Pure `no_std` state machine, heap-free, panic-free, flight lint set.

#![no_std]
#![forbid(unsafe_code)]

pub mod config;
pub mod downlink;
pub mod event;
pub mod uplink;

use aldis::core::{ErrorKind, Result};
use aldis::spp::SpacePacket;
pub use config::{DataVc, LinkConfig};
pub use downlink::{DATA_VCS, DownlinkState, TM_DATA_FIELD_LEN, TM_FRAME_LEN, TM_QUEUE_LEN};
pub use event::{
    APP_DOWNLINK_REFUSED, APP_FARM1_STATE, APP_FARM1_UNLOCK, APP_FRAME_DISCARDED,
    APP_FRAME_REJECTED, APP_PACKET_REJECTED, APP_SDU_TOO_LONG, APP_SEQUENCE_VIOLATION,
    APP_UNKNOWN_MAP, LAYER_FARM1, LAYER_RECEIVER,
};
pub use refapp_scenario::Instant;
pub use uplink::{
    TC_ASSEMBLY_LEN, TC_FRAME_AREA_LEN, TC_MAX_FRAME_LEN, TC_MAX_PACKET_LEN,
    TC_REASSEMBLY_AREA_LEN, UplinkBuffers, UplinkState,
};

/// Extracts the 11-bit APID from a valid Space Packet byte slice.
#[must_use]
pub fn packet_apid(packet: &[u8]) -> Option<u16> {
    SpacePacket::decode_exact(packet)
        .ok()
        .map(|p| p.apid().get())
}

/// Data link level of operation (§4.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// Packet-level link: one Space Packet per link unit.
    Packets,
    /// Frame-level link: TM downlink and TC uplink frames with Type-BD.
    Frames,
    /// COP-1 link: TM downlink and TC uplink frames with COP-1 FARM-1.
    Cop1,
}

/// Sink for uplink events and extracted command packets.
pub trait UplinkSink {
    /// Delivers one extracted Space Packet from an uplink unit.
    fn command_packet(&mut self, packet: &[u8]);

    /// Reports a link application event.
    fn event(&mut self, code: u16, args: &[u64]) {
        let _ = (code, args);
    }
}

/// Sink for downlink events and emitted link units.
pub trait DownlinkSink {
    /// Emits one downlink unit (Space Packet at `Packets` level, TM frame at `Frames`/`Cop1`).
    fn unit(&mut self, unit: &[u8]);

    /// Reports a link application event.
    fn event(&mut self, code: u16, args: &[u64]) {
        let _ = (code, args);
    }
}

/// The sans-I/O data link layer state machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    level: Level,
    config: LinkConfig,
    downlink: DownlinkState,
    uplink: UplinkState,
    slot_field_buf: [u8; TM_DATA_FIELD_LEN],
    slot_frame_buf: [u8; TM_FRAME_LEN],
    up_frame_area: [u8; TC_FRAME_AREA_LEN],
    up_fdu_buf: [u8; TC_FRAME_AREA_LEN],
    up_reassembly_area: [u8; TC_REASSEMBLY_AREA_LEN],
}

const _: () = assert!(core::mem::size_of::<Link>() <= 8192);

impl Link {
    /// Creates a new data link engine at the specified `level` using `config`.
    ///
    /// # Errors
    /// Returns `ErrorKind::InvalidConfig` if managed parameters fail engine validation.
    pub fn new(level: Level, config: &LinkConfig) -> Result<Self> {
        Ok(Self {
            level,
            config: *config,
            downlink: DownlinkState::new(config)?,
            uplink: UplinkState::new(config)?,
            slot_field_buf: [0_u8; TM_DATA_FIELD_LEN],
            slot_frame_buf: [0_u8; TM_FRAME_LEN],
            up_frame_area: [0_u8; TC_FRAME_AREA_LEN],
            up_fdu_buf: [0_u8; TC_FRAME_AREA_LEN],
            up_reassembly_area: [0_u8; TC_REASSEMBLY_AREA_LEN],
        })
    }

    /// The link level of operation.
    #[must_use]
    pub const fn level(&self) -> Level {
        self.level
    }

    /// The managed parameters configuration.
    #[must_use]
    pub const fn config(&self) -> &LinkConfig {
        &self.config
    }

    /// Processes one delivered uplink unit.
    ///
    /// At `Packets` level, unit is decoded as a Space Packet and passed to `sink.command_packet`.
    /// At `Frames`/`Cop1` level, unit passes through TC reception, FARM-1, and MAP reassembly,
    /// delivering extracted command packets to `sink.command_packet` and events to `sink.event`.
    pub fn accept_unit(&mut self, now: Instant, unit: &[u8], sink: &mut dyn UplinkSink) {
        match self.level {
            Level::Packets => match SpacePacket::decode_exact(unit) {
                Ok(_) => sink.command_packet(unit),
                Err(e) => sink.event(APP_PACKET_REJECTED, &[u64::from(e.kind().code())]),
            },
            Level::Frames | Level::Cop1 => {
                let mut buffers = UplinkBuffers {
                    frame_area: &mut self.up_frame_area,
                    fdu_buf: &mut self.up_fdu_buf,
                    reassembly_area: &mut self.up_reassembly_area,
                };
                self.uplink
                    .accept(now, unit, self.level == Level::Cop1, &mut buffers, sink);
            }
        }
    }

    /// Submits one telemetry packet from an on-board application to the downlink.
    ///
    /// At `Packets` level, validates the packet and emits it directly as a link unit via `sink.unit`.
    /// At `Frames`/`Cop1` level, routes APID to Virtual Channel and submits to its generator.
    pub fn accept_packet(&mut self, now: Instant, packet: &[u8], sink: &mut dyn DownlinkSink) {
        let packet_view = match SpacePacket::decode_exact(packet) {
            Ok(p) => p,
            Err(e) => {
                sink.event(APP_DOWNLINK_REFUSED, &[u64::from(e.kind().code())]);
                return;
            }
        };

        match self.level {
            Level::Packets => {
                sink.unit(packet);
            }
            Level::Frames | Level::Cop1 => {
                let apid = packet_view.apid().get();
                let Some(vc) = self.config.vc_for_apid(apid) else {
                    sink.event(
                        APP_DOWNLINK_REFUSED,
                        &[u64::from(ErrorKind::UnknownChannel.code())],
                    );
                    return;
                };
                if let Err(e) = self.downlink.submit(vc, packet, now) {
                    sink.event(
                        APP_DOWNLINK_REFUSED,
                        &[u64::from(e.kind().code()), u64::from(vc.vcid())],
                    );
                }
            }
        }
    }

    /// Transmit slot processing at `now`.
    ///
    /// Emits exactly one TM frame unit via `sink.unit` at frame level when `tx_slot` is true.
    /// At `Packets` level, slot has no effect.
    pub fn slot(&mut self, now: Instant, tx_slot: bool, sink: &mut dyn DownlinkSink) {
        if !tx_slot {
            return;
        }

        match self.level {
            Level::Packets => {}
            Level::Frames | Level::Cop1 => {
                self.downlink.ocf = self.uplink.farm.clcw().to_octets();
                if let Err(e) = self.downlink.slot(
                    now,
                    &mut self.slot_field_buf,
                    &mut self.slot_frame_buf,
                    sink,
                ) {
                    sink.event(APP_DOWNLINK_REFUSED, &[u64::from(e.kind().code())]);
                }
            }
        }
    }

    /// The earliest release deadline across generators after `now`.
    #[must_use]
    pub fn next_deadline(&self, now: Instant) -> Option<Instant> {
        match self.level {
            Level::Packets => None,
            Level::Frames | Level::Cop1 => {
                self.downlink.next_deadline(now, self.config.frame_slot_us)
            }
        }
    }
}
