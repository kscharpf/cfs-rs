//! Link configuration and routing.

use refapp_scenario::generated::{SPACECRAFT_ID, apid, cop1, link, tc, tm};

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
        match self {
            Self::Realtime => tm::VCID_REALTIME,
            Self::Payload => tm::VCID_PAYLOAD,
        }
    }
}

/// Managed parameters of the link layer (reference-application §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinkConfig {
    /// Spacecraft ID for TM and TC frames (§3.4, APP-026).
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
}

impl LinkConfig {
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
        }
    }

    /// Maps an application process identifier (APID) to a downlink virtual channel (§4.1).
    #[must_use]
    pub const fn vc_for_apid(&self, apid: u16) -> Option<DataVc> {
        match apid {
            apid::HOUSEKEEPING | apid::EVENTS => Some(DataVc::Realtime),
            apid::PAYLOAD | apid::DUMP => Some(DataVc::Payload),
            _ => None,
        }
    }
}

impl Default for LinkConfig {
    fn default() -> Self {
        Self::from_scenario()
    }
}
