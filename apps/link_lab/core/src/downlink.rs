//! Downlink state and frame processing.

use core::num::NonZeroU64;

use aldis::core::crc::write_fecf;
use aldis::core::engine::Actions;
use aldis::core::ids::{SpacecraftId, VcId};
use aldis::core::time::Duration;
use aldis::core::{Error, ErrorKind, Result};
use aldis::link::common::{IdlePacketKind, IdlePolicy, ValidPvns};
use aldis::link::generator::{
    GeneratorInput, GeneratorOutput, PacketGenerator, PacketGeneratorConfig,
};
use aldis::link::mux::{
    ChannelMux, ChannelMuxConfig, FrameClass, MuxInput, MuxOutput, Offer, Scope, TmChannel,
    TmOffer, TmVc, VcConfig as MuxVcConfig,
};
use aldis::link::policy::MuxPolicy;
use aldis::link::protocol::{Tm, TmParams};
use aldis::tm::{DataFieldContent, TmPhysicalChannelConfig, TmVirtualChannelConfig};
use refapp_scenario::Instant;
use refapp_scenario::generated::tm;

use crate::DownlinkSink;
use crate::config::{DataVc, LinkConfig};

/// Octets of a transfer frame (§4.1, APP-030).
pub const TM_FRAME_LEN: usize = 223;

/// Octets of the data field: frame less primary header (6), OCF (4), and FECF (2) (CMN-040).
pub const TM_DATA_FIELD_LEN: usize = 211;

/// Capacity in octets of a generator's packet queue behind the data field.
pub const TM_QUEUE_LEN: usize = 512;

#[expect(
    clippy::as_conversions,
    reason = "usize::from is not const in stable Rust (RFC 143874)"
)]
const _: () = {
    assert!(TM_FRAME_LEN == tm::FRAME_LENGTH as usize);
    assert!(
        TM_DATA_FIELD_LEN
            .saturating_add(6)
            .saturating_add(4)
            .saturating_add(2)
            == TM_FRAME_LEN
    );
};

/// Type alias for the packet generator used for TM virtual channels.
type TmGenerator = PacketGenerator<Tm, TM_DATA_FIELD_LEN, TM_QUEUE_LEN>;

/// Type alias for the TM channel multiplexer.
type TmMux = ChannelMux<Tm, 3, TM_FRAME_LEN>;

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

fn make_generator(vc: VcId<3>, deadline_us: u64) -> Result<TmGenerator> {
    let config = PacketGeneratorConfig::<Tm>::new(
        vc,
        TmParams,
        TM_DATA_FIELD_LEN,
        ValidPvns::SPACE,
        usize::from(tm::MAX_PACKET_LENGTH),
    )?
    .with_idle(
        IdlePolicy::Oid,
        IdlePacketKind::SpacePacket,
        tm::IDLE_PATTERN,
    )?
    .with_release_deadline(Duration::from_micros(deadline_us));
    TmGenerator::new(config)
}

fn make_mux(config: &LinkConfig) -> Result<TmMux> {
    let scid = SpacecraftId::<10>::new(config.spacecraft_id)
        .ok_or(Error::new(ErrorKind::InvalidConfig))?;
    let physical =
        TmPhysicalChannelConfig::new(usize::from(config.tm_frame_length), config.tm_fecf)?;
    let mut channel = TmChannel::new(physical, scid, VcId::<3>::from_const::<{ tm::VCID_IDLE }>());
    channel.mc_ocf = Some(tm::OCF_INITIAL.to_be_bytes());
    let vc = |id: VcId<3>| {
        MuxVcConfig::<Tm>::new(TmVc::new(TmVirtualChannelConfig::new(
            id,
            DataFieldContent::Packets,
        )))
    };
    let vcs = [
        vc(DataVc::Realtime.tm_vcid()).with_priority(mux_priority(tm::PRIORITY_REALTIME)),
        vc(DataVc::Payload.tm_vcid()).with_priority(mux_priority(tm::PRIORITY_PAYLOAD)),
        vc(VcId::<3>::from_const::<{ tm::VCID_IDLE }>()),
    ];
    let mux_config = ChannelMuxConfig::<Tm, 3>::new(channel, MuxPolicy::FixedPriority, &vcs)?;
    TmMux::new(mux_config)
}

/// Per-virtual-channel downlink generator and multiplexer hold state.
#[derive(Debug, Clone, PartialEq, Eq)]
struct VcChannel {
    vc: DataVc,
    generator: TmGenerator,
    held: bool,
}

impl VcChannel {
    fn new(vc: DataVc, deadline_us: u64) -> Result<Self> {
        Ok(Self {
            vc,
            generator: make_generator(vc.tm_vcid(), deadline_us)?,
            held: false,
        })
    }
}

/// Downlink state machine at frame level.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownlinkState {
    /// Per-VC packet generators and multiplexer hold flags in serving order.
    vcs: [VcChannel; 2],
    /// Channel multiplexer.
    mux: TmMux,
    /// Whether FECF is present on emitted TM frames.
    tm_fecf: bool,
    /// Downlink frame slot period in microseconds (`None` if disabled/zero).
    slot_period_us: Option<NonZeroU64>,
}

impl DownlinkState {
    /// Initializes downlink state from configuration.
    ///
    /// # Errors
    /// Returns `ErrorKind::InvalidConfig` if managed parameters fail engine validation
    /// (including `tm_frame_length != TM_FRAME_LEN` or `!tm_fecf`).
    pub fn new(config: &LinkConfig) -> Result<Self> {
        if usize::from(config.tm_frame_length) != TM_FRAME_LEN || !config.tm_fecf {
            return Err(Error::new(ErrorKind::InvalidConfig));
        }
        Ok(Self {
            vcs: [
                VcChannel::new(DataVc::Realtime, config.vc_realtime_deadline_us)?,
                VcChannel::new(DataVc::Payload, config.vc_payload_deadline_us)?,
            ],
            mux: make_mux(config)?,
            tm_fecf: config.tm_fecf,
            slot_period_us: NonZeroU64::new(config.frame_slot_us),
        })
    }

    /// Submits a packet to the generator of `vc` at `now`.
    ///
    /// # Errors
    /// Returns `ErrorKind::WouldBlock` if the generator queue is full.
    pub fn submit(&mut self, vc: DataVc, packet: &[u8], now: Instant) -> Result<()> {
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

    /// Earliest release deadline across generators after `now`.
    #[must_use]
    pub fn next_deadline(&self, now: Instant) -> Option<Instant> {
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
        for ch in &mut self.vcs {
            if ch.generator.next_deadline().is_some_and(|due| due <= now) {
                none(ch.generator.step(GeneratorInput::Timeout, now, field_buf)?)?;
            }
            if ch.held {
                continue;
            }
            let actions = ch.generator.step(GeneratorInput::Release, now, field_buf)?;
            match actions.get(0) {
                Some(GeneratorOutput::DataField(data)) => {
                    let octets = data
                        .field
                        .slice(field_buf)
                        .ok_or(Error::new(ErrorKind::InternalError))?;
                    none(self.mux.step(
                        MuxInput::Offer(Offer {
                            vc: ch.vc.tm_vcid(),
                            field: octets,
                            meta: TmOffer {
                                pointer: Some(data.pointer),
                            },
                        }),
                        &mut [],
                    )?)?;
                    ch.held = true;
                }
                Some(GeneratorOutput::Discarded { .. }) | None => {}
            }
        }

        none(self.mux.step(
            MuxInput::SetOcf {
                scope: Scope::Mc,
                ocf,
            },
            &mut [],
        )?)?;

        let actions = self.mux.step(MuxInput::Opportunity, frame_buf)?;
        let Some(MuxOutput::Transmit(sent)) = actions.get(0) else {
            return Err(Error::new(ErrorKind::InternalError));
        };

        if sent.class == FrameClass::Data {
            for ch in &mut self.vcs {
                if ch.vc.tm_vcid() == sent.vc {
                    ch.held = false;
                }
            }
        }

        let octets = frame_buf
            .get_mut(sent.frame.start()..sent.frame.end())
            .ok_or(Error::new(ErrorKind::InternalError))?;
        if self.tm_fecf {
            write_fecf(octets)?;
        }
        sink.unit(octets);
        Ok(())
    }
}
