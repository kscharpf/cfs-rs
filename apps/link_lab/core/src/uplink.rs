//! Uplink state machine and frame reception.

use aldis::cop1::farm1::{Farm1, Farm1Config, Farm1Input, Farm1Notify, Farm1Output, Farm1Window};
use aldis::cop1::farm1_table::{Farm1Event, Farm1State};
use aldis::core::ids::FrameSeqNum;
use aldis::core::security::{NoSecurity, SecurityLayout};
use aldis::core::time::Instant as CoreInstant;
use aldis::core::{DecodeMode, Error, ErrorKind, Result, SecuritySlices};
use aldis::link::protocol::Tc;
use aldis::link::reassembler::{
    FduIn, MapReassembler, MapReassemblerConfig, ReassemblerInput, ReassemblerMap,
    ReassemblerOutput, ReassemblyEvent, TcReassemblerParams,
};
use aldis::link::receiver::{
    CallerDiscardReason, DiscardReason, FrameReceiver, FrameReceiverConfig, ReceiverInput,
    ReceiverOutput, TcChannel, TcVc, VcEntry,
};
use aldis::spp::SpacePacket;
use aldis::tc::{
    Clcw, ClcwStatus, FrameType, MapConfig, PhysicalChannelConfig, Reason, SegmentHeader,
    SegmentHeaderUse, TcMapId, TcSpacecraftId, TcVcId, VcConfig, VcTable, check_frame,
};
use refapp_scenario::Instant;
use refapp_scenario::generated::{cop1, tc};

use crate::UplinkSink;
use crate::config::LinkConfig;
use crate::event::{
    APP_FARM1_STATE, APP_FARM1_UNLOCK, APP_FRAME_DISCARDED, APP_FRAME_REJECTED,
    APP_PACKET_REJECTED, APP_SDU_TOO_LONG, APP_SEQUENCE_VIOLATION, APP_UNKNOWN_MAP, LAYER_FARM1,
    LAYER_RECEIVER,
};

/// Capacity of a MAP's assembly buffer: maximum packet length (§4.2).
pub const TC_ASSEMBLY_LEN: usize = 512;

/// Maximum transfer frame length in octets (§4.2).
pub const TC_FRAME_AREA_LEN: usize = 128;

/// Alias for maximum TC frame length in octets.
pub const TC_MAX_FRAME_LEN: usize = TC_FRAME_AREA_LEN;

/// Alias for maximum TC packet length in octets.
pub const TC_MAX_PACKET_LEN: usize = TC_ASSEMBLY_LEN;

/// Output area for segment reassembly: held SDU plus frame data.
pub const TC_REASSEMBLY_AREA_LEN: usize = TC_ASSEMBLY_LEN.saturating_add(TC_FRAME_AREA_LEN);

#[expect(
    clippy::as_conversions,
    reason = "usize::from is not const in stable Rust (RFC 143874)"
)]
const _: () = {
    assert!(TC_FRAME_AREA_LEN == tc::MAX_FRAME_LENGTH as usize);
    assert!(TC_ASSEMBLY_LEN == tc::MAX_PACKET_LENGTH as usize);
};

/// Scratch buffers for uplink frame reception and reassembly.
#[derive(Debug)]
pub struct UplinkBuffers<'a> {
    /// Area for frame receiver and FARM-1 output.
    pub frame_area: &'a mut [u8; TC_FRAME_AREA_LEN],
    /// Buffer for reassembling FDU with segment header.
    pub fdu_buf: &'a mut [u8; TC_FRAME_AREA_LEN],
    /// Assembly area for reassembler output.
    pub reassembly_area: &'a mut [u8; TC_REASSEMBLY_AREA_LEN],
}

/// FARM-1 delivery buffer holding accepted FDUs until reassembly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeliveryBuffer {
    held: u8,
}

impl DeliveryBuffer {
    /// Depth of the buffer (§4.2, COP-032).
    pub const DEPTH: u8 = cop1::FARM_BUFFER_DEPTH;

    /// Creates an empty delivery buffer.
    #[must_use]
    pub const fn empty() -> Self {
        Self { held: 0 }
    }

    /// Whether there is room for another accepted FDU.
    #[must_use]
    pub const fn has_room(self) -> bool {
        self.held < Self::DEPTH
    }

    /// Takes one place for an accepted FDU.
    pub const fn take(&mut self) {
        self.held = self.held.saturating_add(1);
    }

    /// Releases one place after handing the FDU to the reassembler.
    pub const fn release(&mut self) {
        self.held = self.held.saturating_sub(1);
    }

    /// Resets the buffer on Unlock.
    pub const fn reset(&mut self) {
        self.held = 0;
    }
}

/// Frame-level uplink operating mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UplinkMode {
    /// Frame level (`a2-*`): Type-BD frames only; COP-1 state events suppressed.
    Frames,
    /// COP-1 level (`a3-*`): Type-AD/Type-BC frames with FARM-1 state and unlock events.
    Cop1,
}

/// Uplink receive chain for TC frames.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UplinkState {
    /// Frame-level operating mode (`Frames` vs `Cop1`).
    mode: UplinkMode,
    /// Codec view of the channel.
    pub table: VcTable<1, 2>,
    /// Uplink virtual channel ID.
    pub vc: TcVcId,
    /// Frame receiver engine.
    pub receiver: FrameReceiver<Tc, 1>,
    /// FARM-1 engine.
    pub farm: Farm1,
    /// Segment reassembler.
    pub reassembler: MapReassembler<Tc, 2, TC_ASSEMBLY_LEN>,
    /// Reassembler configuration for purge on Unlock.
    pub reassembler_config: MapReassemblerConfig<Tc, 2>,
    /// Delivery buffer.
    pub delivery: DeliveryBuffer,
}

impl UplinkState {
    /// Initializes uplink receive chain from configuration and operating `mode`.
    ///
    /// # Errors
    /// Returns `ErrorKind::InvalidConfig` if managed parameters fail engine validation.
    pub fn new(config: &LinkConfig, mode: UplinkMode) -> Result<Self> {
        let bad = Error::new(ErrorKind::InvalidConfig);
        let scid = TcSpacecraftId::new(config.spacecraft_id).ok_or(bad)?;
        let vc = TcVcId::new(tc::VCID).ok_or(bad)?;
        let frame_len = usize::from(config.tc_max_frame_length);
        let map = |id: u8| {
            TcMapId::new(id)
                .map(|map_id| MapConfig::new(scid, vc, map_id, SecurityLayout::NONE))
                .ok_or(bad)
        };
        let header = if tc::SEGMENT_HEADER {
            SegmentHeaderUse::Present
        } else {
            SegmentHeaderUse::Absent(SecurityLayout::NONE)
        };
        let physical = PhysicalChannelConfig::new(frame_len, config.tc_fecf)?;
        let table = VcTable::new(
            physical,
            &[VcConfig::new(scid, vc, frame_len, header)?],
            &[map(tc::MAP_COMMANDS)?, map(tc::MAP_TABLE_UPLOAD)?],
        )?;
        let receiver = FrameReceiver::new(FrameReceiverConfig::new(
            TcChannel::new(physical),
            &[VcEntry::new(vc, TcVc::SERVED)],
        )?);
        let farm = Farm1::new(
            Farm1Config {
                vc_id: vc,
                status: ClcwStatus::new(config.cop1_status_field).ok_or(bad)?,
                window: Farm1Window::retransmission_allowed(u16::from(config.cop1_w))?,
            },
            FrameSeqNum::new(config.farm_initial_vr),
        );
        let max_packet = usize::from(tc::MAX_PACKET_LENGTH);
        let packets = |id: u8| {
            TcMapId::new(id)
                .ok_or(bad)
                .and_then(|map_id| ReassemblerMap::packets(vc, map_id, max_packet))
        };
        let reassembler_config = MapReassemblerConfig::new(
            TcReassemblerParams::with_segment_header(),
            &[packets(tc::MAP_COMMANDS)?, packets(tc::MAP_TABLE_UPLOAD)?],
        )?;
        let reassembler = MapReassembler::new(reassembler_config)?;
        Ok(Self {
            mode,
            table,
            vc,
            receiver,
            farm,
            reassembler,
            reassembler_config,
            delivery: DeliveryBuffer::empty(),
        })
    }

    /// Reports the configured uplink operating mode.
    #[must_use]
    pub const fn mode(&self) -> UplinkMode {
        self.mode
    }

    /// Reports current CLCW from FARM-1.
    #[must_use]
    pub const fn clcw(&self) -> Clcw {
        self.farm.clcw()
    }

    /// Handles one delivered TC frame.
    pub fn accept(
        &mut self,
        now: Instant,
        octets: &[u8],
        buffers: &mut UplinkBuffers<'_>,
        sink: &mut dyn UplinkSink,
    ) {
        let core_now = CoreInstant::from_micros(now.as_micros());
        let validated = match check_frame(octets, &self.table, DecodeMode::Strict) {
            Ok(val) => val.into_view(),
            Err(rejection) => {
                let fecf = rejection.reason() == Reason::FecfMismatch;
                self.reject_frame(core_now, fecf, rejection.error().kind(), sink);
                return;
            }
        };
        let plain = match validated.into_frame().into_plain(NoSecurity) {
            Ok(p) => p,
            Err(e) => return self.reject_frame(core_now, false, e.kind(), sink),
        };
        match self
            .receiver
            .step(ReceiverInput::Frame(plain), buffers.frame_area)
        {
            Ok(actions) => {
                if !receiver_accepts(actions.iter(), sink) {
                    return;
                }
            }
            Err(e) => return self.reject_frame(core_now, false, e.kind(), sink),
        }
        if self.mode == UplinkMode::Frames && validated.frame_type() != FrameType::Bd {
            sink.event(
                APP_FRAME_REJECTED,
                &[u64::from(ErrorKind::InvalidField.code())],
            );
            return;
        }
        let input = Farm1Input::from_tc(&validated, self.delivery.has_room());
        let (farm, actions) = match self.farm.step(input, core_now, buffers.frame_area) {
            Ok(stepped) => stepped,
            Err(e) => return self.reject_frame(core_now, false, e.kind(), sink),
        };
        self.farm = farm;
        let mut fdu = None;
        let mut discarded = None;
        let mut unlocked = false;
        let mut new_state = None;
        for action in actions.iter() {
            match action {
                Farm1Output::Notify(Farm1Notify::FrameDiscarded { event, .. }) => {
                    discarded = Some(*event);
                }
                Farm1Output::Deliver { data, .. } => fdu = Some(*data),
                Farm1Output::Notify(Farm1Notify::StateChanged { to, .. }) => new_state = Some(*to),
                Farm1Output::Notify(Farm1Notify::Unlocked) => unlocked = true,
            }
        }
        if let Some(event) = discarded {
            sink.event(APP_FRAME_DISCARDED, &[LAYER_FARM1, event_number(event)]);
        }
        if unlocked && self.mode == UplinkMode::Cop1 {
            sink.event(APP_FARM1_UNLOCK, &[]);
            self.purge(sink);
        }
        if let Some(range) = fdu {
            self.delivery.take();
            let header = validated.segment_header();
            self.reassemble(
                header,
                range.slice(buffers.frame_area),
                buffers.fdu_buf,
                buffers.reassembly_area,
                sink,
            );
            self.delivery.release();
        }
        if let Some(state) = new_state
            && self.mode == UplinkMode::Cop1
        {
            sink.event(APP_FARM1_STATE, &[state_number(state)]);
        }
    }

    fn purge(&mut self, sink: &mut dyn UplinkSink) {
        self.delivery.reset();
        let mut none = [0_u8; 0];
        let input = ReassemblerInput::Reconfigure(self.reassembler_config);
        if let Err(e) = self.reassembler.step(input, &mut none) {
            sink.event(APP_PACKET_REJECTED, &[u64::from(e.kind().code())]);
        }
    }

    fn reject_frame(
        &mut self,
        now: CoreInstant,
        fecf: bool,
        mut kind: ErrorKind,
        sink: &mut dyn UplinkSink,
    ) {
        let mut none = [0_u8; 0];
        if fecf
            && let Err(e) = self.receiver.step(
                ReceiverInput::CallerDiscard(CallerDiscardReason::CrcMismatch),
                &mut none,
            )
        {
            kind = e.kind();
        }
        match self.farm.step(Farm1Input::InvalidFrame, now, &mut none) {
            Ok((farm, _)) => self.farm = farm,
            Err(e) => kind = e.kind(),
        }
        sink.event(APP_FRAME_REJECTED, &[u64::from(kind.code())]);
    }

    fn reassemble(
        &mut self,
        header: Option<SegmentHeader>,
        user_data: Option<&[u8]>,
        fdu_buf: &mut [u8; TC_FRAME_AREA_LEN],
        reassembly_area: &mut [u8; TC_REASSEMBLY_AREA_LEN],
        sink: &mut dyn UplinkSink,
    ) {
        let filled = header.zip(user_data).and_then(|(hdr, user)| {
            let len = user.len().checked_add(1)?;
            let dst = fdu_buf.get_mut(..len)?;
            let (first, rest) = dst.split_first_mut()?;
            *first = hdr.octet();
            for (d, s) in rest.iter_mut().zip(user) {
                *d = *s;
            }
            Some(len)
        });
        let Some(data) = filled.and_then(|len| fdu_buf.get(..len)) else {
            sink.event(
                APP_PACKET_REJECTED,
                &[u64::from(ErrorKind::InternalError.code())],
            );
            return;
        };
        let input = ReassemblerInput::Fdu(FduIn {
            vc: self.vc,
            meta: (),
            data,
        });
        let actions = match self.reassembler.step(input, reassembly_area) {
            Ok(acts) => acts,
            Err(e) => {
                sink.event(APP_PACKET_REJECTED, &[u64::from(e.kind().code())]);
                return;
            }
        };
        for action in actions.iter() {
            match action {
                ReassemblerOutput::Event { event, map, .. } => {
                    let code = match event {
                        ReassemblyEvent::SequenceViolation => APP_SEQUENCE_VIOLATION,
                        ReassemblyEvent::UnknownMap => APP_UNKNOWN_MAP,
                        ReassemblyEvent::SduTooLong => APP_SDU_TOO_LONG,
                        ReassemblyEvent::GapDiscard => continue,
                    };
                    sink.event(code, &[u64::from(*map)]);
                }
                ReassemblerOutput::Packets { run, count, .. } => {
                    deliver_run(run.slice(reassembly_area), *count, sink);
                }
                ReassemblerOutput::Sdu { .. }
                | ReassemblerOutput::StreamData { .. }
                | ReassemblerOutput::Discarded { .. } => {}
            }
        }
    }
}

fn deliver_run(run: Option<&[u8]>, count: usize, sink: &mut dyn UplinkSink) {
    let mut rest = run.unwrap_or_default();
    for _ in 0..count {
        let split = SpacePacket::peek_length(rest).and_then(|len| {
            rest.split_at_checked(len)
                .ok_or(Error::new(ErrorKind::InternalError))
        });
        match split {
            Ok((packet, tail)) => {
                sink.command_packet(packet);
                rest = tail;
            }
            Err(e) => {
                sink.event(APP_PACKET_REJECTED, &[u64::from(e.kind().code())]);
                return;
            }
        }
    }
}

fn receiver_accepts<'a>(
    actions: impl Iterator<Item = &'a ReceiverOutput<Tc>>,
    sink: &mut dyn UplinkSink,
) -> bool {
    let mut accepted = false;
    for action in actions {
        match action {
            ReceiverOutput::TcFrame { .. } => accepted = true,
            ReceiverOutput::FrameDiscarded { reason, .. } => {
                if let Some(reason_code) = receiver_reason(*reason) {
                    sink.event(APP_FRAME_DISCARDED, &[LAYER_RECEIVER, reason_code]);
                }
                return false;
            }
            ReceiverOutput::FrameGap { .. }
            | ReceiverOutput::CountDiscontinuity { .. }
            | ReceiverOutput::LossFlag
            | ReceiverOutput::DataField { .. }
            | ReceiverOutput::Ocf { .. }
            | ReceiverOutput::FshSdu { .. }
            | ReceiverOutput::InsertSdu { .. }
            | ReceiverOutput::ProtocolControl { .. } => {}
        }
    }
    accepted
}

const fn receiver_reason(reason: DiscardReason) -> Option<u64> {
    match reason {
        DiscardReason::UnknownChannel => Some(1),
        DiscardReason::NotServed => Some(2),
        DiscardReason::CrcMismatch
        | DiscardReason::HeaderUncorrectable
        | DiscardReason::Security => None,
    }
}

const fn event_number(event: Farm1Event) -> u64 {
    match event {
        Farm1Event::E1 => 1,
        Farm1Event::E2 => 2,
        Farm1Event::E3 => 3,
        Farm1Event::E4 => 4,
        Farm1Event::E5 => 5,
        Farm1Event::E6 => 6,
        Farm1Event::E7 => 7,
        Farm1Event::E8 => 8,
        Farm1Event::E9 => 9,
        Farm1Event::E10 => 10,
        Farm1Event::E11 => 11,
    }
}

const fn state_number(state: Farm1State) -> u64 {
    match state {
        Farm1State::Open => 1,
        Farm1State::Wait => 2,
        Farm1State::Lockout => 3,
    }
}
