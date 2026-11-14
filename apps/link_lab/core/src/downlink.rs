//! Downlink state and frame processing.

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
use crate::config::LinkConfig;

/// Octets of a transfer frame (§4.1, APP-030).
pub const TM_FRAME_LEN: usize = 223;

/// Octets of the data field: frame less primary header (6), OCF (4), and FECF (2) (CMN-040).
pub const TM_DATA_FIELD_LEN: usize = 211;

/// Capacity in octets of a generator's packet queue behind the data field.
pub const TM_QUEUE_LEN: usize = 512;

/// Virtual channels of data in serving order: real-time (0) then payload (1).
pub const DATA_VCS: [u8; 2] = [tm::VCID_REALTIME, tm::VCID_PAYLOAD];

#[expect(
    clippy::as_conversions,
    reason = "const context: usize from u16 is not const in older editions"
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
pub type TmGenerator = PacketGenerator<Tm, TM_DATA_FIELD_LEN, TM_QUEUE_LEN>;

/// Type alias for the TM channel multiplexer.
pub type TmMux = ChannelMux<Tm, 3, TM_FRAME_LEN>;

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
        vc(VcId::<3>::from_const::<{ tm::VCID_REALTIME }>())
            .with_priority(mux_priority(tm::PRIORITY_REALTIME)),
        vc(VcId::<3>::from_const::<{ tm::VCID_PAYLOAD }>())
            .with_priority(mux_priority(tm::PRIORITY_PAYLOAD)),
        vc(VcId::<3>::from_const::<{ tm::VCID_IDLE }>()),
    ];
    let mux_config = ChannelMuxConfig::<Tm, 3>::new(channel, MuxPolicy::FixedPriority, &vcs)?;
    TmMux::new(mux_config)
}

/// Downlink state machine at frame level.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownlinkState {
    /// Packet generators for VC 0 and VC 1.
    pub generators: [TmGenerator; 2],
    /// Whether the multiplexer holds a field for the corresponding VC.
    pub held: [bool; 2],
    /// Channel multiplexer.
    pub mux: TmMux,
    /// Latest CLCW octets for the OCF slot.
    pub ocf: [u8; 4],
}

impl DownlinkState {
    /// Initializes downlink state from configuration.
    ///
    /// # Errors
    /// Returns `ErrorKind::InvalidConfig` if managed parameters fail engine validation.
    pub fn new(config: &LinkConfig) -> Result<Self> {
        Ok(Self {
            generators: [
                make_generator(
                    VcId::<3>::from_const::<{ tm::VCID_REALTIME }>(),
                    config.vc_realtime_deadline_us,
                )?,
                make_generator(
                    VcId::<3>::from_const::<{ tm::VCID_PAYLOAD }>(),
                    config.vc_payload_deadline_us,
                )?,
            ],
            held: [false; 2],
            mux: make_mux(config)?,
            ocf: tm::OCF_INITIAL.to_be_bytes(),
        })
    }

    /// Submits a packet to the generator of `vc` at `now`.
    ///
    /// # Errors
    /// Returns `ErrorKind::UnknownChannel` if `vc` is unknown, or `ErrorKind::WouldBlock`
    /// if the generator queue is full.
    pub fn submit(&mut self, vc: u8, packet: &[u8], now: Instant) -> Result<()> {
        let generator = self
            .generators
            .iter_mut()
            .zip(DATA_VCS)
            .find_map(|(g, id)| (id == vc).then_some(g))
            .ok_or(Error::new(ErrorKind::UnknownChannel))?;
        none(generator.step(GeneratorInput::SubmitPacket(packet), now, &mut [])?)
    }

    /// Earliest release deadline across generators after `now`.
    #[must_use]
    pub fn next_deadline(&self, now: Instant, slot_period_us: u64) -> Option<Instant> {
        if slot_period_us == 0 {
            return None;
        }
        let next_slot = now
            .as_micros()
            .checked_div(slot_period_us)
            .map(|n| Instant::from_micros(n.saturating_add(1).saturating_mul(slot_period_us)));
        self.generators
            .iter()
            .filter_map(PacketGenerator::next_deadline)
            .filter_map(|due| if due > now { Some(due) } else { next_slot })
            .min()
    }

    /// Advances the slot at `now`: collects released fields, sets OCF, and emits one frame.
    ///
    /// # Errors
    /// Returns engine errors if releasing fields, setting OCF, or generating frame fails.
    pub fn slot(
        &mut self,
        now: Instant,
        field_buf: &mut [u8; TM_DATA_FIELD_LEN],
        frame_buf: &mut [u8; TM_FRAME_LEN],
        sink: &mut dyn DownlinkSink,
    ) -> Result<()> {
        for ((generator, held), vc) in self
            .generators
            .iter_mut()
            .zip(self.held.iter_mut())
            .zip(DATA_VCS)
        {
            if generator.next_deadline().is_some_and(|due| due <= now) {
                none(generator.step(GeneratorInput::Timeout, now, field_buf)?)?;
            }
            if *held {
                continue;
            }
            let actions = generator.step(GeneratorInput::Release, now, field_buf)?;
            match actions.get(0) {
                Some(GeneratorOutput::DataField(data)) => {
                    let octets = data
                        .field
                        .slice(field_buf)
                        .ok_or(Error::new(ErrorKind::InternalError))?;
                    none(self.mux.step(
                        MuxInput::Offer(Offer {
                            vc,
                            field: octets,
                            meta: TmOffer {
                                pointer: Some(data.pointer),
                            },
                        }),
                        &mut [],
                    )?)?;
                    *held = true;
                }
                Some(GeneratorOutput::Discarded { .. }) | None => {}
            }
        }

        none(self.mux.step(
            MuxInput::SetOcf {
                scope: Scope::Mc,
                ocf: self.ocf,
            },
            &mut [],
        )?)?;

        let actions = self.mux.step(MuxInput::Opportunity, frame_buf)?;
        let Some(MuxOutput::Transmit(sent)) = actions.get(0) else {
            return Err(Error::new(ErrorKind::InternalError));
        };

        if sent.class == FrameClass::Data {
            for (held, vc) in self.held.iter_mut().zip(DATA_VCS) {
                if vc == sent.vc {
                    *held = false;
                }
            }
        }

        let octets = frame_buf
            .get_mut(sent.frame.start()..sent.frame.end())
            .ok_or(Error::new(ErrorKind::InternalError))?;
        write_fecf(octets)?;
        sink.unit(octets);
        Ok(())
    }
}
