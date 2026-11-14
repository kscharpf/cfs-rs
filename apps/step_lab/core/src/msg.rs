//! Internal Software Bus message layouts for the `link_lab` scenario (docs/design/link-lab.md §4.4).
//!
//! Free of Aldis dependencies and panic-free.

/// Maximum unit size supported in a `WAKE_RX` message.
pub const MAX_UNIT_LEN: usize = 1024;

/// Link layer operation level for the scenario run.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkLevel {
    /// Packets level (`a1-*`).
    Packets = 0,
    /// Frames level (`a2-*`).
    Frames = 1,
    /// COP-1 level (`a3-*`).
    Cop1 = 2,
}

impl LinkLevel {
    /// Determines the link level from the ASCII case ID prefix.
    #[must_use]
    pub const fn from_case_id(case_id: &str) -> Self {
        let bytes = case_id.as_bytes();
        let Some(&first) = bytes.first() else {
            return Self::Packets;
        };
        let second = match bytes {
            [_, b, ..] => *b,
            _ => return Self::Packets,
        };
        if first == b'a' {
            match second {
                b'2' => Self::Frames,
                b'3' => Self::Cop1,
                _ => Self::Packets,
            }
        } else {
            Self::Packets
        }
    }
}

/// Returns a zero-initialized cFE command header.
#[must_use]
pub const fn default_cmd_header() -> cfe_sys::CFE_MSG_CommandHeader_t {
    cfe_sys::CFE_MSG_CommandHeader_t {
        Msg: cfe_sys::CFE_MSG_Message_t {
            CCSDS: cfe_sys::CCSDS_SpacePacket_t {
                Pri: cfe_sys::CCSDS_PrimaryHeader_t {
                    StreamId: [0; 2],
                    Sequence: [0; 2],
                    Length: [0; 2],
                },
            },
        },
        Sec: cfe_sys::CFE_MSG_CommandSecondaryHeader_t {
            FunctionCode: 0,
            Checksum: 0,
        },
    }
}

/// Returns a zero-initialized cFE telemetry header.
#[must_use]
pub const fn default_tlm_header() -> cfe_sys::CFE_MSG_TelemetryHeader_t {
    cfe_sys::CFE_MSG_TelemetryHeader_t {
        Msg: cfe_sys::CFE_MSG_Message_t {
            CCSDS: cfe_sys::CCSDS_SpacePacket_t {
                Pri: cfe_sys::CCSDS_PrimaryHeader_t {
                    StreamId: [0; 2],
                    Sequence: [0; 2],
                    Length: [0; 2],
                },
            },
        },
        Sec: cfe_sys::CFE_MSG_TelemetrySecondaryHeader_t { Time: [0; 6] },
        Spare: [0; 4],
    }
}

/// Copies command header fields from a raw byte slice without panic paths.
fn copy_cmd_header(header: &mut cfe_sys::CFE_MSG_CommandHeader_t, bytes: &[u8]) {
    if let Some(src) = bytes.get(0..2) {
        header.Msg.CCSDS.Pri.StreamId.copy_from_slice(src);
    }
    if let Some(src) = bytes.get(2..4) {
        header.Msg.CCSDS.Pri.Sequence.copy_from_slice(src);
    }
    if let Some(src) = bytes.get(4..6) {
        header.Msg.CCSDS.Pri.Length.copy_from_slice(src);
    }
    if let Some(&fc) = bytes.get(6) {
        header.Sec.FunctionCode = fc;
    }
    if let Some(&cs) = bytes.get(7) {
        header.Sec.Checksum = cs;
    }
}

/// Copies telemetry header fields from a raw byte slice without panic paths.
fn copy_tlm_header(header: &mut cfe_sys::CFE_MSG_TelemetryHeader_t, bytes: &[u8]) {
    if let Some(src) = bytes.get(0..2) {
        header.Msg.CCSDS.Pri.StreamId.copy_from_slice(src);
    }
    if let Some(src) = bytes.get(2..4) {
        header.Msg.CCSDS.Pri.Sequence.copy_from_slice(src);
    }
    if let Some(src) = bytes.get(4..6) {
        header.Msg.CCSDS.Pri.Length.copy_from_slice(src);
    }
    if let Some(src) = bytes.get(6..12) {
        header.Sec.Time.copy_from_slice(src);
    }
    if let Some(src) = bytes.get(12..16) {
        header.Spare.copy_from_slice(src);
    }
}

/// `START`: `step_lab` -> all apps: informs apps of the scenario link level.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct StartMsg {
    /// Command header.
    pub header: cfe_sys::CFE_MSG_CommandHeader_t,
    /// Configured link level (`0` = Packets, `1` = Frames, `2` = Cop1).
    pub level: u8,
    /// Alignment padding.
    pub pad: [u8; 7],
}

impl StartMsg {
    /// Creates a new `StartMsg` with the given link level.
    #[must_use]
    pub const fn new(level: LinkLevel) -> Self {
        Self {
            header: default_cmd_header(),
            level: level as u8,
            pad: [0; 7],
        }
    }

    /// Returns the link level, or `None` if invalid.
    #[must_use]
    pub const fn link_level(&self) -> Option<LinkLevel> {
        match self.level {
            0 => Some(LinkLevel::Packets),
            1 => Some(LinkLevel::Frames),
            2 => Some(LinkLevel::Cop1),
            _ => None,
        }
    }

    /// Safely decodes a `StartMsg` from raw message bytes.
    #[must_use]
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < core::mem::size_of::<Self>() {
            return None;
        }
        let level_byte = *bytes.get(8)?;
        let level = match level_byte {
            0 => LinkLevel::Packets,
            1 => LinkLevel::Frames,
            2 => LinkLevel::Cop1,
            _ => return None,
        };
        let mut msg = Self::new(level);
        if let Some(src) = bytes.get(..8) {
            copy_cmd_header(&mut msg.header, src);
        }
        Some(msg)
    }

    /// Returns a reference to the generic message header.
    #[must_use]
    pub const fn msg(&self) -> &cfe_sys::CFE_MSG_Message_t {
        &self.header.Msg
    }

    /// Returns a mutable reference to the generic message header.
    pub const fn msg_mut(&mut self) -> &mut cfe_sys::CFE_MSG_Message_t {
        &mut self.header.Msg
    }
}
const _: () = assert!(core::mem::size_of::<StartMsg>() == 16);
const _: () = assert!(core::mem::offset_of!(StartMsg, level) == 8);

/// `WAKE_RX`: `step_lab` -> `link_lab`: deliver one received link unit.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct WakeRxMsg {
    /// Command header.
    pub header: cfe_sys::CFE_MSG_CommandHeader_t,
    /// Scenario time in microseconds.
    pub now: u64,
    /// Length of the unit payload in octets.
    pub unit_len: u16,
    /// Alignment padding.
    pub pad: [u8; 6],
    /// Unit octets.
    pub unit: [u8; MAX_UNIT_LEN],
}

impl WakeRxMsg {
    /// Creates a new `WakeRxMsg` containing `unit_slice`.
    ///
    /// Returns `None` if `unit_slice.len() > MAX_UNIT_LEN`.
    #[must_use]
    pub fn new(now: u64, unit_slice: &[u8]) -> Option<Self> {
        if unit_slice.len() > MAX_UNIT_LEN {
            return None;
        }
        let len_u16 = u16::try_from(unit_slice.len()).ok()?;
        let mut msg = Self {
            header: default_cmd_header(),
            now,
            unit_len: len_u16,
            pad: [0; 6],
            unit: [0; MAX_UNIT_LEN],
        };
        if let Some(dest) = msg.unit.get_mut(..unit_slice.len()) {
            dest.copy_from_slice(unit_slice);
        }
        Some(msg)
    }

    /// Safely decodes a `WakeRxMsg` from raw message bytes.
    #[must_use]
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < core::mem::size_of::<Self>() {
            return None;
        }
        let now_bytes: [u8; 8] = bytes.get(8..16)?.try_into().ok()?;
        let now = u64::from_ne_bytes(now_bytes);
        let len_bytes: [u8; 2] = bytes.get(16..18)?.try_into().ok()?;
        let unit_len = usize::from(u16::from_ne_bytes(len_bytes));
        if unit_len > MAX_UNIT_LEN {
            return None;
        }
        let unit_slice = bytes.get(24..24usize.checked_add(unit_len)?)?;
        let mut msg = Self::new(now, unit_slice)?;
        if let Some(src) = bytes.get(..8) {
            copy_cmd_header(&mut msg.header, src);
        }
        Some(msg)
    }

    /// Returns the unit octets slice.
    #[must_use]
    pub fn unit_bytes(&self) -> &[u8] {
        self.unit.get(..usize::from(self.unit_len)).unwrap_or(&[])
    }

    /// Returns a reference to the generic message header.
    #[must_use]
    pub const fn msg(&self) -> &cfe_sys::CFE_MSG_Message_t {
        &self.header.Msg
    }

    /// Returns a mutable reference to the generic message header.
    pub const fn msg_mut(&mut self) -> &mut cfe_sys::CFE_MSG_Message_t {
        &mut self.header.Msg
    }
}
const _: () = assert!(core::mem::size_of::<WakeRxMsg>() == 1048);
const _: () = assert!(core::mem::offset_of!(WakeRxMsg, now) == 8);
const _: () = assert!(core::mem::offset_of!(WakeRxMsg, unit) == 24);

/// `WAKE_TX`: `step_lab` -> `link_lab`: transmit slot and drain trigger.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct WakeTxMsg {
    /// Command header.
    pub header: cfe_sys::CFE_MSG_CommandHeader_t,
    /// Scenario time in microseconds.
    pub now: u64,
    /// Whether the link takes a transmit unit in this step (`1` = true, `0` = false).
    pub tx_slot: u8,
    /// Alignment padding.
    pub pad: [u8; 7],
}

impl WakeTxMsg {
    /// Creates a new `WakeTxMsg`.
    #[must_use]
    pub const fn new(now: u64, tx_slot: bool) -> Self {
        Self {
            header: default_cmd_header(),
            now,
            tx_slot: tx_slot as u8,
            pad: [0; 7],
        }
    }

    /// Safely decodes a `WakeTxMsg` from raw message bytes.
    #[must_use]
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < core::mem::size_of::<Self>() {
            return None;
        }
        let now_bytes: [u8; 8] = bytes.get(8..16)?.try_into().ok()?;
        let now = u64::from_ne_bytes(now_bytes);
        let tx_slot_byte = *bytes.get(16)?;
        let tx_slot = match tx_slot_byte {
            0 => false,
            1 => true,
            _ => return None,
        };
        let mut msg = Self::new(now, tx_slot);
        if let Some(src) = bytes.get(..8) {
            copy_cmd_header(&mut msg.header, src);
        }
        Some(msg)
    }

    /// Returns `true` if `tx_slot` is asserted.
    #[must_use]
    pub const fn is_tx_slot(&self) -> bool {
        self.tx_slot == 1
    }

    /// Returns a reference to the generic message header.
    #[must_use]
    pub const fn msg(&self) -> &cfe_sys::CFE_MSG_Message_t {
        &self.header.Msg
    }

    /// Returns a mutable reference to the generic message header.
    pub const fn msg_mut(&mut self) -> &mut cfe_sys::CFE_MSG_Message_t {
        &mut self.header.Msg
    }
}
const _: () = assert!(core::mem::size_of::<WakeTxMsg>() == 24);
const _: () = assert!(core::mem::offset_of!(WakeTxMsg, now) == 8);
const _: () = assert!(core::mem::offset_of!(WakeTxMsg, tx_slot) == 16);

/// `DONE`: app -> `step_lab`: signals completion of turn and next deadline.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DoneMsg {
    /// Telemetry header.
    pub header: cfe_sys::CFE_MSG_TelemetryHeader_t,
    /// Earliest next deadline in microseconds (`u64::MAX` if `None`).
    pub next_deadline: u64,
}

impl DoneMsg {
    /// Creates a new `DoneMsg` with an optional next deadline.
    #[must_use]
    pub const fn new(next_deadline: Option<u64>) -> Self {
        let val = match next_deadline {
            Some(d) => d,
            None => u64::MAX,
        };
        Self {
            header: default_tlm_header(),
            next_deadline: val,
        }
    }

    /// Safely decodes a `DoneMsg` from raw message bytes.
    #[must_use]
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < core::mem::size_of::<Self>() {
            return None;
        }
        let deadline_bytes: [u8; 8] = bytes.get(16..24)?.try_into().ok()?;
        let next_deadline_raw = u64::from_ne_bytes(deadline_bytes);
        let next_deadline = if next_deadline_raw == u64::MAX {
            None
        } else {
            Some(next_deadline_raw)
        };
        let mut msg = Self::new(next_deadline);
        if let Some(src) = bytes.get(..16) {
            copy_tlm_header(&mut msg.header, src);
        }
        Some(msg)
    }

    /// Returns the next deadline, or `None` if `u64::MAX`.
    #[must_use]
    pub const fn deadline(&self) -> Option<u64> {
        if self.next_deadline == u64::MAX {
            None
        } else {
            Some(self.next_deadline)
        }
    }

    /// Returns a reference to the generic message header.
    #[must_use]
    pub const fn msg(&self) -> &cfe_sys::CFE_MSG_Message_t {
        &self.header.Msg
    }

    /// Returns a mutable reference to the generic message header.
    pub const fn msg_mut(&mut self) -> &mut cfe_sys::CFE_MSG_Message_t {
        &mut self.header.Msg
    }
}
const _: () = assert!(core::mem::size_of::<DoneMsg>() == 24);
const _: () = assert!(core::mem::offset_of!(DoneMsg, next_deadline) == 16);
