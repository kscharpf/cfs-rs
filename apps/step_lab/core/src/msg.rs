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
