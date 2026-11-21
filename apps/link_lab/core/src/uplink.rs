//! Uplink state machine and frame reception for TC and USLP.

use aldis::cop1::farm1::{
    Farm1, Farm1Config, Farm1Frame, Farm1Input, Farm1Notify, Farm1Output, Farm1Window,
};
use aldis::cop1::farm1_table::{Farm1Event, Farm1State};
use aldis::core::ids::{FrameSeqNum, VcId};
use aldis::core::security::{NoSecurity, SecurityLayout};
use aldis::core::time::Instant as CoreInstant;
use aldis::core::{DecodeMode, Error, ErrorKind, Result, SecuritySlices};
use aldis::link::protocol::{Tc, Uslp};
use aldis::link::reassembler::{
    FduIn, MapReassembler, MapReassemblerConfig, ReassemblerInput, ReassemblerMap,
    ReassemblerOutput, ReassemblyEvent, TcReassemblerParams, UslpFdu,
};
use aldis::link::receiver::{
    CallerDiscardReason, DeliveryService, DiscardReason, FrameReceiver, FrameReceiverConfig,
    MapSet, ReceiveProtocol, ReceiverInput, ReceiverOutput, TcChannel, TcVc, UslpChannel, UslpVc,
    VcEntry,
};
use aldis::spp::SpacePacket;
use aldis::tc::{
    Clcw, ClcwStatus, ControlCommand, FrameType, MapConfig, PhysicalChannelConfig, Reason,
    SegmentHeader, SegmentHeaderUse, TcMapId, TcSpacecraftId, TcVcId, VcConfig, VcTable,
    check_frame,
};
use aldis::uslp::{
    FrameKind, MapConfig as UslpMapConfig, OcfUse,
    PhysicalChannelConfig as UslpPhysicalChannelConfig, Reason as UslpReason, TfdfHeader, TfdzRule,
    TransferFrameType, Upid, UslpMapId, UslpSpacecraftId, UslpVcId, VcConfig as UslpVcConfig,
    VcParams, VcTable as UslpVcTable, VcfCountLen, check_frame as check_uslp_frame,
};
use refapp_scenario::generated::{cop1, tc, uslp_up};
use refapp_scenario::{Instant, Variant};

use crate::UplinkSink;
use crate::config::LinkConfig;
use crate::event::{
    APP_FARM1_STATE, APP_FARM1_UNLOCK, APP_FRAME_DISCARDED, APP_FRAME_REJECTED, APP_GAP_DISCARD,
    APP_PACKET_REJECTED, APP_SDU_TOO_LONG, APP_SEQUENCE_VIOLATION, APP_UNKNOWN_MAP, LAYER_FARM1,
    LAYER_RECEIVER,
};

/// Capacity of a TC MAP's assembly buffer: maximum packet length (§4.2).
pub const TC_ASSEMBLY_LEN: usize = 512;

/// Maximum TC transfer frame length in octets (§4.2).
pub const TC_FRAME_AREA_LEN: usize = 128;

/// Alias for maximum TC frame length in octets.
pub const TC_MAX_FRAME_LEN: usize = TC_FRAME_AREA_LEN;

/// Alias for maximum TC packet length in octets.
pub const TC_MAX_PACKET_LEN: usize = TC_ASSEMBLY_LEN;

/// Output area for TC segment reassembly: held SDU plus frame data.
pub const TC_REASSEMBLY_AREA_LEN: usize = TC_ASSEMBLY_LEN.saturating_add(TC_FRAME_AREA_LEN);

/// Buffer capacity for prepending the 1-octet TC segment header onto an FDU.
pub const TC_FDU_BUF_LEN: usize = TC_FRAME_AREA_LEN;

/// Maximum USLP uplink transfer frame length in octets (§4.3.3, APP-182).
pub const USLP_MAX_FRAME_LEN: usize = 512;

/// Maximum USLP uplink packet length on MAP 0 in octets (§4.3.3, APP-182).
pub const USLP_MAX_PACKET_LEN: usize = 501;

/// Maximum uplink transfer frame length across all link variants (TC: 128, USLP: 512).
pub const MAX_UPLINK_FRAME_LEN: usize = USLP_MAX_FRAME_LEN;

/// Output area for uplink MAP reassembly across all link variants (max assembly + max frame).
pub const MAX_REASSEMBLY_AREA_LEN: usize = TC_ASSEMBLY_LEN.saturating_add(MAX_UPLINK_FRAME_LEN);

#[expect(
    clippy::as_conversions,
    reason = "usize::from is not const in stable Rust (RFC 143874)"
)]
const _: () = {
    assert!(TC_FRAME_AREA_LEN == tc::MAX_FRAME_LENGTH as usize);
    assert!(TC_ASSEMBLY_LEN == tc::MAX_PACKET_LENGTH as usize);
    assert!(USLP_MAX_FRAME_LEN == uslp_up::MAX_FRAME_LENGTH as usize);
    assert!(USLP_MAX_PACKET_LEN == uslp_up::MAX_PACKET_LENGTH as usize);
    assert!(MAX_UPLINK_FRAME_LEN == 512);
    assert!(TC_FDU_BUF_LEN == 128);
    assert!(MAX_REASSEMBLY_AREA_LEN == 1024);
    assert!(!uslp_up::OCF_ALLOWED);
    assert!(uslp_up::VCF_COUNT_LENGTH_SEQUENCE == 1);
    assert!(uslp_up::VCF_COUNT_LENGTH_EXPEDITED == 0);
    assert!(uslp_up::MAP_BLOCKING && !uslp_up::MAP_SEGMENTATION);
};

/// Scratch buffers for uplink frame reception and reassembly.
#[derive(Debug)]
pub struct UplinkBuffers<'a> {
    /// Area for frame receiver and FARM-1 output.
    pub frame_area: &'a mut [u8; MAX_UPLINK_FRAME_LEN],
    /// Buffer for reassembling a TC FDU with its 1-octet segment header.
    pub fdu_buf: &'a mut [u8; TC_FDU_BUF_LEN],
    /// Assembly area for reassembler output.
    pub reassembly_area: &'a mut [u8; MAX_REASSEMBLY_AREA_LEN],
}

/// FARM-1 delivery buffer holding accepted FDUs until reassembly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DeliveryBuffer {
    held: u8,
}

impl DeliveryBuffer {
    /// Depth of the buffer (§4.2, COP-032).
    const DEPTH: u8 = cop1::FARM_BUFFER_DEPTH;

    /// Creates an empty delivery buffer.
    #[must_use]
    const fn empty() -> Self {
        Self { held: 0 }
    }

    /// Whether there is room for another accepted FDU.
    #[must_use]
    const fn has_room(self) -> bool {
        self.held < Self::DEPTH
    }

    /// Takes one place for an accepted FDU.
    const fn take(&mut self) {
        self.held = self.held.saturating_add(1);
    }

    /// Releases one place after handing the FDU to the reassembler.
    const fn release(&mut self) {
        self.held = self.held.saturating_sub(1);
    }

    /// Resets the buffer on Unlock.
    const fn reset(&mut self) {
        self.held = 0;
    }
}

/// Frame-level uplink operating mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UplinkMode {
    /// Frame level (`a2-*`): Type-BD frames only; COP-1 state events suppressed.
    Frames,
    /// COP-1 level (`a3-*`, `a4-*`): Type-AD/Type-BC frames with FARM-1 state and unlock events.
    Cop1,
}

pub(crate) fn build_tc_vc_table(
    spacecraft_id: u16,
    tc_max_frame_length: u16,
    tc_fecf: bool,
) -> Result<(PhysicalChannelConfig, TcVcId, VcTable<1, 2>)> {
    let bad = Error::new(ErrorKind::InvalidConfig);
    let scid = TcSpacecraftId::new(spacecraft_id).ok_or(bad)?;
    let vc = TcVcId::from_const::<{ tc::VCID }>();
    let frame_len = usize::from(tc_max_frame_length);
    if frame_len > TC_FRAME_AREA_LEN {
        return Err(bad);
    }
    let map = |map_id: TcMapId| MapConfig::new(scid, vc, map_id, SecurityLayout::NONE);
    let header = if tc::SEGMENT_HEADER {
        SegmentHeaderUse::Present
    } else {
        SegmentHeaderUse::Absent(SecurityLayout::NONE)
    };
    let physical = PhysicalChannelConfig::new(frame_len, tc_fecf)?;
    let table = VcTable::new(
        physical,
        &[VcConfig::new(scid, vc, frame_len, header)?],
        &[
            map(TcMapId::from_const::<{ tc::MAP_COMMANDS }>()),
            map(TcMapId::from_const::<{ tc::MAP_TABLE_UPLOAD }>()),
        ],
    )?;
    Ok((physical, vc, table))
}

pub(crate) fn build_uslp_vc_table(
    spacecraft_id: u16,
    uslp_up_max_frame_length: u16,
    uslp_up_fecf: bool,
) -> Result<(
    UslpPhysicalChannelConfig,
    UslpVcId,
    UslpMapId,
    UslpVcTable<1, 1>,
)> {
    let bad = Error::new(ErrorKind::InvalidConfig);
    let scid = UslpSpacecraftId::new(spacecraft_id).ok_or(bad)?;
    let vc = UslpVcId::from_const::<{ uslp_up::VCID }>();
    let map = UslpMapId::from_const::<{ uslp_up::MAP_ID }>();
    let frame_len = usize::from(uslp_up_max_frame_length);
    if frame_len > MAX_UPLINK_FRAME_LEN {
        return Err(bad);
    }
    let physical =
        UslpPhysicalChannelConfig::new(TransferFrameType::Variable, frame_len, 0, uslp_up_fecf)?;
    let params = VcParams {
        vcf_count_len_sequence: VcfCountLen::new(uslp_up::VCF_COUNT_LENGTH_SEQUENCE)?,
        vcf_count_len_expedited: VcfCountLen::new(uslp_up::VCF_COUNT_LENGTH_EXPEDITED)?,
        ocf: OcfUse::Absent,
        ..VcParams::new(TransferFrameType::Variable)
    };
    let table = UslpVcTable::new(
        physical,
        &[UslpVcConfig::new(scid, vc, params)?],
        &[UslpMapConfig::new(scid, vc, map, SecurityLayout::NONE)],
    )?;
    Ok((physical, vc, map, table))
}

pub(crate) fn validate_uslp_uplink_config(
    spacecraft_id: u16,
    uslp_up_max_frame_length: u16,
    uslp_up_max_packet_length: u16,
    uslp_up_fecf: bool,
) -> Result<()> {
    let (_, vc, map, _) =
        build_uslp_vc_table(spacecraft_id, uslp_up_max_frame_length, uslp_up_fecf)?;
    let max_pkt = usize::from(uslp_up_max_packet_length);
    if max_pkt == 0 || max_pkt > USLP_MAX_PACKET_LEN {
        return Err(Error::new(ErrorKind::InvalidConfig));
    }
    let _ =
        MapReassemblerConfig::<Uslp, 1>::new((), &[ReassemblerMap::packets(vc, map, max_pkt)?])?;
    Ok(())
}

/// TC uplink receive chain (`Variant::TmTc` and `Variant::Aos`).
#[derive(Debug, Clone, PartialEq, Eq)]
struct TcUplink {
    mode: UplinkMode,
    table: VcTable<1, 2>,
    vc: TcVcId,
    receiver: FrameReceiver<Tc, 1>,
    farm: Farm1,
    reassembler: MapReassembler<Tc, 2, TC_ASSEMBLY_LEN>,
    reassembler_config: MapReassemblerConfig<Tc, 2>,
    delivery: DeliveryBuffer,
}

impl TcUplink {
    fn new(config: &LinkConfig, mode: UplinkMode) -> Result<Self> {
        let bad = Error::new(ErrorKind::InvalidConfig);
        let (physical, vc, table) = build_tc_vc_table(
            config.spacecraft_id,
            config.tc_max_frame_length,
            config.tc_fecf,
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
        let packets = |map_id: TcMapId| ReassemblerMap::packets(vc, map_id, max_packet);
        let reassembler_config = MapReassemblerConfig::new(
            TcReassemblerParams::with_segment_header(),
            &[
                packets(TcMapId::from_const::<{ tc::MAP_COMMANDS }>())?,
                packets(TcMapId::from_const::<{ tc::MAP_TABLE_UPLOAD }>())?,
            ],
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

    const fn mode(&self) -> UplinkMode {
        self.mode
    }

    const fn clcw(&self) -> Clcw {
        self.farm.clcw()
    }

    fn accept(
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
        let outcome = collect_farm_actions(actions.iter());
        if let Some(event) = outcome.discarded {
            sink.event(APP_FRAME_DISCARDED, &[LAYER_FARM1, event_number(event)]);
        }
        if outcome.unlocked && self.mode == UplinkMode::Cop1 {
            sink.event(APP_FARM1_UNLOCK, &[]);
            self.purge(sink);
        }
        if let Some(range) = outcome.fdu {
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
        if let Some(state) = outcome.new_state
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
        fdu_buf: &mut [u8; TC_FDU_BUF_LEN],
        reassembly_area: &mut [u8; MAX_REASSEMBLY_AREA_LEN],
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
                    if let Some(code) = reassembly_event_code(*event, None) {
                        sink.event(code, &[u64::from(map.get())]);
                    }
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

/// USLP uplink receive chain (`Variant::Uslp`, `UplinkMode::Cop1` only).
#[derive(Debug, Clone, PartialEq, Eq)]
struct UslpUplink {
    table: UslpVcTable<1, 1>,
    vc: VcId<6>,
    receiver: FrameReceiver<Uslp, 1>,
    farm: Farm1,
    reassembler: MapReassembler<Uslp, 1, USLP_MAX_PACKET_LEN>,
    reassembler_config: MapReassemblerConfig<Uslp, 1>,
    delivery: DeliveryBuffer,
}

impl UslpUplink {
    fn new(config: &LinkConfig) -> Result<Self> {
        let bad = Error::new(ErrorKind::InvalidConfig);
        let (physical, vc, map, table) = build_uslp_vc_table(
            config.spacecraft_id,
            config.uslp_up_max_frame_length,
            config.uslp_up_fecf,
        )?;
        let max_pkt = usize::from(config.uslp_up_max_packet_length);
        if max_pkt == 0 || max_pkt > USLP_MAX_PACKET_LEN {
            return Err(bad);
        }
        let vc_params = UslpVc::new(MapSet::new(&[map]), true, true);
        let receiver = FrameReceiver::new(FrameReceiverConfig::new(
            UslpChannel::new(physical, DeliveryService::None)?,
            &[VcEntry::new(vc, vc_params)],
        )?);
        let farm = Farm1::new(
            Farm1Config {
                vc_id: TcVcId::from_const::<{ uslp_up::VCID }>(),
                status: ClcwStatus::new(config.cop1_status_field).ok_or(bad)?,
                window: Farm1Window::retransmission_allowed(u16::from(config.cop1_w))?,
            },
            FrameSeqNum::new(config.farm_initial_vr),
        );
        let reassembler_config =
            MapReassemblerConfig::new((), &[ReassemblerMap::packets(vc, map, max_pkt)?])?;
        let reassembler = MapReassembler::new(reassembler_config)?;
        Ok(Self {
            table,
            vc,
            receiver,
            farm,
            reassembler,
            reassembler_config,
            delivery: DeliveryBuffer::empty(),
        })
    }

    const fn clcw(&self) -> Clcw {
        self.farm.clcw()
    }

    fn accept(
        &mut self,
        now: Instant,
        octets: &[u8],
        buffers: &mut UplinkBuffers<'_>,
        sink: &mut dyn UplinkSink,
    ) {
        let core_now = CoreInstant::from_micros(now.as_micros());
        let frame = match check_uslp_frame(octets, &self.table, DecodeMode::Strict) {
            Ok(val) => val.into_view().into_frame(),
            Err(rejection) => {
                let fecf = rejection.reason() == UslpReason::FecfMismatch;
                self.reject_frame(core_now, fecf, rejection.error().kind(), sink);
                return;
            }
        };
        let Some(full) = frame.full() else {
            return self.reject_frame(core_now, false, ErrorKind::InvalidField, sink);
        };
        let Some(tfdf) = full.octets().get(full.layout().data()) else {
            return self.reject_frame(core_now, false, ErrorKind::InternalError, sink);
        };
        // Manual Farm1Frame construction (tracking kscharpf/aldis#718).
        let farm_frame = match full.kind() {
            FrameKind::SequenceControlled => {
                let Ok(seq) = u8::try_from(full.vcf_count().value()) else {
                    return self.reject_frame(core_now, false, ErrorKind::InternalError, sink);
                };
                Farm1Frame::Ad {
                    sequence_number: FrameSeqNum::new(seq),
                    data: tfdf,
                }
            }
            FrameKind::Expedited => Farm1Frame::Bd { data: tfdf },
            FrameKind::ProtocolControl => {
                let Some(cmd) = parse_uslp_control_command(full.tfdf_header(), full.tfdz()) else {
                    return self.reject_frame(core_now, false, ErrorKind::ReservedValue, sink);
                };
                Farm1Frame::Bc { command: Some(cmd) }
            }
        };
        let Some(vc_id) = TcVcId::new(full.header().vc_id().get()) else {
            return self.reject_frame(core_now, false, ErrorKind::InternalError, sink);
        };
        let info = UslpFdu {
            map: full.header().map_id(),
            gap: false,
        };
        let plain = match frame.into_plain(NoSecurity) {
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
        let input = Farm1Input::Frame {
            vc_id,
            frame: farm_frame,
            buffer_available: self.delivery.has_room(),
        };
        let (farm, actions) = match self.farm.step(input, core_now, buffers.frame_area) {
            Ok(stepped) => stepped,
            Err(e) => return self.reject_frame(core_now, false, e.kind(), sink),
        };
        self.farm = farm;
        let outcome = collect_farm_actions(actions.iter());
        if let Some(event) = outcome.discarded {
            sink.event(APP_FRAME_DISCARDED, &[LAYER_FARM1, event_number(event)]);
        }
        if outcome.unlocked {
            sink.event(APP_FARM1_UNLOCK, &[]);
            self.purge(sink);
        }
        if let Some(range) = outcome.fdu {
            self.delivery.take();
            self.reassemble(
                info,
                range.slice(buffers.frame_area),
                buffers.reassembly_area,
                sink,
            );
            self.delivery.release();
        }
        if let Some(state) = outcome.new_state {
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
        info: UslpFdu,
        tfdf: Option<&[u8]>,
        reassembly_area: &mut [u8; MAX_REASSEMBLY_AREA_LEN],
        sink: &mut dyn UplinkSink,
    ) {
        let Some(data) = tfdf else {
            sink.event(
                APP_PACKET_REJECTED,
                &[u64::from(ErrorKind::InternalError.code())],
            );
            return;
        };
        let input = ReassemblerInput::Fdu(FduIn {
            vc: self.vc,
            meta: info,
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
                    if let Some(code) = reassembly_event_code(*event, Some(APP_GAP_DISCARD)) {
                        sink.event(code, &[u64::from(map.get())]);
                    }
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

/// Parses a COP-1 Control Command from a USLP Type-BC frame's TFDF header and TFDZ.
fn parse_uslp_control_command(header: TfdfHeader, tfdz: &[u8]) -> Option<ControlCommand> {
    if header.upid == Upid::Cop1ControlCommands && header.rule == TfdzRule::NoSegmentation {
        ControlCommand::parse(tfdz)
    } else {
        None
    }
}

/// Closed-enum uplink receive chain across TC and USLP.
#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(
    clippy::large_enum_variant,
    reason = "UplinkState is held by value inside Link on the 64 KiB task stack with zero heap allocation"
)]
enum UplinkInner {
    Tc(TcUplink),
    Uslp(UslpUplink),
}

/// Uplink receive chain for TC (`TmTc`, `Aos`) and USLP (`Uslp`) frames.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UplinkState {
    inner: UplinkInner,
}

impl UplinkState {
    /// Initializes TC uplink receive chain from configuration and operating `mode`.
    ///
    /// # Errors
    /// Returns `ErrorKind::InvalidConfig` if managed parameters fail engine validation.
    pub fn new(config: &LinkConfig, mode: UplinkMode) -> Result<Self> {
        Self::new_with_variant(Variant::TmTc, config, mode)
    }

    /// Initializes uplink receive chain for `variant`, `config`, and operating `mode`.
    ///
    /// # Errors
    /// Returns `ErrorKind::InvalidConfig` if `(variant, mode)` is unsupported (USLP requires
    /// [`UplinkMode::Cop1`]) or if managed parameters fail engine validation.
    pub fn new_with_variant(
        variant: Variant,
        config: &LinkConfig,
        mode: UplinkMode,
    ) -> Result<Self> {
        let inner = match variant {
            Variant::TmTc | Variant::Aos => UplinkInner::Tc(TcUplink::new(config, mode)?),
            Variant::Uslp => {
                if mode != UplinkMode::Cop1 {
                    return Err(Error::new(ErrorKind::InvalidConfig));
                }
                UplinkInner::Uslp(UslpUplink::new(config)?)
            }
        };
        Ok(Self { inner })
    }

    /// Reports the configured uplink operating mode.
    #[must_use]
    pub const fn mode(&self) -> UplinkMode {
        match &self.inner {
            UplinkInner::Tc(u) => u.mode(),
            UplinkInner::Uslp(_) => UplinkMode::Cop1,
        }
    }

    /// Reports current CLCW from FARM-1.
    #[must_use]
    pub const fn clcw(&self) -> Clcw {
        match &self.inner {
            UplinkInner::Tc(u) => u.clcw(),
            UplinkInner::Uslp(u) => u.clcw(),
        }
    }

    /// Handles one delivered uplink frame.
    pub fn accept(
        &mut self,
        now: Instant,
        octets: &[u8],
        buffers: &mut UplinkBuffers<'_>,
        sink: &mut dyn UplinkSink,
    ) {
        match &mut self.inner {
            UplinkInner::Tc(u) => u.accept(now, octets, buffers, sink),
            UplinkInner::Uslp(u) => u.accept(now, octets, buffers, sink),
        }
    }
}

struct FarmOutcome {
    fdu: Option<aldis::core::engine::OutRange>,
    discarded: Option<Farm1Event>,
    unlocked: bool,
    new_state: Option<Farm1State>,
}

fn collect_farm_actions<'a>(actions: impl Iterator<Item = &'a Farm1Output>) -> FarmOutcome {
    let mut out = FarmOutcome {
        fdu: None,
        discarded: None,
        unlocked: false,
        new_state: None,
    };
    for action in actions {
        match action {
            Farm1Output::Notify(Farm1Notify::FrameDiscarded { event, .. }) => {
                out.discarded = Some(*event);
            }
            Farm1Output::Deliver { data, .. } => out.fdu = Some(*data),
            Farm1Output::Notify(Farm1Notify::StateChanged { to, .. }) => out.new_state = Some(*to),
            Farm1Output::Notify(Farm1Notify::Unlocked) => out.unlocked = true,
        }
    }
    out
}

const fn reassembly_event_code(event: ReassemblyEvent, gap_discard: Option<u16>) -> Option<u16> {
    match event {
        ReassemblyEvent::SequenceViolation => Some(APP_SEQUENCE_VIOLATION),
        ReassemblyEvent::UnknownMap => Some(APP_UNKNOWN_MAP),
        ReassemblyEvent::SduTooLong => Some(APP_SDU_TOO_LONG),
        ReassemblyEvent::GapDiscard => gap_discard,
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

fn receiver_accepts<'a, P: ReceiveProtocol + 'a>(
    actions: impl Iterator<Item = &'a ReceiverOutput<P>>,
    sink: &mut dyn UplinkSink,
) -> bool {
    let mut accepted = false;
    for action in actions {
        match action {
            ReceiverOutput::TcFrame { .. }
            | ReceiverOutput::DataField { .. }
            | ReceiverOutput::ProtocolControl { .. } => accepted = true,
            ReceiverOutput::FrameDiscarded { reason, .. } => {
                if let Some(reason_code) = receiver_reason(*reason) {
                    sink.event(APP_FRAME_DISCARDED, &[LAYER_RECEIVER, reason_code]);
                }
                return false;
            }
            ReceiverOutput::FrameGap { .. }
            | ReceiverOutput::CountDiscontinuity { .. }
            | ReceiverOutput::LossFlag
            | ReceiverOutput::Ocf { .. }
            | ReceiverOutput::FshSdu { .. }
            | ReceiverOutput::InsertSdu { .. } => {}
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
