//! Downlink state and frame processing for TM, AOS, and USLP.

use core::num::NonZeroU64;

use aldis::aos::{
    AosEdition, AosPhysicalChannelConfig, AosVirtualChannelConfig,
    DataFieldContent as AosDataFieldContent, fhec,
};
use aldis::core::crc::write_fecf;
use aldis::core::engine::Actions;
use aldis::core::ids::{MapId, SpacecraftId, VcId};
use aldis::core::time::Duration;
use aldis::core::{Error, ErrorKind, Result};
use aldis::link::common::{IdlePacketKind, IdlePolicy, Qos, ValidPvns};
use aldis::link::generator::{
    GeneratorInput, GeneratorOutput, PacketGenerator, PacketGeneratorConfig,
};
use aldis::link::mux::{
    AosChannel, AosVc, ChannelMux, ChannelMuxConfig, FrameClass, MuxInput, MuxOutput, MuxProtocol,
    Offer, Scope, TmChannel, TmOffer, TmVc, UslpChannel, UslpOffer, UslpVc,
    VcConfig as MuxVcConfig,
};
use aldis::link::policy::MuxPolicy;
use aldis::link::protocol::{
    Aos, AosParams, AosPointer, PacketProtocol, Tm, TmParams, Uslp, UslpParams,
};
use aldis::tm::{
    DataFieldContent as TmDataFieldContent, TmPhysicalChannelConfig, TmVirtualChannelConfig,
};
use aldis::uslp::{
    FirstHeaderPointer, OcfUse, PhysicalChannelConfig as UslpPhysicalChannelConfig,
    SourceOrDestination, TransferFrameType, Upid, VcConfig as UslpCodecVcConfig, VcParams,
    VcfCountLen,
};
use refapp_scenario::generated::{aos, tm, uslp_down};
use refapp_scenario::{Instant, Variant};

use crate::DownlinkSink;
use crate::config::{DataVc, LinkConfig};

/// Octets of a transfer frame (§4.1, APP-030, APP-169, APP-175).
pub const TM_FRAME_LEN: usize = 223;

/// Octets of the TM data field: frame less primary header (6), OCF (4), and FECF (2) (CMN-040).
pub const TM_DATA_FIELD_LEN: usize = 211;

/// Octets of the AOS `M_PDU`: frame (223) less header (6), FHEC (2), `VC_OCF` (4), and FECF (2).
pub const AOS_MPDU_LEN: usize = 209;

/// Octets of the USLP TFDF: frame (223) less header (7), VCF Count (1), OCF (4), and FECF (2).
pub const USLP_TFDF_LEN: usize = 209;

/// Minimum valid Space Packet length (6 B primary header + 1 B data), used for the AOS idle VC
/// generator which never queues data packets (`QUEUE = 1`).
const IDLE_GEN_MAX_PACKET_LEN: usize = 7;

/// Capacity in octets of a generator's packet queue behind the data field.
pub const TM_QUEUE_LEN: usize = 512;

#[expect(
    clippy::as_conversions,
    reason = "usize::from is not const in stable Rust (RFC 143874)"
)]
const _: () = {
    assert!(TM_FRAME_LEN == tm::FRAME_LENGTH as usize);
    assert!(TM_FRAME_LEN == aos::FRAME_LENGTH as usize);
    assert!(TM_FRAME_LEN == uslp_down::FRAME_LENGTH as usize);
    assert!(
        TM_DATA_FIELD_LEN
            .saturating_add(6)
            .saturating_add(4)
            .saturating_add(2)
            == TM_FRAME_LEN
    );
    assert!(
        AOS_MPDU_LEN
            .saturating_add(6)
            .saturating_add(2)
            .saturating_add(4)
            .saturating_add(2)
            == TM_FRAME_LEN
    );
    assert!(
        USLP_TFDF_LEN
            .saturating_add(7)
            .saturating_add(1)
            .saturating_add(4)
            .saturating_add(2)
            == TM_FRAME_LEN
    );
    assert!(AOS_MPDU_LEN <= TM_DATA_FIELD_LEN);
    assert!(USLP_TFDF_LEN <= TM_DATA_FIELD_LEN);
    assert!(aos::VCID_REALTIME == tm::VCID_REALTIME);
    assert!(aos::VCID_PAYLOAD == tm::VCID_PAYLOAD);
    assert!(uslp_down::VCID_REALTIME == tm::VCID_REALTIME);
    assert!(uslp_down::VCID_PAYLOAD == tm::VCID_PAYLOAD);
    assert!(uslp_down::VCF_COUNT_LENGTH_SEQUENCE == 0);
    assert!(uslp_down::VCF_COUNT_LENGTH_EXPEDITED == 1);
    assert!(uslp_down::OCF_REQUIRED);
};

/// Type alias for the downlink channel multiplexer.
type DownMux<P> = ChannelMux<P, 3, TM_FRAME_LEN>;

const fn mux_priority(rank: u8) -> u8 {
    u8::MAX.saturating_sub(rank)
}

fn none<A: Copy, const N: usize>(actions: Actions<A, N>) -> Result<()> {
    if actions.is_empty() {
        Ok(())
    } else {
        Err(Error::new(ErrorKind::InternalError))
    }
}

/// Protocol-specific downlink behaviour behind [`FrameDownlink`].
trait DownProtocol: PacketProtocol + MuxProtocol {
    /// Octets of the data field the generator produces (at most [`TM_DATA_FIELD_LEN`]).
    const FIELD_LEN: usize;
    /// Idle virtual channel offering `IdleField`s when no data VC offers (AOS), or `None` for OID.
    const IDLE_VC: Option<Self::Vc>;
    /// Maximum packet length accepted by the generators, in octets.
    const MAX_PACKET_LEN: u16;
    /// Idle fill octet.
    const IDLE_PATTERN: u8;

    /// Converts a [`DataVc`] into the protocol's typed VC identifier.
    fn data_vcid(vc: DataVc) -> Self::Vc;

    /// Validates protocol-specific fields in `config`.
    fn validate_config(config: &LinkConfig) -> Result<()>;

    /// Protocol parameters for the packet generators.
    fn generator_params() -> Result<<Self as PacketProtocol>::Params>;

    /// Builds the channel multiplexer for `config`.
    fn make_mux(config: &LinkConfig) -> Result<DownMux<Self>>;

    /// Builds protocol offer metadata from the generator's field pointer.
    fn offer_meta(
        pointer: <Self as PacketProtocol>::FieldPointer,
    ) -> <Self as MuxProtocol>::OfferMeta;

    /// Updates the OCF slot(s) of `mux` to `ocf`.
    fn set_ocf(mux: &mut DownMux<Self>, ocf: [u8; 4]) -> Result<()>;

    /// Finishes the emitted frame in-place (FHEC where present, then FECF).
    fn finish(frame: &mut [u8]) -> Result<()>;
}

impl DownProtocol for Tm {
    const FIELD_LEN: usize = TM_DATA_FIELD_LEN;
    const IDLE_VC: Option<Self::Vc> = None;
    const MAX_PACKET_LEN: u16 = tm::MAX_PACKET_LENGTH;
    const IDLE_PATTERN: u8 = tm::IDLE_PATTERN;

    fn data_vcid(vc: DataVc) -> Self::Vc {
        vc.tm_vcid()
    }

    fn validate_config(config: &LinkConfig) -> Result<()> {
        if usize::from(config.tm_frame_length) != TM_FRAME_LEN || !config.tm_fecf {
            return Err(Error::new(ErrorKind::InvalidConfig));
        }
        Ok(())
    }

    fn generator_params() -> Result<TmParams> {
        Ok(TmParams)
    }

    fn make_mux(config: &LinkConfig) -> Result<DownMux<Self>> {
        let scid = config.spacecraft_id;
        let physical =
            TmPhysicalChannelConfig::new(usize::from(config.tm_frame_length), config.tm_fecf)?;
        let mut channel =
            TmChannel::new(physical, scid, VcId::<3>::from_const::<{ tm::VCID_IDLE }>());
        channel.mc_ocf = Some(tm::OCF_INITIAL.to_be_bytes());
        let vc = |id: VcId<3>| {
            MuxVcConfig::<Self>::new(TmVc::new(TmVirtualChannelConfig::new(
                id,
                TmDataFieldContent::Packets,
            )))
        };
        let vcs = [
            vc(DataVc::Realtime.tm_vcid()).with_priority(mux_priority(tm::PRIORITY_REALTIME)),
            vc(DataVc::Payload.tm_vcid()).with_priority(mux_priority(tm::PRIORITY_PAYLOAD)),
            vc(VcId::<3>::from_const::<{ tm::VCID_IDLE }>()),
        ];
        let mux_config = ChannelMuxConfig::<Self, 3>::new(channel, MuxPolicy::FixedPriority, &vcs)?;
        ChannelMux::new(mux_config)
    }

    fn offer_meta(pointer: aldis::tm::FirstHeaderPointer) -> TmOffer {
        TmOffer {
            pointer: Some(pointer),
        }
    }

    fn set_ocf(mux: &mut DownMux<Self>, ocf: [u8; 4]) -> Result<()> {
        none(mux.step(
            MuxInput::SetOcf {
                scope: Scope::Mc,
                ocf,
            },
            &mut [],
        )?)
    }

    fn finish(frame: &mut [u8]) -> Result<()> {
        write_fecf(frame)
    }
}

fn make_aos_vc(scid: SpacecraftId<10>, id: VcId<6>, rank: u8) -> Result<MuxVcConfig<Aos>> {
    let config = AosVirtualChannelConfig::new(scid, id, AosDataFieldContent::Mpdu)?;
    let params = AosVc::new(config.with_ocf()).with_initial_ocf(tm::OCF_INITIAL.to_be_bytes())?;
    Ok(MuxVcConfig::<Aos>::new(params).with_priority(mux_priority(rank)))
}

impl DownProtocol for Aos {
    const FIELD_LEN: usize = AOS_MPDU_LEN;
    const IDLE_VC: Option<Self::Vc> = Some(VcId::<6>::from_const::<{ aos::VCID_IDLE }>());
    const MAX_PACKET_LEN: u16 = aos::MAX_PACKET_LENGTH;
    const IDLE_PATTERN: u8 = aos::IDLE_PATTERN;

    fn data_vcid(vc: DataVc) -> Self::Vc {
        vc.aos_vcid()
    }

    fn validate_config(config: &LinkConfig) -> Result<()> {
        if usize::from(config.aos_frame_length) != TM_FRAME_LEN
            || !config.aos_fhec
            || !config.aos_vc_ocf
            || !config.aos_fecf
        {
            return Err(Error::new(ErrorKind::InvalidConfig));
        }
        Ok(())
    }

    fn generator_params() -> Result<AosParams> {
        Ok(AosParams {
            edition: AosEdition::Issue5,
        })
    }

    fn make_mux(config: &LinkConfig) -> Result<DownMux<Self>> {
        let scid = config.spacecraft_id;
        let physical = AosPhysicalChannelConfig::new(
            usize::from(config.aos_frame_length),
            config.aos_fhec,
            0,
            config.aos_fecf,
            AosEdition::Issue5,
        )?;
        let channel = AosChannel::new(physical, scid, false)?;
        let vcs = [
            make_aos_vc(scid, DataVc::Realtime.aos_vcid(), aos::PRIORITY_REALTIME)?,
            make_aos_vc(scid, DataVc::Payload.aos_vcid(), aos::PRIORITY_PAYLOAD)?,
            make_aos_vc(
                scid,
                VcId::<6>::from_const::<{ aos::VCID_IDLE }>(),
                aos::PRIORITY_IDLE,
            )?,
        ];
        let mux_config = ChannelMuxConfig::<Self, 3>::new(channel, MuxPolicy::FixedPriority, &vcs)?;
        ChannelMux::new(mux_config)
    }

    fn offer_meta(_pointer: AosPointer) {}

    fn set_ocf(mux: &mut DownMux<Self>, ocf: [u8; 4]) -> Result<()> {
        let vcs = [
            DataVc::Realtime.aos_vcid(),
            DataVc::Payload.aos_vcid(),
            VcId::<6>::from_const::<{ aos::VCID_IDLE }>(),
        ];
        for id in vcs {
            none(mux.step(
                MuxInput::SetOcf {
                    scope: Scope::Vc(id),
                    ocf,
                },
                &mut [],
            )?)?;
        }
        Ok(())
    }

    fn finish(frame: &mut [u8]) -> Result<()> {
        // LNK-035 order: FHEC over the first 8 octets first, then FECF over the whole frame.
        let (header, _) = frame
            .split_first_chunk_mut::<8>()
            .ok_or(Error::buffer_too_small(8))?;
        fhec::write(header);
        write_fecf(frame)
    }
}

fn make_uslp_vc(scid: SpacecraftId<16>, id: VcId<6>, rank: u8) -> Result<MuxVcConfig<Uslp>> {
    let mut codec = VcParams::new(TransferFrameType::Fixed);
    codec.vcf_count_len_sequence = VcfCountLen::new(uslp_down::VCF_COUNT_LENGTH_SEQUENCE)?;
    codec.vcf_count_len_expedited = VcfCountLen::new(uslp_down::VCF_COUNT_LENGTH_EXPEDITED)?;
    codec.ocf = OcfUse::Required;
    let config = UslpCodecVcConfig::new(scid, id, codec)?;
    let params = UslpVc::new(
        config,
        MapId::<4>::from_const::<{ uslp_down::MAP_ID }>(),
        Qos::Expedited,
        false,
    )?;
    Ok(MuxVcConfig::<Uslp>::new(params).with_priority(mux_priority(rank)))
}

impl DownProtocol for Uslp {
    const FIELD_LEN: usize = USLP_TFDF_LEN;
    const IDLE_VC: Option<Self::Vc> = None;
    const MAX_PACKET_LEN: u16 = uslp_down::MAX_PACKET_LENGTH;
    const IDLE_PATTERN: u8 = uslp_down::IDLE_PATTERN;

    fn data_vcid(vc: DataVc) -> Self::Vc {
        vc.uslp_vcid()
    }

    fn validate_config(config: &LinkConfig) -> Result<()> {
        if usize::from(config.uslp_down_frame_length) != TM_FRAME_LEN || !config.uslp_down_fecf {
            return Err(Error::new(ErrorKind::InvalidConfig));
        }
        Ok(())
    }

    fn generator_params() -> Result<UslpParams> {
        UslpParams::new(
            Upid::Packets,
            MapId::<4>::from_const::<{ uslp_down::MAP_ID }>(),
            Qos::Expedited,
        )
    }

    fn make_mux(config: &LinkConfig) -> Result<DownMux<Self>> {
        let scid = SpacecraftId::<16>::new(config.spacecraft_id.get())
            .ok_or(Error::new(ErrorKind::InvalidConfig))?;
        let physical = UslpPhysicalChannelConfig::new(
            TransferFrameType::Fixed,
            usize::from(config.uslp_down_frame_length),
            0,
            config.uslp_down_fecf,
        )?;
        let channel = UslpChannel::new(
            physical,
            scid,
            SourceOrDestination::Source,
            VcfCountLen::NONE,
            Some(tm::OCF_INITIAL.to_be_bytes()),
        )?;
        let vcs = [
            make_uslp_vc(
                scid,
                DataVc::Realtime.uslp_vcid(),
                uslp_down::PRIORITY_REALTIME,
            )?,
            make_uslp_vc(
                scid,
                DataVc::Payload.uslp_vcid(),
                uslp_down::PRIORITY_PAYLOAD,
            )?,
        ];
        let mux_config = ChannelMuxConfig::<Self, 3>::new(channel, MuxPolicy::FixedPriority, &vcs)?;
        ChannelMux::new(mux_config)
    }

    fn offer_meta(_pointer: FirstHeaderPointer) -> UslpOffer {
        UslpOffer::default()
    }

    fn set_ocf(mux: &mut DownMux<Self>, ocf: [u8; 4]) -> Result<()> {
        none(mux.step(
            MuxInput::SetOcf {
                scope: Scope::Mc,
                ocf,
            },
            &mut [],
        )?)
    }

    fn finish(frame: &mut [u8]) -> Result<()> {
        write_fecf(frame)
    }
}

fn make_generator<P: DownProtocol, const Q: usize>(
    vc: P::Vc,
    deadline_us: Option<u64>,
    policy: IdlePolicy,
    max_packet_len: usize,
) -> Result<PacketGenerator<P, TM_DATA_FIELD_LEN, Q>> {
    let config = PacketGeneratorConfig::<P>::new(
        vc,
        P::generator_params()?,
        P::FIELD_LEN,
        ValidPvns::SPACE,
        max_packet_len,
    )?
    .with_idle(policy, IdlePacketKind::SpacePacket, P::IDLE_PATTERN)?;
    let config = deadline_us.map_or(config, |us| {
        config.with_release_deadline(Duration::from_micros(us))
    });
    PacketGenerator::new(config)
}

/// Per-virtual-channel downlink generator and multiplexer hold state.
#[derive(Debug, Clone, PartialEq, Eq)]
struct VcChannel<P: DownProtocol, const Q: usize = TM_QUEUE_LEN> {
    vc: P::Vc,
    generator: PacketGenerator<P, TM_DATA_FIELD_LEN, Q>,
    held: bool,
}

impl<P: DownProtocol, const Q: usize> VcChannel<P, Q> {
    fn new(
        vc: P::Vc,
        deadline_us: Option<u64>,
        policy: IdlePolicy,
        max_packet_len: usize,
    ) -> Result<Self> {
        Ok(Self {
            vc,
            generator: make_generator(vc, deadline_us, policy, max_packet_len)?,
            held: false,
        })
    }

    fn offer(&mut self, mux: &mut DownMux<P>, now: Instant, field_buf: &mut [u8]) -> Result<()> {
        if self.held {
            return Ok(());
        }
        let actions = self
            .generator
            .step(GeneratorInput::Release, now, field_buf)?;
        match actions.get(0) {
            Some(GeneratorOutput::DataField(data)) => {
                let octets = data
                    .field
                    .slice(field_buf)
                    .ok_or(Error::new(ErrorKind::InternalError))?;
                none(mux.step(
                    MuxInput::Offer(Offer {
                        vc: self.vc,
                        field: octets,
                        meta: P::offer_meta(data.pointer),
                    }),
                    &mut [],
                )?)?;
                self.held = true;
            }
            Some(GeneratorOutput::Discarded { .. }) | None => {}
        }
        Ok(())
    }
}

/// Protocol-generic frame downlink state machine.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FrameDownlink<P: DownProtocol> {
    /// Per-VC packet generators and multiplexer hold flags in serving order (Realtime, Payload).
    vcs: [VcChannel<P, TM_QUEUE_LEN>; 2],
    /// Optional idle VC offering `IdleField`s when no data VC offers (AOS VC 7).
    idle: Option<VcChannel<P, 1>>,
    /// Channel multiplexer.
    mux: DownMux<P>,
    /// Downlink frame slot period in microseconds (`None` if disabled/zero).
    slot_period_us: Option<NonZeroU64>,
}

impl<P: DownProtocol> FrameDownlink<P> {
    #[inline(never)]
    fn new(config: &LinkConfig) -> Result<Self> {
        P::validate_config(config)?;
        let max_pkt = usize::from(P::MAX_PACKET_LEN);
        let idle = match P::IDLE_VC {
            Some(vc) => Some(VcChannel::<P, 1>::new(
                vc,
                None,
                IdlePolicy::IdleField,
                IDLE_GEN_MAX_PACKET_LEN,
            )?),
            None => None,
        };
        Ok(Self {
            vcs: [
                VcChannel::new(
                    P::data_vcid(DataVc::Realtime),
                    Some(config.vc_realtime_deadline_us),
                    IdlePolicy::Oid,
                    max_pkt,
                )?,
                VcChannel::new(
                    P::data_vcid(DataVc::Payload),
                    Some(config.vc_payload_deadline_us),
                    IdlePolicy::Oid,
                    max_pkt,
                )?,
            ],
            idle,
            mux: P::make_mux(config)?,
            slot_period_us: NonZeroU64::new(config.frame_slot_us),
        })
    }

    fn submit(&mut self, vc: DataVc, packet: &[u8], now: Instant) -> Result<()> {
        let [realtime, payload] = &mut self.vcs;
        let channel = match vc {
            DataVc::Realtime => realtime,
            DataVc::Payload => payload,
        };
        none(
            channel
                .generator
                .step(GeneratorInput::SubmitPacket(packet), now, &mut [])?,
        )
    }

    #[must_use]
    fn next_deadline(&self, now: Instant) -> Option<Instant> {
        let period = self.slot_period_us?.get();
        let next_slot = now
            .as_micros()
            .checked_div(period)
            .map(|n| Instant::from_micros(n.saturating_add(1).saturating_mul(period)));
        self.vcs
            .iter()
            .filter_map(|ch| ch.generator.next_deadline())
            .filter_map(|due| if due > now { Some(due) } else { next_slot })
            .min()
    }

    fn slot(
        &mut self,
        now: Instant,
        ocf: [u8; 4],
        field_buf: &mut [u8; TM_DATA_FIELD_LEN],
        frame_buf: &mut [u8; TM_FRAME_LEN],
        sink: &mut dyn DownlinkSink,
    ) -> Result<()> {
        for ch in &mut self.vcs {
            if ch.generator.next_deadline().is_some_and(|due| due <= now) {
                none(ch.generator.step(GeneratorInput::Timeout, now, field_buf)?)?;
            }
            ch.offer(&mut self.mux, now, field_buf)?;
        }
        if let Some(idle) = self.idle.as_mut() {
            idle.offer(&mut self.mux, now, field_buf)?;
        }

        P::set_ocf(&mut self.mux, ocf)?;

        let actions = self.mux.step(MuxInput::Opportunity, frame_buf)?;
        let Some(MuxOutput::Transmit(sent)) = actions.get(0) else {
            return Err(Error::new(ErrorKind::InternalError));
        };

        if sent.class == FrameClass::Data {
            for ch in &mut self.vcs {
                if ch.vc == sent.vc {
                    ch.held = false;
                }
            }
            if let Some(idle) = self.idle.as_mut()
                && idle.vc == sent.vc
            {
                idle.held = false;
            }
        }

        let octets = frame_buf
            .get_mut(sent.frame.start()..sent.frame.end())
            .ok_or(Error::new(ErrorKind::InternalError))?;
        P::finish(octets)?;
        sink.unit(octets);
        Ok(())
    }
}

/// Closed-enum downlink state machine across TM, AOS, and USLP.
#[derive(Debug, Clone, PartialEq, Eq)]
enum DownlinkInner {
    /// TM downlink (`Variant::TmTc`).
    Tm(FrameDownlink<Tm>),
    /// AOS downlink (`Variant::Aos`).
    Aos(FrameDownlink<Aos>),
    /// USLP downlink (`Variant::Uslp`).
    Uslp(FrameDownlink<Uslp>),
}

/// Downlink state machine at frame level across TM, AOS, and USLP.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownlinkState {
    inner: DownlinkInner,
}

impl DownlinkState {
    /// Initializes TM downlink state from configuration.
    ///
    /// # Errors
    /// Returns `ErrorKind::InvalidConfig` if managed parameters fail engine validation.
    pub fn new(config: &LinkConfig) -> Result<Self> {
        Self::new_with_variant(Variant::TmTc, config)
    }

    #[inline(never)]
    fn new_tm(config: &LinkConfig) -> Result<Self> {
        Ok(Self {
            inner: DownlinkInner::Tm(FrameDownlink::new(config)?),
        })
    }

    #[inline(never)]
    fn new_aos(config: &LinkConfig) -> Result<Self> {
        Ok(Self {
            inner: DownlinkInner::Aos(FrameDownlink::new(config)?),
        })
    }

    #[inline(never)]
    fn new_uslp(config: &LinkConfig) -> Result<Self> {
        Ok(Self {
            inner: DownlinkInner::Uslp(FrameDownlink::new(config)?),
        })
    }

    /// Initializes downlink state for `variant` from `config`.
    ///
    /// # Errors
    /// Returns `ErrorKind::InvalidConfig` if managed parameters fail engine validation.
    #[inline(never)]
    pub fn new_with_variant(variant: Variant, config: &LinkConfig) -> Result<Self> {
        match variant {
            Variant::TmTc => Self::new_tm(config),
            Variant::Aos => Self::new_aos(config),
            Variant::Uslp => Self::new_uslp(config),
        }
    }

    /// Submits a packet to the generator of `vc` at `now`.
    ///
    /// # Errors
    /// Returns `ErrorKind::WouldBlock` if the generator queue is full.
    pub fn submit(&mut self, vc: DataVc, packet: &[u8], now: Instant) -> Result<()> {
        match &mut self.inner {
            DownlinkInner::Tm(d) => d.submit(vc, packet, now),
            DownlinkInner::Aos(d) => d.submit(vc, packet, now),
            DownlinkInner::Uslp(d) => d.submit(vc, packet, now),
        }
    }

    /// Earliest release deadline across generators after `now`.
    #[must_use]
    pub fn next_deadline(&self, now: Instant) -> Option<Instant> {
        match &self.inner {
            DownlinkInner::Tm(d) => d.next_deadline(now),
            DownlinkInner::Aos(d) => d.next_deadline(now),
            DownlinkInner::Uslp(d) => d.next_deadline(now),
        }
    }

    /// Advances the slot at `now`: updates OCF, collects released fields, and emits one frame.
    ///
    /// # Errors
    /// Returns engine errors if releasing fields, setting OCF, or generating frame fails.
    pub fn slot(
        &mut self,
        now: Instant,
        ocf: [u8; 4],
        field_buf: &mut [u8; TM_DATA_FIELD_LEN],
        frame_buf: &mut [u8; TM_FRAME_LEN],
        sink: &mut dyn DownlinkSink,
    ) -> Result<()> {
        match &mut self.inner {
            DownlinkInner::Tm(d) => d.slot(now, ocf, field_buf, frame_buf, sink),
            DownlinkInner::Aos(d) => d.slot(now, ocf, field_buf, frame_buf, sink),
            DownlinkInner::Uslp(d) => d.slot(now, ocf, field_buf, frame_buf, sink),
        }
    }
}
