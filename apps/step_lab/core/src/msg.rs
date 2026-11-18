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
    /// Returns the raw `u8` value.
    #[must_use]
    pub const fn to_raw(self) -> u8 {
        match self {
            Self::Packets => 0,
            Self::Frames => 1,
            Self::Cop1 => 2,
        }
    }

    /// Determines the link level from the ASCII case ID prefix, or `None` if unknown.
    #[must_use]
    pub const fn from_case_id(case_id: &str) -> Option<Self> {
        let bytes = case_id.as_bytes();
        match bytes {
            [b'a', b'0' | b'1', b'-', ..] | [b'a', b'0' | b'1'] => Some(Self::Packets),
            [b'a', b'2', b'-', ..] | [b'a', b'2'] => Some(Self::Frames),
            [b'a', b'3', b'-', ..] | [b'a', b'3'] => Some(Self::Cop1),
            _ => None,
        }
    }
}

/// Returns a zero-initialized cFE command header with unsegmented sequence flags.
#[must_use]
pub const fn default_cmd_header() -> cfe_sys::CFE_MSG_CommandHeader_t {
    cfe_sys::CFE_MSG_CommandHeader_t {
        Msg: cfe_sys::CFE_MSG_Message_t {
            CCSDS: cfe_sys::CCSDS_SpacePacket_t {
                Pri: cfe_sys::CCSDS_PrimaryHeader_t {
                    StreamId: [0; 2],
                    Sequence: [0xC0, 0x00],
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

/// Returns a zero-initialized cFE telemetry header with unsegmented sequence flags.
#[must_use]
pub const fn default_tlm_header() -> cfe_sys::CFE_MSG_TelemetryHeader_t {
    cfe_sys::CFE_MSG_TelemetryHeader_t {
        Msg: cfe_sys::CFE_MSG_Message_t {
            CCSDS: cfe_sys::CCSDS_SpacePacket_t {
                Pri: cfe_sys::CCSDS_PrimaryHeader_t {
                    StreamId: [0; 2],
                    Sequence: [0xC0, 0x00],
                    Length: [0; 2],
                },
            },
        },
        Sec: cfe_sys::CFE_MSG_TelemetrySecondaryHeader_t { Time: [0; 6] },
        Spare: [0; 4],
    }
}
const _: () = assert!(core::mem::size_of::<cfe_sys::CFE_MSG_CommandHeader_t>() == 8);
const _: () = assert!(core::mem::size_of::<cfe_sys::CFE_MSG_TelemetryHeader_t>() == 16);

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
            level: level.to_raw(),
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
const _: () = assert!(core::mem::offset_of!(WakeRxMsg, unit_len) == 16);
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
            tx_slot: if tx_slot { 1 } else { 0 },
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

/// `WAKE_CMD` / `WAKE_HK` / `WAKE_PL`: `step_lab` -> scenario app: wake for turn at `now`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct WakeTimeMsg<const MID: u32> {
    /// Command header.
    pub header: cfe_sys::CFE_MSG_CommandHeader_t,
    /// Scenario time in microseconds.
    pub now: u64,
}

/// `WAKE_CMD`: `step_lab` -> `cmd_lab`: execute time-tagged commands and dump segments.
pub type WakeCmdMsg = WakeTimeMsg<{ WakeTurn::Cmd.msg_id() }>;

/// `WAKE_HK`: `step_lab` -> `hk_lab`: run housekeeping periodic step.
pub type WakeHkMsg = WakeTimeMsg<{ WakeTurn::Hk.msg_id() }>;

/// `WAKE_PL`: `step_lab` -> `pl_lab`: run payload periodic step.
pub type WakePlMsg = WakeTimeMsg<{ WakeTurn::Pl.msg_id() }>;

impl<const MID: u32> WakeTimeMsg<MID> {
    /// The Software Bus message ID (`u32`) for this wake message type.
    pub const MID: u32 = MID;

    /// Creates a new `WakeTimeMsg`.
    #[must_use]
    pub const fn new(now: u64) -> Self {
        Self {
            header: default_cmd_header(),
            now,
        }
    }

    /// Safely decodes a `WakeTimeMsg` from raw message bytes.
    #[must_use]
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < core::mem::size_of::<Self>() {
            return None;
        }
        let now_bytes: [u8; 8] = bytes.get(8..16)?.try_into().ok()?;
        let now = u64::from_ne_bytes(now_bytes);
        let mut msg = Self::new(now);
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
    #[must_use]
    pub const fn msg_mut(&mut self) -> &mut cfe_sys::CFE_MSG_Message_t {
        &mut self.header.Msg
    }
}
const _: () = assert!(core::mem::size_of::<WakeCmdMsg>() == 16);
const _: () = assert!(core::mem::offset_of!(WakeCmdMsg, now) == 8);

/// Scenario time-turn target paired with its Software Bus message ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WakeTurn {
    /// `WAKE_CMD`: `step_lab` -> `cmd_lab`: execute time-tagged commands and dump segments.
    Cmd,
    /// `WAKE_HK`: `step_lab` -> `hk_lab`: run housekeeping periodic step.
    Hk,
    /// `WAKE_PL`: `step_lab` -> `pl_lab`: run payload periodic step.
    Pl,
}

impl WakeTurn {
    /// All scenario time turns in exact §4.5 execution order.
    pub const ORDER: [Self; 3] = [Self::Cmd, Self::Hk, Self::Pl];

    /// Returns the Software Bus message ID for this turn.
    #[must_use]
    pub const fn msg_id(self) -> u32 {
        match self {
            Self::Cmd => cfe_sys::NODE_WAKE_CMD_MID,
            Self::Hk => cfe_sys::NODE_WAKE_HK_MID,
            Self::Pl => cfe_sys::NODE_WAKE_PL_MID,
        }
    }

    /// Decodes the scenario timestamp (`now` in microseconds) from a [`WakeTimeMsg`] when `mid`
    /// matches [`self.msg_id()`](Self::msg_id).
    #[must_use]
    pub fn decode(self, mid: u32, bytes: &[u8]) -> Option<u64> {
        if mid != self.msg_id() {
            return None;
        }
        WakeCmdMsg::from_bytes(bytes).map(|m| m.now)
    }
}

/// Payload mode carried in [`ScnStatus`] and [`ScnStatusMsg`].
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScnPayloadMode {
    /// Payload packets disabled (`0`).
    Off = 0,
    /// Payload packets enabled (`1`, default at scenario start).
    #[default]
    On = 1,
}

impl ScnPayloadMode {
    /// Converts this mode to its raw `u8` wire value.
    #[must_use]
    pub const fn to_raw(self) -> u8 {
        match self {
            Self::Off => 0,
            Self::On => 1,
        }
    }

    /// Converts a raw `u8` wire value to a `ScnPayloadMode`, or `None` if invalid.
    #[must_use]
    pub const fn from_raw(val: u8) -> Option<Self> {
        match val {
            0 => Some(Self::Off),
            1 => Some(Self::On),
            _ => None,
        }
    }
}

/// Primitive snapshot of scenario status carried by [`ScnStatusMsg`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ScnStatus {
    /// Commands executed, wrapping modulo 2^16.
    pub commands_executed: u16,
    /// Commands rejected, wrapping modulo 2^16.
    pub commands_rejected: u16,
    /// Time-tag queue occupancy.
    pub queue_occupancy: u8,
    /// Last activated table ID (0 if none).
    pub table_id: u8,
    /// CRC-16 of the last activated table (0 if none).
    pub table_crc: u16,
    /// Dumps completed.
    pub dumps_completed: u16,
    /// Payload mode (`Off` or `On`).
    pub payload_mode: ScnPayloadMode,
}

/// `SCN_STATUS`: `cmd_lab` -> `hk_lab`, `pl_lab`: snapshot of scenario status.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ScnStatusMsg {
    /// Telemetry header.
    pub header: cfe_sys::CFE_MSG_TelemetryHeader_t,
    /// Commands executed, wrapping modulo 2^16.
    pub commands_executed: u16,
    /// Commands rejected, wrapping modulo 2^16.
    pub commands_rejected: u16,
    /// CRC-16 of the last activated table (0 if none).
    pub table_crc: u16,
    /// Dumps completed.
    pub dumps_completed: u16,
    /// Time-tag queue occupancy.
    pub queue_occupancy: u8,
    /// Last activated table ID (0 if none).
    pub table_id: u8,
    /// Payload mode (`0` = Off, `1` = On).
    pub payload_mode: u8,
    /// Alignment padding.
    pub pad: u8,
}

impl ScnStatusMsg {
    /// Creates a new `ScnStatusMsg` from primitive status fields.
    #[must_use]
    pub const fn new(status: ScnStatus) -> Self {
        Self {
            header: default_tlm_header(),
            commands_executed: status.commands_executed,
            commands_rejected: status.commands_rejected,
            table_crc: status.table_crc,
            dumps_completed: status.dumps_completed,
            queue_occupancy: status.queue_occupancy,
            table_id: status.table_id,
            payload_mode: status.payload_mode.to_raw(),
            pad: 0,
        }
    }

    /// Returns the decoded [`ScnStatus`], or `None` if `payload_mode` is invalid.
    #[must_use]
    pub const fn status(&self) -> Option<ScnStatus> {
        let Some(payload_mode) = ScnPayloadMode::from_raw(self.payload_mode) else {
            return None;
        };
        Some(ScnStatus {
            commands_executed: self.commands_executed,
            commands_rejected: self.commands_rejected,
            queue_occupancy: self.queue_occupancy,
            table_id: self.table_id,
            table_crc: self.table_crc,
            dumps_completed: self.dumps_completed,
            payload_mode,
        })
    }

    /// Safely decodes a `ScnStatusMsg` from raw message bytes.
    #[must_use]
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < core::mem::size_of::<Self>() {
            return None;
        }
        let commands_executed = u16::from_ne_bytes(bytes.get(16..18)?.try_into().ok()?);
        let commands_rejected = u16::from_ne_bytes(bytes.get(18..20)?.try_into().ok()?);
        let table_crc = u16::from_ne_bytes(bytes.get(20..22)?.try_into().ok()?);
        let dumps_completed = u16::from_ne_bytes(bytes.get(22..24)?.try_into().ok()?);
        let queue_occupancy = *bytes.get(24)?;
        let table_id = *bytes.get(25)?;
        let payload_mode = ScnPayloadMode::from_raw(*bytes.get(26)?)?;
        let mut msg = Self::new(ScnStatus {
            commands_executed,
            commands_rejected,
            queue_occupancy,
            table_id,
            table_crc,
            dumps_completed,
            payload_mode,
        });
        if let Some(src) = bytes.get(..16) {
            copy_tlm_header(&mut msg.header, src);
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
const _: () = assert!(core::mem::size_of::<ScnStatusMsg>() == 28);
const _: () = assert!(core::mem::offset_of!(ScnStatusMsg, commands_executed) == 16);
const _: () = assert!(core::mem::offset_of!(ScnStatusMsg, commands_rejected) == 18);
const _: () = assert!(core::mem::offset_of!(ScnStatusMsg, table_crc) == 20);
const _: () = assert!(core::mem::offset_of!(ScnStatusMsg, dumps_completed) == 22);
const _: () = assert!(core::mem::offset_of!(ScnStatusMsg, queue_occupancy) == 24);
const _: () = assert!(core::mem::offset_of!(ScnStatusMsg, table_id) == 25);
const _: () = assert!(core::mem::offset_of!(ScnStatusMsg, payload_mode) == 26);
const _: () = assert!(core::mem::offset_of!(ScnStatusMsg, pad) == 27);

/// `CMD_PACKET`: `link_lab` -> `cmd_lab`: deliver one extracted command packet.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CmdPacketMsg {
    /// Command header.
    pub header: cfe_sys::CFE_MSG_CommandHeader_t,
    /// Scenario time in microseconds.
    pub now: u64,
    /// Length of the packet in octets.
    pub packet_len: u16,
    /// Alignment padding.
    pub pad: [u8; 6],
    /// Packet octets.
    pub packet: [u8; MAX_UNIT_LEN],
}

impl CmdPacketMsg {
    /// Creates a new `CmdPacketMsg` containing `packet_slice`.
    ///
    /// Returns `None` if `packet_slice.len() > MAX_UNIT_LEN`.
    #[must_use]
    pub fn new(now: u64, packet_slice: &[u8]) -> Option<Self> {
        if packet_slice.len() > MAX_UNIT_LEN {
            return None;
        }
        let len_u16 = u16::try_from(packet_slice.len()).ok()?;
        let mut msg = Self {
            header: default_cmd_header(),
            now,
            packet_len: len_u16,
            pad: [0; 6],
            packet: [0; MAX_UNIT_LEN],
        };
        if let Some(dest) = msg.packet.get_mut(..packet_slice.len()) {
            dest.copy_from_slice(packet_slice);
        }
        Some(msg)
    }

    /// Safely decodes a `CmdPacketMsg` from raw message bytes.
    #[must_use]
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < core::mem::size_of::<Self>() {
            return None;
        }
        let now_bytes: [u8; 8] = bytes.get(8..16)?.try_into().ok()?;
        let now = u64::from_ne_bytes(now_bytes);
        let len_bytes: [u8; 2] = bytes.get(16..18)?.try_into().ok()?;
        let packet_len = usize::from(u16::from_ne_bytes(len_bytes));
        if packet_len > MAX_UNIT_LEN {
            return None;
        }
        let packet_slice = bytes.get(24..24usize.checked_add(packet_len)?)?;
        let mut msg = Self::new(now, packet_slice)?;
        if let Some(src) = bytes.get(..8) {
            copy_cmd_header(&mut msg.header, src);
        }
        Some(msg)
    }

    /// Returns the packet octets slice.
    #[must_use]
    pub fn packet_bytes(&self) -> &[u8] {
        self.packet
            .get(..usize::from(self.packet_len))
            .unwrap_or(&[])
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
const _: () = assert!(core::mem::size_of::<CmdPacketMsg>() == 1048);
const _: () = assert!(core::mem::offset_of!(CmdPacketMsg, now) == 8);
const _: () = assert!(core::mem::offset_of!(CmdPacketMsg, packet_len) == 16);
const _: () = assert!(core::mem::offset_of!(CmdPacketMsg, packet) == 24);

/// `CMD_PACKET_DONE`: `cmd_lab` -> `link_lab`: return turn after handling a command packet.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CmdPacketDoneMsg {
    /// Command header.
    pub header: cfe_sys::CFE_MSG_CommandHeader_t,
}

impl Default for CmdPacketDoneMsg {
    fn default() -> Self {
        Self::new()
    }
}

impl CmdPacketDoneMsg {
    /// Creates a new `CmdPacketDoneMsg`.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            header: default_cmd_header(),
        }
    }

    /// Safely decodes a `CmdPacketDoneMsg` from raw message bytes.
    #[must_use]
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < core::mem::size_of::<Self>() {
            return None;
        }
        let mut msg = Self::new();
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
const _: () = assert!(core::mem::size_of::<CmdPacketDoneMsg>() == 8);

/// `TLM_PACKET`: `cmd_lab` / `hk_lab` / `pl_lab` -> `link_lab`: one telemetry packet for the downlink.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct TlmPacketMsg {
    /// Telemetry header.
    pub header: cfe_sys::CFE_MSG_TelemetryHeader_t,
    /// Length of the packet in octets.
    pub packet_len: u16,
    /// Alignment padding.
    pub pad: [u8; 6],
    /// Packet octets.
    pub packet: [u8; MAX_UNIT_LEN],
}

impl TlmPacketMsg {
    /// Creates a new `TlmPacketMsg` containing `packet_slice`.
    ///
    /// Returns `None` if `packet_slice.len() > MAX_UNIT_LEN`.
    #[must_use]
    pub fn new(packet_slice: &[u8]) -> Option<Self> {
        if packet_slice.len() > MAX_UNIT_LEN {
            return None;
        }
        let len_u16 = u16::try_from(packet_slice.len()).ok()?;
        let mut msg = Self {
            header: default_tlm_header(),
            packet_len: len_u16,
            pad: [0; 6],
            packet: [0; MAX_UNIT_LEN],
        };
        if let Some(dest) = msg.packet.get_mut(..packet_slice.len()) {
            dest.copy_from_slice(packet_slice);
        }
        Some(msg)
    }

    /// Safely decodes a `TlmPacketMsg` from raw message bytes.
    #[must_use]
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < core::mem::size_of::<Self>() {
            return None;
        }
        let len_bytes: [u8; 2] = bytes.get(16..18)?.try_into().ok()?;
        let packet_len = usize::from(u16::from_ne_bytes(len_bytes));
        if packet_len > MAX_UNIT_LEN {
            return None;
        }
        let packet_slice = bytes.get(24..24usize.checked_add(packet_len)?)?;
        let mut msg = Self::new(packet_slice)?;
        if let Some(src) = bytes.get(..16) {
            copy_tlm_header(&mut msg.header, src);
        }
        Some(msg)
    }

    /// Returns the packet octets slice.
    #[must_use]
    pub fn packet_bytes(&self) -> &[u8] {
        self.packet
            .get(..usize::from(self.packet_len))
            .unwrap_or(&[])
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
const _: () = assert!(core::mem::size_of::<TlmPacketMsg>() == 1048);
const _: () = assert!(core::mem::offset_of!(TlmPacketMsg, packet_len) == 16);
const _: () = assert!(core::mem::offset_of!(TlmPacketMsg, packet) == 24);

/// `UNIT`: `link_lab` -> `step_lab`: one downlink unit.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct UnitMsg {
    /// Telemetry header.
    pub header: cfe_sys::CFE_MSG_TelemetryHeader_t,
    /// Length of the unit in octets.
    pub unit_len: u16,
    /// Alignment padding.
    pub pad: [u8; 6],
    /// Unit octets.
    pub unit: [u8; MAX_UNIT_LEN],
}

impl UnitMsg {
    /// Creates a new `UnitMsg` containing `unit_slice`.
    ///
    /// Returns `None` if `unit_slice.len() > MAX_UNIT_LEN`.
    #[must_use]
    pub fn new(unit_slice: &[u8]) -> Option<Self> {
        if unit_slice.len() > MAX_UNIT_LEN {
            return None;
        }
        let len_u16 = u16::try_from(unit_slice.len()).ok()?;
        let mut msg = Self {
            header: default_tlm_header(),
            unit_len: len_u16,
            pad: [0; 6],
            unit: [0; MAX_UNIT_LEN],
        };
        if let Some(dest) = msg.unit.get_mut(..unit_slice.len()) {
            dest.copy_from_slice(unit_slice);
        }
        Some(msg)
    }

    /// Safely decodes a `UnitMsg` from raw message bytes.
    #[must_use]
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < core::mem::size_of::<Self>() {
            return None;
        }
        let len_bytes: [u8; 2] = bytes.get(16..18)?.try_into().ok()?;
        let unit_len = usize::from(u16::from_ne_bytes(len_bytes));
        if unit_len > MAX_UNIT_LEN {
            return None;
        }
        let unit_slice = bytes.get(24..24usize.checked_add(unit_len)?)?;
        let mut msg = Self::new(unit_slice)?;
        if let Some(src) = bytes.get(..16) {
            copy_tlm_header(&mut msg.header, src);
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
const _: () = assert!(core::mem::size_of::<UnitMsg>() == 1048);
const _: () = assert!(core::mem::offset_of!(UnitMsg, unit_len) == 16);
const _: () = assert!(core::mem::offset_of!(UnitMsg, unit) == 24);

/// Maximum number of arguments for an event in `RecordMsg`.
pub const MAX_EVENT_ARGS: usize = 8;

/// Kind of record carried by `RecordMsg`.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordKind {
    /// Telemetry packet output (`NodeOutput::PacketOut`).
    PacketOut = 2,
    /// Telecommand packet input (`NodeOutput::PacketIn`).
    PacketIn = 3,
    /// Application event (`NodeOutput::Event`).
    Event = 4,
}

impl RecordKind {
    /// Converts this kind to its raw `u8` wire value.
    #[must_use]
    pub const fn to_raw(self) -> u8 {
        match self {
            Self::PacketOut => 2,
            Self::PacketIn => 3,
            Self::Event => 4,
        }
    }

    /// Converts a raw `u8` wire value to a `RecordKind`, or `None` if invalid.
    #[must_use]
    pub const fn from_raw(val: u8) -> Option<Self> {
        match val {
            2 => Some(Self::PacketOut),
            3 => Some(Self::PacketIn),
            4 => Some(Self::Event),
            _ => None,
        }
    }
}

/// `RECORD`: `cmd_lab` / `hk_lab` / `pl_lab` / `link_lab` -> `step_lab`: carries one trace output record.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RecordMsg {
    /// Telemetry header.
    pub header: cfe_sys::CFE_MSG_TelemetryHeader_t,
    /// Kind of record (`2` = `PacketOut`, `3` = `PacketIn`, `4` = `Event`).
    pub kind: u8,
    /// Number of `u64` arguments in `args` (for Event).
    pub arg_count: u8,
    /// Event code or packet APID.
    pub code_or_apid: u16,
    /// Length of packet data in octets (0 for Event).
    pub data_len: u16,
    /// Alignment padding.
    pub pad: [u8; 2],
    /// Event arguments.
    pub args: [u64; MAX_EVENT_ARGS],
    /// Packet data octets.
    pub data: [u8; MAX_UNIT_LEN],
}

impl RecordMsg {
    /// Creates a new `RecordMsg` for an application event.
    #[must_use]
    pub fn new_event(code: u16, args: &[u64]) -> Option<Self> {
        if args.len() > MAX_EVENT_ARGS {
            return None;
        }
        let arg_count = u8::try_from(args.len()).ok()?;
        let mut msg = Self {
            header: default_tlm_header(),
            kind: RecordKind::Event.to_raw(),
            arg_count,
            code_or_apid: code,
            data_len: 0,
            pad: [0; 2],
            args: [0; MAX_EVENT_ARGS],
            data: [0; MAX_UNIT_LEN],
        };
        for (i, &arg) in args.iter().enumerate() {
            if let Some(dest) = msg.args.get_mut(i) {
                *dest = arg;
            }
        }
        Some(msg)
    }

    /// Creates a new `RecordMsg` for a packet output.
    #[must_use]
    pub fn new_packet_out(apid: u16, data: &[u8]) -> Option<Self> {
        if data.len() > MAX_UNIT_LEN {
            return None;
        }
        let data_len = u16::try_from(data.len()).ok()?;
        let mut msg = Self {
            header: default_tlm_header(),
            kind: RecordKind::PacketOut.to_raw(),
            arg_count: 0,
            code_or_apid: apid,
            data_len,
            pad: [0; 2],
            args: [0; MAX_EVENT_ARGS],
            data: [0; MAX_UNIT_LEN],
        };
        if let Some(dest) = msg.data.get_mut(..data.len()) {
            dest.copy_from_slice(data);
        }
        Some(msg)
    }

    /// Creates a new `RecordMsg` for a packet input.
    #[must_use]
    pub fn new_packet_in(apid: u16, data: &[u8]) -> Option<Self> {
        if data.len() > MAX_UNIT_LEN {
            return None;
        }
        let data_len = u16::try_from(data.len()).ok()?;
        let mut msg = Self {
            header: default_tlm_header(),
            kind: RecordKind::PacketIn.to_raw(),
            arg_count: 0,
            code_or_apid: apid,
            data_len,
            pad: [0; 2],
            args: [0; MAX_EVENT_ARGS],
            data: [0; MAX_UNIT_LEN],
        };
        if let Some(dest) = msg.data.get_mut(..data.len()) {
            dest.copy_from_slice(data);
        }
        Some(msg)
    }

    /// Safely decodes a `RecordMsg` from raw message bytes.
    #[must_use]
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < core::mem::size_of::<Self>() {
            return None;
        }
        let kind_byte = *bytes.get(16)?;
        let kind = RecordKind::from_raw(kind_byte)?;
        let arg_count = usize::from(*bytes.get(17)?);
        if arg_count > MAX_EVENT_ARGS {
            return None;
        }
        let apid_bytes: [u8; 2] = bytes.get(18..20)?.try_into().ok()?;
        let code_or_apid = u16::from_ne_bytes(apid_bytes);
        let len_bytes: [u8; 2] = bytes.get(20..22)?.try_into().ok()?;
        let data_len = usize::from(u16::from_ne_bytes(len_bytes));
        if data_len > MAX_UNIT_LEN {
            return None;
        }

        let mut msg = match kind {
            RecordKind::Event => {
                let mut args = [0u64; MAX_EVENT_ARGS];
                for i in 0..arg_count {
                    let start = 24usize.checked_add(i.checked_mul(8)?)?;
                    let end = start.checked_add(8)?;
                    let chunk = bytes.get(start..end)?;
                    let arr: [u8; 8] = chunk.try_into().ok()?;
                    if let Some(slot) = args.get_mut(i) {
                        *slot = u64::from_ne_bytes(arr);
                    }
                }
                Self::new_event(code_or_apid, args.get(..arg_count)?)?
            }
            RecordKind::PacketOut => {
                let data = bytes.get(88..88usize.checked_add(data_len)?)?;
                Self::new_packet_out(code_or_apid, data)?
            }
            RecordKind::PacketIn => {
                let data = bytes.get(88..88usize.checked_add(data_len)?)?;
                Self::new_packet_in(code_or_apid, data)?
            }
        };

        if let Some(src) = bytes.get(..16) {
            copy_tlm_header(&mut msg.header, src);
        }
        Some(msg)
    }

    /// Converts this record to a `proto::NodeOutput`.
    #[must_use]
    pub fn to_node_output(&self) -> Option<crate::proto::NodeOutput<'_>> {
        let kind = RecordKind::from_raw(self.kind)?;
        match kind {
            RecordKind::Event => {
                let args_slice = self.args.get(..usize::from(self.arg_count))?;
                Some(crate::proto::NodeOutput::Event(
                    self.code_or_apid,
                    crate::proto::EventArgs::Slice(args_slice),
                ))
            }
            RecordKind::PacketOut => {
                let data_slice = self.data.get(..usize::from(self.data_len))?;
                Some(crate::proto::NodeOutput::PacketOut(
                    self.code_or_apid,
                    data_slice,
                ))
            }
            RecordKind::PacketIn => {
                let data_slice = self.data.get(..usize::from(self.data_len))?;
                Some(crate::proto::NodeOutput::PacketIn(
                    self.code_or_apid,
                    data_slice,
                ))
            }
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
const _: () = assert!(core::mem::size_of::<RecordMsg>() == 1112);
const _: () = assert!(core::mem::offset_of!(RecordMsg, kind) == 16);
const _: () = assert!(core::mem::offset_of!(RecordMsg, args) == 24);
const _: () = assert!(core::mem::offset_of!(RecordMsg, data) == 88);
