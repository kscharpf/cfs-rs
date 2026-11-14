//! Application and link event codes and constants.

/// Code of the application event `packet_rejected` (§7.4).
pub const APP_PACKET_REJECTED: u16 = 6;

/// Code of the application event `frame_rejected` (§7.4): the argument is the error code.
pub const APP_FRAME_REJECTED: u16 = 102;

/// Code of the application event `frame_discarded` (§7.4): layer, reason.
pub const APP_FRAME_DISCARDED: u16 = 103;

/// Code of the application event `sequence_violation` (§7.4): MAP ID.
pub const APP_SEQUENCE_VIOLATION: u16 = 110;

/// Code of the application event `unknown_map` (§7.4): MAP ID.
pub const APP_UNKNOWN_MAP: u16 = 111;

/// Code of the application event `sdu_too_long` (§7.4): MAP ID.
pub const APP_SDU_TOO_LONG: u16 = 112;

/// Code of the application event `downlink_refused` (§7.4): error code, and optionally VCID.
pub const APP_DOWNLINK_REFUSED: u16 = 113;

/// Code of the application event `farm1_state` (§7.4): the new state, 1 Open, 2 Wait, 3 Lockout.
pub const APP_FARM1_STATE: u16 = 210;

/// Code of the application event `farm1_unlock` (§7.4): no argument.
pub const APP_FARM1_UNLOCK: u16 = 211;

/// Layer 1 of `frame_discarded`: Frame Receiver.
pub const LAYER_RECEIVER: u64 = 1;

/// Layer 2 of `frame_discarded`: FARM-1.
pub const LAYER_FARM1: u64 = 2;
