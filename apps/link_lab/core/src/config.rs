//! Link configuration and routing.

use core::net::Ipv4Addr;
use core::num::NonZeroU16;

use aldis::cop1::farm1::Farm1Window;
use aldis::core::ErrorKind;
use aldis::core::ids::VcId;
use refapp_scenario::Variant;
use refapp_scenario::generated::{
    SPACECRAFT_ID, aos, apid, cop1, link, tc, tm, uslp_down, uslp_up,
};

use crate::downlink::TM_FRAME_LEN;
use crate::uplink::{build_tc_vc_table, validate_uslp_uplink_config};

/// Maximum number of APID-to-VC routing entries in [`LinkConfig`].
pub const MAX_APID_ROUTES: usize = 8;

/// Downlink virtual channel carrying data packets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataVc {
    /// Real-time virtual channel (VC 0).
    Realtime,
    /// Payload virtual channel (VC 1).
    Payload,
}

impl DataVc {
    /// Returns the CCSDS virtual channel identifier (VCID).
    #[must_use]
    pub const fn vcid(self) -> u8 {
        self.tm_vcid().get()
    }

    /// Returns the typed TM virtual channel identifier.
    #[must_use]
    pub const fn tm_vcid(self) -> VcId<3> {
        match self {
            Self::Realtime => VcId::<3>::from_const::<{ tm::VCID_REALTIME }>(),
            Self::Payload => VcId::<3>::from_const::<{ tm::VCID_PAYLOAD }>(),
        }
    }

    /// Returns the typed AOS virtual channel identifier.
    #[must_use]
    pub const fn aos_vcid(self) -> VcId<6> {
        match self {
            Self::Realtime => VcId::<6>::from_const::<{ aos::VCID_REALTIME }>(),
            Self::Payload => VcId::<6>::from_const::<{ aos::VCID_PAYLOAD }>(),
        }
    }

    /// Returns the typed USLP downlink virtual channel identifier.
    #[must_use]
    pub const fn uslp_vcid(self) -> VcId<6> {
        match self {
            Self::Realtime => VcId::<6>::from_const::<{ uslp_down::VCID_REALTIME }>(),
            Self::Payload => VcId::<6>::from_const::<{ uslp_down::VCID_PAYLOAD }>(),
        }
    }

    /// Converts a raw CCSDS virtual channel identifier to [`DataVc`].
    #[must_use]
    pub const fn from_vcid(vcid: u8) -> Option<Self> {
        match vcid {
            tm::VCID_REALTIME => Some(Self::Realtime),
            tm::VCID_PAYLOAD => Some(Self::Payload),
            _ => None,
        }
    }
}

/// An APID-to-Virtual-Channel routing entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApidRoute {
    /// 11-bit CCSDS Application Process Identifier.
    pub apid: u16,
    /// Target downlink virtual channel.
    pub vc: DataVc,
}

/// Configuration parameters specific to [`LinkMode::NativeRealtime`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RealtimeConfig {
    /// Fallback VC for unmapped APIDs.
    pub default_vc: Option<DataVc>,
    /// UDP receive port for incoming uplink frames (non-zero).
    pub rx_port: NonZeroU16,
    /// UDP destination port for outgoing downlink frames (non-zero).
    pub tx_port: NonZeroU16,
    /// Ground IPv4 address for outgoing downlink frames.
    pub ground_ipv4: Ipv4Addr,
}

/// Operating mode of `link_lab`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkMode {
    /// Lockstep simulation driven by `step_lab` (`a0`–`a4`).
    Lockstep,
    /// Native real-time mode over UDP frames.
    NativeRealtime(RealtimeConfig),
}

impl LinkMode {
    /// Returns the raw `u8` representation used in `LINK_LAB_CfgTbl_t`.
    #[must_use]
    pub const fn to_raw(self) -> u8 {
        match self {
            Self::Lockstep => 0,
            Self::NativeRealtime(_) => 1,
        }
    }
}

const fn variant_to_raw(variant: Variant) -> u8 {
    match variant {
        Variant::TmTc => 0,
        Variant::Aos => 1,
        Variant::Uslp => 2,
    }
}

const fn variant_from_raw(raw: u8) -> Result<Variant, ErrorKind> {
    match raw {
        0 => Ok(Variant::TmTc),
        1 => Ok(Variant::Aos),
        2 => Ok(Variant::Uslp),
        _ => Err(ErrorKind::InvalidConfig),
    }
}

/// Managed parameters of the link layer (reference-application §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "LinkConfig mirrors the CCSDS managed parameters of TM, TC, AOS, and USLP tables directly"
)]
pub struct LinkConfig {
    /// Spacecraft ID for TM, TC, AOS, and USLP frames (§3.4, APP-026).
    pub spacecraft_id: u16,
    /// Downlink frame slot period in microseconds (§6.1, APP-071).
    pub frame_slot_us: u64,
    /// TM transfer frame length in octets (§4.1, APP-030).
    pub tm_frame_length: u16,
    /// Whether FECF is present on TM frames (§4.1, APP-030).
    pub tm_fecf: bool,
    /// Release deadline of the real-time VC (VC 0) in microseconds (§4.1, APP-033).
    pub vc_realtime_deadline_us: u64,
    /// Release deadline of the payload VC (VC 1) in microseconds (§4.1, APP-033).
    pub vc_payload_deadline_us: u64,
    /// Maximum TC transfer frame length in octets (§4.2, APP-040).
    pub tc_max_frame_length: u16,
    /// Whether FECF is present on TC frames (§4.2, APP-040).
    pub tc_fecf: bool,
    /// FARM sliding window width W (§4.2, APP-044).
    pub cop1_w: u8,
    /// Status Field of the CLCW (§4.2, APP-136).
    pub cop1_status_field: u8,
    /// Initial V(R) of FARM-1 (§4.2, APP-136).
    pub farm_initial_vr: u8,
    /// APID-to-VC routing table (up to [`MAX_APID_ROUTES`] entries).
    pub apid_routes: [Option<ApidRoute>; MAX_APID_ROUTES],
    /// Link operating mode (`Lockstep` vs `NativeRealtime`).
    pub mode: LinkMode,
    /// Configured link protocol variant (`TmTc`, `Aos`, `Uslp`).
    pub variant: Variant,
    /// AOS transfer frame length in octets (§4.3.1, APP-169).
    pub aos_frame_length: u16,
    /// Whether FHEC is present on AOS frames (§4.3.1, APP-169).
    pub aos_fhec: bool,
    /// Whether `VC_OCF` is present on AOS virtual channels (§4.3.1, APP-173).
    pub aos_vc_ocf: bool,
    /// Whether FECF is present on AOS frames (§4.3.1, APP-169).
    pub aos_fecf: bool,
    /// USLP downlink transfer frame length in octets (§4.3.2, APP-175).
    pub uslp_down_frame_length: u16,
    /// Whether FECF is present on USLP downlink frames (§4.3.2, APP-175).
    pub uslp_down_fecf: bool,
    /// Maximum USLP uplink transfer frame length in octets (§4.3.3, APP-182).
    pub uslp_up_max_frame_length: u16,
    /// Maximum USLP uplink packet length on MAP 0 in octets (§4.3.3, APP-182).
    pub uslp_up_max_packet_length: u16,
    /// Whether FECF is present on USLP uplink frames (§4.3.3, APP-181).
    pub uslp_up_fecf: bool,
}

fn read_u8(bytes: &[u8], offset: usize) -> Result<u8, ErrorKind> {
    bytes.get(offset).copied().ok_or(ErrorKind::InvalidConfig)
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, ErrorKind> {
    let slice = bytes
        .get(offset..offset.saturating_add(2))
        .ok_or(ErrorKind::InvalidConfig)?;
    let arr: [u8; 2] = slice.try_into().map_err(|_| ErrorKind::InvalidConfig)?;
    Ok(u16::from_ne_bytes(arr))
}

fn read_u64(bytes: &[u8], offset: usize) -> Result<u64, ErrorKind> {
    let slice = bytes
        .get(offset..offset.saturating_add(8))
        .ok_or(ErrorKind::InvalidConfig)?;
    let arr: [u8; 8] = slice.try_into().map_err(|_| ErrorKind::InvalidConfig)?;
    Ok(u64::from_ne_bytes(arr))
}

fn read_bool(bytes: &[u8], offset: usize) -> Result<bool, ErrorKind> {
    match read_u8(bytes, offset)? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(ErrorKind::InvalidConfig),
    }
}

impl LinkConfig {
    /// Size in bytes of the cFE `LINK_LAB_CfgTbl_t` binary table image.
    pub const TABLE_SIZE: usize = 40_usize
        .saturating_add(MAX_APID_ROUTES.saturating_mul(4))
        .saturating_add(32);

    /// Builds configuration from `refapp-scenario` generated constants.
    #[must_use]
    pub const fn from_scenario() -> Self {
        Self {
            spacecraft_id: SPACECRAFT_ID,
            frame_slot_us: link::FRAME_SLOT_US,
            tm_frame_length: tm::FRAME_LENGTH,
            tm_fecf: tm::FECF,
            vc_realtime_deadline_us: tm::RELEASE_DEADLINE_REALTIME_US,
            vc_payload_deadline_us: tm::RELEASE_DEADLINE_PAYLOAD_US,
            tc_max_frame_length: tc::MAX_FRAME_LENGTH,
            tc_fecf: tc::FECF,
            cop1_w: cop1::W,
            cop1_status_field: cop1::FARM_STATUS_FIELD,
            farm_initial_vr: cop1::FARM_INITIAL_VR,
            apid_routes: [
                Some(ApidRoute {
                    apid: apid::HOUSEKEEPING,
                    vc: DataVc::Realtime,
                }),
                Some(ApidRoute {
                    apid: apid::EVENTS,
                    vc: DataVc::Realtime,
                }),
                Some(ApidRoute {
                    apid: apid::PAYLOAD,
                    vc: DataVc::Payload,
                }),
                Some(ApidRoute {
                    apid: apid::DUMP,
                    vc: DataVc::Payload,
                }),
                None,
                None,
                None,
                None,
            ],
            mode: LinkMode::Lockstep,
            variant: Variant::TmTc,
            aos_frame_length: aos::FRAME_LENGTH,
            aos_fhec: aos::FHEC,
            aos_vc_ocf: aos::VC_OCF,
            aos_fecf: aos::FECF,
            uslp_down_frame_length: uslp_down::FRAME_LENGTH,
            uslp_down_fecf: uslp_down::FECF,
            uslp_up_max_frame_length: uslp_up::MAX_FRAME_LENGTH,
            uslp_up_max_packet_length: uslp_up::MAX_PACKET_LENGTH,
            uslp_up_fecf: uslp_up::FECF,
        }
    }

    /// Decodes and validates a [`LinkConfig`] from a cFE `LINK_LAB_CfgTbl_t` byte slice.
    ///
    /// Layout (104 bytes, target-native endianness matching C struct `LINK_LAB_CfgTbl_t`):
    /// - `0..8`: `frame_slot_us` (`u64`)
    /// - `8..16`: `vc_realtime_deadline_us` (`u64`)
    /// - `16..24`: `vc_payload_deadline_us` (`u64`)
    /// - `24..26`: `spacecraft_id` (`u16`, 10-bit `<= 0x03FF`)
    /// - `26..28`: `tm_frame_length` (`u16`, must equal [`TM_FRAME_LEN`])
    /// - `28..30`: `tc_max_frame_length` (`u16`, `<= TC_FRAME_AREA_LEN` and valid for TC physical channel)
    /// - `30`: `tm_fecf` (`u8`, must be `1`)
    /// - `31`: `tc_fecf` (`u8`, `0` or `1`)
    /// - `32`: `cop1_w` (`u8`, even in `2..=254` for `Farm1Window::retransmission_allowed`)
    /// - `33`: `cop1_status_field` (`u8`, `<= 0x07`)
    /// - `34`: `farm_initial_vr` (`u8`)
    /// - `35`: `route_count` (`u8`, `<= MAX_APID_ROUTES`)
    /// - `36..40`: `reserved` (`[u8; 4]`, must be zero)
    /// - `40..72`: `routes` (`8 * { apid: u16, vcid: u8, reserved: u8 }`)
    /// - `72..74`: `rx_port` (`u16`)
    /// - `74..76`: `tx_port` (`u16`)
    /// - `76..80`: `ground_ipv4` (`[u8; 4]`)
    /// - `80`: `mode` (`u8`, `0` = `Lockstep`, `1` = `NativeRealtime`)
    /// - `81`: `default_vc` (`u8`, `0` = `Realtime`, `1` = `Payload`, `0xFF` = `None`)
    /// - `82`: `variant` (`u8`, `0` = `TmTc`, `1` = `Aos`, `2` = `Uslp`)
    /// - `83`: `reserved2` (`u8`, must be zero)
    /// - `84..86`: `aos_frame_length` (`u16`, must equal [`TM_FRAME_LEN`])
    /// - `86..88`: `uslp_down_frame_length` (`u16`, must equal [`TM_FRAME_LEN`])
    /// - `88..90`: `uslp_up_max_frame_length` (`u16`, `<= MAX_UPLINK_FRAME_LEN` and valid for USLP physical channel)
    /// - `90..92`: `uslp_up_max_packet_length` (`u16`, `1..=USLP_MAX_PACKET_LEN`)
    /// - `92`: `aos_fhec` (`u8`, must be `1`)
    /// - `93`: `aos_vc_ocf` (`u8`, must be `1`)
    /// - `94`: `aos_fecf` (`u8`, must be `1`)
    /// - `95`: `uslp_down_fecf` (`u8`, must be `1`)
    /// - `96`: `uslp_up_fecf` (`u8`, `0` or `1`)
    /// - `97..104`: `reserved3` (`[u8; 7]`, must be zero)
    ///
    /// # Errors
    ///
    /// Returns [`ErrorKind::InvalidConfig`] if `bytes.len() != Self::TABLE_SIZE` or any
    /// field fails range, boolean, alignment/reserved, APID, VCID, or uniqueness checks.
    #[expect(
        clippy::too_many_lines,
        reason = "flat linear table decode and validation of LINK_LAB_CfgTbl_t"
    )]
    pub fn from_table_bytes(bytes: &[u8]) -> Result<Self, ErrorKind> {
        if bytes.len() != Self::TABLE_SIZE {
            return Err(ErrorKind::InvalidConfig);
        }

        let frame_slot_us = read_u64(bytes, 0)?;
        let vc_realtime_deadline_us = read_u64(bytes, 8)?;
        let vc_payload_deadline_us = read_u64(bytes, 16)?;
        let spacecraft_id = read_u16(bytes, 24)?;
        let tm_frame_length = read_u16(bytes, 26)?;
        let tc_max_frame_length = read_u16(bytes, 28)?;
        let tm_fecf = read_bool(bytes, 30)?;
        let uplink_fecf = read_bool(bytes, 31)?;
        let cop1_w = read_u8(bytes, 32)?;
        let cop1_status_field = read_u8(bytes, 33)?;
        let farm_initial_vr = read_u8(bytes, 34)?;
        let route_count = usize::from(read_u8(bytes, 35)?);

        if spacecraft_id > 0x03FF
            || usize::from(tm_frame_length) != TM_FRAME_LEN
            || !tm_fecf
            || cop1_status_field > 0x07
            || route_count > MAX_APID_ROUTES
        {
            return Err(ErrorKind::InvalidConfig);
        }
        build_tc_vc_table(spacecraft_id, tc_max_frame_length, uplink_fecf).map_err(|e| e.kind())?;
        Farm1Window::retransmission_allowed(u16::from(cop1_w)).map_err(|e| e.kind())?;

        if bytes.get(36..40) != Some(&[0_u8; 4][..]) {
            return Err(ErrorKind::InvalidConfig);
        }

        let mut apid_routes: [Option<ApidRoute>; MAX_APID_ROUTES] = [None; MAX_APID_ROUTES];
        let mut idx = 0usize;
        while idx < route_count {
            let offset = 40_usize.saturating_add(idx.saturating_mul(4));
            let route_apid = read_u16(bytes, offset)?;
            let vcid = read_u8(bytes, offset.saturating_add(2))?;
            let route_reserved = read_u8(bytes, offset.saturating_add(3))?;

            if route_apid > 0x07FF || route_reserved != 0 {
                return Err(ErrorKind::InvalidConfig);
            }
            let vc = DataVc::from_vcid(vcid).ok_or(ErrorKind::InvalidConfig)?;

            // Reject duplicate APIDs in the routing table.
            for existing in apid_routes.iter().take(idx).flatten() {
                if existing.apid == route_apid {
                    return Err(ErrorKind::InvalidConfig);
                }
            }

            if let Some(slot) = apid_routes.get_mut(idx) {
                *slot = Some(ApidRoute {
                    apid: route_apid,
                    vc,
                });
            }
            idx = idx.saturating_add(1);
        }

        let rx_port_raw = read_u16(bytes, 72)?;
        let tx_port_raw = read_u16(bytes, 74)?;
        let ground_ipv4_octets: [u8; 4] = bytes
            .get(76..80)
            .and_then(|s| s.try_into().ok())
            .ok_or(ErrorKind::InvalidConfig)?;
        let ground_ipv4 = Ipv4Addr::from(ground_ipv4_octets);
        let mode_raw = read_u8(bytes, 80)?;
        let default_vc = match read_u8(bytes, 81)? {
            0xFF => None,
            vcid => Some(DataVc::from_vcid(vcid).ok_or(ErrorKind::InvalidConfig)?),
        };
        let variant = variant_from_raw(read_u8(bytes, 82)?)?;
        if read_u8(bytes, 83)? != 0 {
            return Err(ErrorKind::InvalidConfig);
        }

        let aos_frame_length = read_u16(bytes, 84)?;
        let uslp_down_frame_length = read_u16(bytes, 86)?;
        let uslp_up_max_frame_length = read_u16(bytes, 88)?;
        let uslp_up_max_packet_length = read_u16(bytes, 90)?;
        let aos_fhec = read_bool(bytes, 92)?;
        let aos_vc_ocf = read_bool(bytes, 93)?;
        let aos_fecf = read_bool(bytes, 94)?;
        let uslp_down_fecf = read_bool(bytes, 95)?;
        let uslp_up_fecf = read_bool(bytes, 96)?;
        if bytes.get(97..104) != Some(&[0_u8; 7][..]) {
            return Err(ErrorKind::InvalidConfig);
        }

        if usize::from(aos_frame_length) != TM_FRAME_LEN
            || !aos_fhec
            || !aos_vc_ocf
            || !aos_fecf
            || usize::from(uslp_down_frame_length) != TM_FRAME_LEN
            || !uslp_down_fecf
        {
            return Err(ErrorKind::InvalidConfig);
        }
        validate_uslp_uplink_config(
            spacecraft_id,
            uslp_up_max_frame_length,
            uslp_up_max_packet_length,
            uslp_up_fecf,
        )
        .map_err(|e| e.kind())?;

        let mode = match mode_raw {
            0 => LinkMode::Lockstep,
            1 => {
                let rx_port = NonZeroU16::new(rx_port_raw).ok_or(ErrorKind::InvalidConfig)?;
                let tx_port = NonZeroU16::new(tx_port_raw).ok_or(ErrorKind::InvalidConfig)?;
                LinkMode::NativeRealtime(RealtimeConfig {
                    default_vc,
                    rx_port,
                    tx_port,
                    ground_ipv4,
                })
            }
            _ => return Err(ErrorKind::InvalidConfig),
        };

        Ok(Self {
            spacecraft_id,
            frame_slot_us,
            tm_frame_length,
            tm_fecf,
            vc_realtime_deadline_us,
            vc_payload_deadline_us,
            tc_max_frame_length,
            tc_fecf: uplink_fecf,
            cop1_w,
            cop1_status_field,
            farm_initial_vr,
            apid_routes,
            mode,
            variant,
            aos_frame_length,
            aos_fhec,
            aos_vc_ocf,
            aos_fecf,
            uslp_down_frame_length,
            uslp_down_fecf,
            uslp_up_max_frame_length,
            uslp_up_max_packet_length,
            uslp_up_fecf,
        })
    }

    /// Encodes this [`LinkConfig`] into the 104-byte `LINK_LAB_CfgTbl_t` binary format.
    #[must_use]
    pub fn to_table_bytes(&self) -> [u8; Self::TABLE_SIZE] {
        let mut out = [0_u8; Self::TABLE_SIZE];
        let write_slice = |buf: &mut [u8], offset: usize, src: &[u8]| {
            if let Some(dest) = buf.get_mut(offset..offset.saturating_add(src.len())) {
                dest.copy_from_slice(src);
            }
        };

        write_slice(&mut out, 0, &self.frame_slot_us.to_ne_bytes());
        write_slice(&mut out, 8, &self.vc_realtime_deadline_us.to_ne_bytes());
        write_slice(&mut out, 16, &self.vc_payload_deadline_us.to_ne_bytes());
        write_slice(&mut out, 24, &self.spacecraft_id.to_ne_bytes());
        write_slice(&mut out, 26, &self.tm_frame_length.to_ne_bytes());
        write_slice(&mut out, 28, &self.tc_max_frame_length.to_ne_bytes());
        write_slice(&mut out, 30, &[u8::from(self.tm_fecf)]);
        write_slice(&mut out, 31, &[u8::from(self.tc_fecf)]);
        write_slice(&mut out, 32, &[self.cop1_w]);
        write_slice(&mut out, 33, &[self.cop1_status_field]);
        write_slice(&mut out, 34, &[self.farm_initial_vr]);

        let mut count = 0_u8;
        for route in self.apid_routes.iter().flatten() {
            let offset = 40_usize.saturating_add(usize::from(count).saturating_mul(4));
            write_slice(&mut out, offset, &route.apid.to_ne_bytes());
            write_slice(&mut out, offset.saturating_add(2), &[route.vc.vcid(), 0]);
            count = count.saturating_add(1);
        }
        write_slice(&mut out, 35, &[count]);
        match self.mode {
            LinkMode::Lockstep => {
                write_slice(&mut out, 80, &[self.mode.to_raw(), 0xFF]);
            }
            LinkMode::NativeRealtime(rt) => {
                write_slice(&mut out, 72, &rt.rx_port.get().to_ne_bytes());
                write_slice(&mut out, 74, &rt.tx_port.get().to_ne_bytes());
                write_slice(&mut out, 76, &rt.ground_ipv4.octets());
                write_slice(
                    &mut out,
                    80,
                    &[self.mode.to_raw(), rt.default_vc.map_or(0xFF, DataVc::vcid)],
                );
            }
        }
        write_slice(&mut out, 82, &[variant_to_raw(self.variant), 0]);
        write_slice(&mut out, 84, &self.aos_frame_length.to_ne_bytes());
        write_slice(&mut out, 86, &self.uslp_down_frame_length.to_ne_bytes());
        write_slice(&mut out, 88, &self.uslp_up_max_frame_length.to_ne_bytes());
        write_slice(&mut out, 90, &self.uslp_up_max_packet_length.to_ne_bytes());
        write_slice(
            &mut out,
            92,
            &[
                u8::from(self.aos_fhec),
                u8::from(self.aos_vc_ocf),
                u8::from(self.aos_fecf),
                u8::from(self.uslp_down_fecf),
                u8::from(self.uslp_up_fecf),
            ],
        );
        out
    }

    /// Maps an application process identifier (APID) to a downlink virtual channel (§4.1).
    #[must_use]
    pub fn vc_for_apid(&self, apid: u16) -> Option<DataVc> {
        self.apid_routes
            .iter()
            .flatten()
            .find_map(|route| (route.apid == apid).then_some(route.vc))
            .or(match self.mode {
                LinkMode::Lockstep => None,
                LinkMode::NativeRealtime(rt) => rt.default_vc,
            })
    }
}

const _: () = assert!(LinkConfig::TABLE_SIZE == 104);

impl Default for LinkConfig {
    fn default() -> Self {
        Self::from_scenario()
    }
}
