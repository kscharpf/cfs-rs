//! Safe Rust core of a replacement for the cFE MSG module.
//!
//! Each function here is the behaviour of one `CFE_MSG_*` function of cFE, without pointers or
//! C types. The `cfe-msg-ffi` crate exports them under the C names. Space Packet behaviour comes
//! from Aldis; what cFS adds to the Blue Book is implemented here.

#![no_std]
#![forbid(unsafe_code)]

use aldis::core::ids::{Apid, PacketVersion, SeqCount};
pub use aldis::spp::{
    MAX_PACKET_LEN as MAX_SIZE, MIN_PACKET_LEN as SIZE_OFFSET, PRIMARY_HEADER_LEN, PacketType,
    SequenceFlags,
};

/// Maximum 3-bit CCSDS Packet Version Number.
pub const MAX_HEADER_VERSION: u16 = PacketVersion::MAX as u16;

/// Maximum 11-bit CCSDS Application Process Identifier.
pub const MAX_APID: u16 = Apid::MAX;

/// Maximum 14-bit CCSDS Packet Sequence Count.
pub const MAX_SEQUENCE_COUNT: u16 = SeqCount::MAX;

/// Length in octets of `CFE_MSG_CommandHeader_t` (primary header + 2-byte secondary header).
pub const COMMAND_HEADER_LEN: usize = PRIMARY_HEADER_LEN + 2;

/// Length in octets of `CFE_MSG_TelemetryHeader_t` (primary header + 6-byte secondary header +
/// 4-byte spare padding).
pub const TELEMETRY_HEADER_LEN: usize = PRIMARY_HEADER_LEN + 6 + 4;

/// Maximum 7-bit command function code (`CFE_MSG_FC_MASK`).
pub const MAX_FCN_CODE: u16 = FCN_CODE_MASK as u16;

/// Expected CCSDS Packet Version Number for cFE messages (`CFE_MISSION_CCSDSVER`).
const CCSDS_VERSION: u16 = 0;

/// Byte index of the high octet of the Stream ID in the CCSDS Primary Header.
const STREAM_ID_HI_IDX: usize = 0;

/// Byte index of the low octet of the Stream ID in the CCSDS Primary Header.
const STREAM_ID_LO_IDX: usize = 1;

/// Byte index of the high octet of Packet Sequence Control in the CCSDS Primary Header.
const SEQUENCE_HI_IDX: usize = 2;

/// Byte index of the low octet of Packet Sequence Control in the CCSDS Primary Header.
const SEQUENCE_LO_IDX: usize = 3;

/// Byte index of the high octet of Packet Data Length in the CCSDS Primary Header.
const LENGTH_HI_IDX: usize = 4;

/// Byte index of the low octet of Packet Data Length in the CCSDS Primary Header.
const LENGTH_LO_IDX: usize = 5;

/// Bit shift of the 3-bit CCSDS Packet Version Number within `hdr[STREAM_ID_HI_IDX]`.
const VERSION_SHIFT: u8 = 5;

/// Bitmask of the 3-bit CCSDS Packet Version Number within `hdr[STREAM_ID_HI_IDX]`.
const VERSION_MASK: u8 = 0b111 << VERSION_SHIFT;

/// Bitmask of the CCSDS Packet Type bit within `hdr[STREAM_ID_HI_IDX]`.
const TYPE_MASK: u8 = 0x10;

/// Bitmask of the CCSDS Secondary Header Flag bit within `hdr[STREAM_ID_HI_IDX]`.
const SEC_HDR_MASK: u8 = 0x08;

/// Bitmask of the upper 3 bits of the 11-bit APID within `hdr[STREAM_ID_HI_IDX]`.
const APID_HI_MASK: u8 = 0x07;

/// Bit shift of the 2-bit CCSDS Sequence Flags within `hdr[SEQUENCE_HI_IDX]`.
const SEG_FLAG_SHIFT: u8 = 6;

/// Bitmask of the 2-bit CCSDS Sequence Flags within `hdr[SEQUENCE_HI_IDX]`.
const SEG_FLAG_MASK: u8 = 0b11 << SEG_FLAG_SHIFT;

/// Bitmask of the upper 6 bits of the 14-bit Sequence Count within `hdr[SEQUENCE_HI_IDX]`.
const SEQ_COUNT_HI_MASK: u8 = 0x3F;

/// Byte index of `FunctionCode` in `CFE_MSG_CommandHeader_t`.
const FCN_CODE_IDX: usize = PRIMARY_HEADER_LEN;

/// Byte index of `Checksum` in `CFE_MSG_CommandHeader_t`.
const CHECKSUM_IDX: usize = PRIMARY_HEADER_LEN + 1;

/// Bitmask of the 7-bit command function code (`CFE_MSG_FC_MASK`).
const FCN_CODE_MASK: u8 = 0x7F;

/// Error returned when an input argument is outside the valid CCSDS/cFE range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BadArgument;

/// Error returned when accessing or mutating a secondary-header field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MsgError {
    /// Input argument is outside the valid CCSDS/cFE range (`CFE_MSG_BAD_ARGUMENT`).
    BadArgument,
    /// Message header is not the expected CCSDS version, packet type, or lacks a secondary header
    /// (`CFE_MSG_WRONG_MSG_TYPE`).
    WrongMsgType,
}

/// System time value (seconds and subseconds): the Rust counterpart of `CFE_TIME_SysTime_t`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SysTime {
    /// Number of seconds since epoch.
    pub seconds: u32,
    /// Number of subseconds since epoch (LSB = 2^(-32) seconds).
    pub subseconds: u32,
}

/// The Packet Sequence Count that follows `count`: the behaviour of
/// `CFE_MSG_GetNextSequenceCount`.
///
/// A valid count advances modulo 16 384 (CCSDS 133.0, Packet Sequence Count). cFE takes any
/// `uint16`, and for a value above 16 383 it returns 0; this function does the same.
#[must_use]
pub const fn next_sequence_count(count: u16) -> u16 {
    match SeqCount::new(count) {
        Some(valid) => valid.next().get(),
        None => 0,
    }
}

/// Returns the total message size in octets (`PacketDataLength + 7`): the behaviour of
/// `CFE_MSG_GetSize`.
#[must_use]
pub fn get_size(hdr: &[u8; PRIMARY_HEADER_LEN]) -> usize {
    let [_, _, _, _, len_hi, len_lo] = *hdr;
    usize::from(u16::from_be_bytes([len_hi, len_lo])).saturating_add(SIZE_OFFSET)
}

/// Sets the total message size in octets (`PacketDataLength = size - 7`): the behaviour of
/// `CFE_MSG_SetSize`.
///
/// # Errors
/// Returns `Err(BadArgument)` if `size < 7` or `size > 65542`.
pub fn set_size(hdr: &mut [u8; PRIMARY_HEADER_LEN], size: usize) -> Result<(), BadArgument> {
    if !(SIZE_OFFSET..=MAX_SIZE).contains(&size) {
        return Err(BadArgument);
    }
    let Ok(pkt_len) = u16::try_from(size.saturating_sub(SIZE_OFFSET)) else {
        return Err(BadArgument);
    };
    let [len_hi, len_lo] = pkt_len.to_be_bytes();
    hdr[LENGTH_HI_IDX] = len_hi;
    hdr[LENGTH_LO_IDX] = len_lo;
    Ok(())
}

/// Returns the CCSDS Packet Type: the behaviour of `CFE_MSG_GetType`.
#[must_use]
pub const fn get_type(hdr: &[u8; PRIMARY_HEADER_LEN]) -> PacketType {
    if (hdr[STREAM_ID_HI_IDX] & TYPE_MASK) != 0 {
        PacketType::Telecommand
    } else {
        PacketType::Telemetry
    }
}

/// Sets the CCSDS Packet Type: the behaviour of `CFE_MSG_SetType`.
pub const fn set_type(hdr: &mut [u8; PRIMARY_HEADER_LEN], packet_type: PacketType) {
    match packet_type {
        PacketType::Telecommand => hdr[STREAM_ID_HI_IDX] |= TYPE_MASK,
        PacketType::Telemetry => hdr[STREAM_ID_HI_IDX] &= !TYPE_MASK,
    }
}

/// Returns whether the Secondary Header Flag is set: the behaviour of
/// `CFE_MSG_GetHasSecondaryHeader`.
#[must_use]
pub const fn get_has_secondary_header(hdr: &[u8; PRIMARY_HEADER_LEN]) -> bool {
    (hdr[STREAM_ID_HI_IDX] & SEC_HDR_MASK) != 0
}

/// Sets the Secondary Header Flag: the behaviour of `CFE_MSG_SetHasSecondaryHeader`.
pub const fn set_has_secondary_header(hdr: &mut [u8; PRIMARY_HEADER_LEN], has_secondary: bool) {
    if has_secondary {
        hdr[STREAM_ID_HI_IDX] |= SEC_HDR_MASK;
    } else {
        hdr[STREAM_ID_HI_IDX] &= !SEC_HDR_MASK;
    }
}

/// Returns the 3-bit CCSDS Packet Version Number: the behaviour of `CFE_MSG_GetHeaderVersion`.
#[must_use]
pub fn get_header_version(hdr: &[u8; PRIMARY_HEADER_LEN]) -> u16 {
    u16::from(hdr[STREAM_ID_HI_IDX] >> VERSION_SHIFT)
}

/// Sets the 3-bit CCSDS Packet Version Number: the behaviour of `CFE_MSG_SetHeaderVersion`.
///
/// # Errors
/// Returns `Err(BadArgument)` if `version > 7`.
pub fn set_header_version(
    hdr: &mut [u8; PRIMARY_HEADER_LEN],
    version: u16,
) -> Result<(), BadArgument> {
    let Some(ver) = u8::try_from(version).ok().and_then(PacketVersion::new) else {
        return Err(BadArgument);
    };
    hdr[STREAM_ID_HI_IDX] = (hdr[STREAM_ID_HI_IDX] & !VERSION_MASK) | (ver.get() << VERSION_SHIFT);
    Ok(())
}

/// Returns the 11-bit CCSDS Application Process Identifier: the behaviour of `CFE_MSG_GetApId`.
#[must_use]
pub const fn get_apid(hdr: &[u8; PRIMARY_HEADER_LEN]) -> u16 {
    u16::from_be_bytes([hdr[STREAM_ID_HI_IDX] & APID_HI_MASK, hdr[STREAM_ID_LO_IDX]])
}

/// Sets the 11-bit CCSDS Application Process Identifier: the behaviour of `CFE_MSG_SetApId`.
///
/// # Errors
/// Returns `Err(BadArgument)` if `apid > 0x07FF`.
pub const fn set_apid(hdr: &mut [u8; PRIMARY_HEADER_LEN], apid: u16) -> Result<(), BadArgument> {
    let Some(valid) = Apid::new(apid) else {
        return Err(BadArgument);
    };
    let [hi, lo] = valid.get().to_be_bytes();
    hdr[STREAM_ID_HI_IDX] = (hdr[STREAM_ID_HI_IDX] & !APID_HI_MASK) | hi;
    hdr[STREAM_ID_LO_IDX] = lo;
    Ok(())
}

/// Returns the 2-bit CCSDS Sequence Flags: the behaviour of `CFE_MSG_GetSegmentationFlag`.
#[must_use]
pub const fn get_segmentation_flag(hdr: &[u8; PRIMARY_HEADER_LEN]) -> SequenceFlags {
    SequenceFlags::from_bits(hdr[SEQUENCE_HI_IDX] >> SEG_FLAG_SHIFT)
}

/// Sets the 2-bit CCSDS Sequence Flags: the behaviour of `CFE_MSG_SetSegmentationFlag`.
pub const fn set_segmentation_flag(hdr: &mut [u8; PRIMARY_HEADER_LEN], flag: SequenceFlags) {
    hdr[SEQUENCE_HI_IDX] =
        (hdr[SEQUENCE_HI_IDX] & !SEG_FLAG_MASK) | (flag.bits() << SEG_FLAG_SHIFT);
}

/// Returns the 14-bit CCSDS Packet Sequence Count: the behaviour of `CFE_MSG_GetSequenceCount`.
#[must_use]
pub const fn get_sequence_count(hdr: &[u8; PRIMARY_HEADER_LEN]) -> u16 {
    u16::from_be_bytes([
        hdr[SEQUENCE_HI_IDX] & SEQ_COUNT_HI_MASK,
        hdr[SEQUENCE_LO_IDX],
    ])
}

/// Sets the 14-bit CCSDS Packet Sequence Count: the behaviour of `CFE_MSG_SetSequenceCount`.
///
/// # Errors
/// Returns `Err(BadArgument)` if `count > 0x3FFF`.
pub const fn set_sequence_count(
    hdr: &mut [u8; PRIMARY_HEADER_LEN],
    count: u16,
) -> Result<(), BadArgument> {
    let Some(valid) = SeqCount::new(count) else {
        return Err(BadArgument);
    };
    let [hi, lo] = valid.get().to_be_bytes();
    hdr[SEQUENCE_HI_IDX] = (hdr[SEQUENCE_HI_IDX] & !SEQ_COUNT_HI_MASK) | hi;
    hdr[SEQUENCE_LO_IDX] = lo;
    Ok(())
}

/// Returns `true` if `pri` is a CCSDS version 0 telecommand packet with a secondary header.
#[must_use]
pub fn is_command_with_secondary_header(pri: &[u8; PRIMARY_HEADER_LEN]) -> bool {
    get_header_version(pri) == CCSDS_VERSION
        && get_type(pri) == PacketType::Telecommand
        && get_has_secondary_header(pri)
}

/// Returns the 7-bit command function code: the behaviour of `CFE_MSG_GetFcnCode`.
///
/// # Errors
/// Returns `Err(MsgError::WrongMsgType)` if the packet is not CCSDS version 0, is not a
/// telecommand packet, or does not have a secondary header.
pub fn get_fcn_code(hdr: &[u8; COMMAND_HEADER_LEN]) -> Result<u16, MsgError> {
    let [b0, b1, b2, b3, b4, b5, fcn, _checksum] = *hdr;
    let pri = [b0, b1, b2, b3, b4, b5];
    if !is_command_with_secondary_header(&pri) {
        return Err(MsgError::WrongMsgType);
    }
    Ok(u16::from(fcn & FCN_CODE_MASK))
}

/// Sets the 7-bit command function code: the behaviour of `CFE_MSG_SetFcnCode`.
///
/// # Errors
/// Returns `Err(MsgError::BadArgument)` if `fcn_code > 0x7F`, or `Err(MsgError::WrongMsgType)` if
/// the packet is not CCSDS version 0, is not a telecommand packet, or does not have a secondary
/// header.
pub fn set_fcn_code(hdr: &mut [u8; COMMAND_HEADER_LEN], fcn_code: u16) -> Result<(), MsgError> {
    let Ok(fcn) = u8::try_from(fcn_code) else {
        return Err(MsgError::BadArgument);
    };
    if fcn > FCN_CODE_MASK {
        return Err(MsgError::BadArgument);
    }
    let [b0, b1, b2, b3, b4, b5, _, _] = *hdr;
    let pri = [b0, b1, b2, b3, b4, b5];
    if !is_command_with_secondary_header(&pri) {
        return Err(MsgError::WrongMsgType);
    }
    hdr[FCN_CODE_IDX] = fcn;
    Ok(())
}

/// Extracts the primary header bytes from a packet slice if it is at least `PRIMARY_HEADER_LEN`
/// octets long.
fn primary_header_from_slice(pkt: &[u8]) -> Option<[u8; PRIMARY_HEADER_LEN]> {
    pkt.get(..PRIMARY_HEADER_LEN)?.try_into().ok()
}

/// Computes the cFE command checksum over `pkt` (XOR of all octets starting from `0xFF`):
/// the behaviour of `CFE_MSG_ComputeCheckSum`.
#[must_use]
pub fn compute_checksum(pkt: &[u8]) -> u8 {
    pkt.iter().fold(0xFF, |acc, &b| acc ^ b)
}

/// Computes and stores the command secondary-header checksum: the behaviour of
/// `CFE_MSG_GenerateChecksum`.
///
/// # Errors
/// Returns `Err(MsgError::WrongMsgType)` if the packet is not CCSDS version 0, is not a
/// telecommand packet, or does not have a secondary header.
/// Returns `Err(MsgError::BadArgument)` if `pkt` is shorter than `COMMAND_HEADER_LEN` or shorter
/// than the packet size encoded in the primary header.
pub fn generate_checksum(pkt: &mut [u8]) -> Result<(), MsgError> {
    let Some(pri) = primary_header_from_slice(pkt) else {
        return Err(MsgError::BadArgument);
    };
    if !is_command_with_secondary_header(&pri) {
        return Err(MsgError::WrongMsgType);
    }
    let size = get_size(&pri);
    if pkt.len() < COMMAND_HEADER_LEN || pkt.len() < size {
        return Err(MsgError::BadArgument);
    }
    if let Some(checksum_slot) = pkt.get_mut(CHECKSUM_IDX) {
        *checksum_slot = 0;
    }
    let chksum = pkt.get(..size).map_or(0xFF, compute_checksum);
    if let Some(checksum_slot) = pkt.get_mut(CHECKSUM_IDX) {
        *checksum_slot = chksum;
    }
    Ok(())
}

/// Validates the command secondary-header checksum: the behaviour of
/// `CFE_MSG_ValidateChecksum`.
///
/// # Errors
/// Returns `Err(MsgError::WrongMsgType)` if the packet is not CCSDS version 0, is not a
/// telecommand packet, or does not have a secondary header.
/// Returns `Err(MsgError::BadArgument)` if `pkt` is shorter than the packet size encoded in the
/// primary header.
pub fn validate_checksum(pkt: &[u8]) -> Result<bool, MsgError> {
    let Some(pri) = primary_header_from_slice(pkt) else {
        return Err(MsgError::BadArgument);
    };
    if !is_command_with_secondary_header(&pri) {
        return Err(MsgError::WrongMsgType);
    }
    let size = get_size(&pri);
    let Some(slice) = pkt.get(..size) else {
        return Err(MsgError::BadArgument);
    };
    Ok(compute_checksum(slice) == 0)
}

/// Returns `true` if `pri` is a CCSDS version 0 telemetry packet with a secondary header.
#[must_use]
pub fn is_telemetry_with_secondary_header(pri: &[u8; PRIMARY_HEADER_LEN]) -> bool {
    get_header_version(pri) == CCSDS_VERSION
        && get_type(pri) == PacketType::Telemetry
        && get_has_secondary_header(pri)
}

/// Decodes the telemetry secondary-header timestamp (big-endian 32-bit seconds, 16-bit
/// subseconds in the upper 16 bits of `subseconds`): the behaviour of `CFE_MSG_GetMsgTime`.
///
/// # Errors
/// Returns `Err(MsgError::WrongMsgType)` if the packet is not CCSDS version 0, is not a
/// telemetry packet, or does not have a secondary header.
pub fn get_msg_time(hdr: &[u8; TELEMETRY_HEADER_LEN]) -> Result<SysTime, MsgError> {
    let [b0, b1, b2, b3, b4, b5, s0, s1, s2, s3, sub0, sub1, ..] = *hdr;
    let pri = [b0, b1, b2, b3, b4, b5];
    if !is_telemetry_with_secondary_header(&pri) {
        return Err(MsgError::WrongMsgType);
    }
    Ok(SysTime {
        seconds: u32::from_be_bytes([s0, s1, s2, s3]),
        subseconds: u32::from_be_bytes([sub0, sub1, 0, 0]),
    })
}

/// Encodes the telemetry secondary-header timestamp (big-endian 32-bit seconds, upper 16 bits
/// of `subseconds`): the behaviour of `CFE_MSG_SetMsgTime`.
///
/// # Errors
/// Returns `Err(MsgError::WrongMsgType)` if the packet is not CCSDS version 0, is not a
/// telemetry packet, or does not have a secondary header.
pub fn set_msg_time(hdr: &mut [u8; TELEMETRY_HEADER_LEN], time: SysTime) -> Result<(), MsgError> {
    let [b0, b1, b2, b3, b4, b5, ..] = *hdr;
    let pri = [b0, b1, b2, b3, b4, b5];
    if !is_telemetry_with_secondary_header(&pri) {
        return Err(MsgError::WrongMsgType);
    }
    let [s0, s1, s2, s3] = time.seconds.to_be_bytes();
    let [sub0, sub1, _, _] = time.subseconds.to_be_bytes();
    let [_, _, _, _, _, _, t0, t1, t2, t3, t4, t5, ..] = hdr;
    *t0 = s0;
    *t1 = s1;
    *t2 = s2;
    *t3 = s3;
    *t4 = sub0;
    *t5 = sub1;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        BadArgument, COMMAND_HEADER_LEN, MAX_APID, MAX_FCN_CODE, MAX_HEADER_VERSION,
        MAX_SEQUENCE_COUNT, MAX_SIZE, MsgError, PRIMARY_HEADER_LEN, PacketType, SIZE_OFFSET,
        SequenceFlags, SysTime, TELEMETRY_HEADER_LEN, compute_checksum, generate_checksum,
        get_apid, get_fcn_code, get_has_secondary_header, get_header_version, get_msg_time,
        get_segmentation_flag, get_sequence_count, get_size, get_type,
        is_telemetry_with_secondary_header, next_sequence_count, set_apid, set_fcn_code,
        set_has_secondary_header, set_header_version, set_msg_time, set_segmentation_flag,
        set_sequence_count, set_size, set_type, validate_checksum,
    };

    /// `CFE_MSG_GetNextSequenceCount` of cFE 7.0.1 (`cfe_msg_ccsdspri.c`), transcribed: increment
    /// as `uint16`, then 0 when the result is above the 14-bit mask.
    fn c_reference(count: u16) -> u16 {
        let next = count.wrapping_add(1);
        if next > 0x3FFF { 0 } else { next }
    }

    #[test]
    fn matches_the_c_implementation_for_every_input() {
        for count in 0..=u16::MAX {
            assert_eq!(
                next_sequence_count(count),
                c_reference(count),
                "count {count}"
            );
        }
    }

    #[test]
    fn size_round_trip_and_bounds() {
        let mut hdr = [0xAA, 0xBB, 0xCC, 0xDD, 0x00, 0x00];
        assert_eq!(get_size(&hdr), SIZE_OFFSET);

        let before = hdr;
        assert_eq!(set_size(&mut hdr, SIZE_OFFSET - 1), Err(BadArgument));
        assert_eq!(set_size(&mut hdr, 0), Err(BadArgument));
        assert_eq!(set_size(&mut hdr, MAX_SIZE + 1), Err(BadArgument));
        assert_eq!(set_size(&mut hdr, usize::MAX), Err(BadArgument));
        assert_eq!(hdr, before);

        for size in [SIZE_OFFSET, 8, 256, 0x1234, MAX_SIZE] {
            assert_eq!(set_size(&mut hdr, size), Ok(()));
            assert_eq!(get_size(&hdr), size);
            assert_eq!(&hdr[..4], &[0xAA, 0xBB, 0xCC, 0xDD]);
        }
        assert_eq!(hdr[4], 0xFF);
        assert_eq!(hdr[5], 0xFF);
    }

    #[test]
    fn type_round_trip_preserves_other_bits() {
        let mut hdr = [0xEF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF];
        assert_eq!(get_type(&hdr), PacketType::Telemetry);

        set_type(&mut hdr, PacketType::Telecommand);
        assert_eq!(get_type(&hdr), PacketType::Telecommand);
        assert_eq!(hdr, [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]);

        set_type(&mut hdr, PacketType::Telemetry);
        assert_eq!(get_type(&hdr), PacketType::Telemetry);
        assert_eq!(hdr, [0xEF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]);
    }

    #[test]
    fn secondary_header_flag_round_trip_preserves_other_bits() {
        let mut hdr = [0xF7, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF];
        assert!(!get_has_secondary_header(&hdr));

        set_has_secondary_header(&mut hdr, true);
        assert!(get_has_secondary_header(&hdr));
        assert_eq!(hdr, [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]);

        set_has_secondary_header(&mut hdr, false);
        assert!(!get_has_secondary_header(&hdr));
        assert_eq!(hdr, [0xF7, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]);
    }

    #[test]
    fn header_version_round_trip_and_bounds() {
        let mut hdr = [0x1F, 0xAB, 0xCD, 0xEF, 0x12, 0x34];
        assert_eq!(get_header_version(&hdr), 0);

        for ver in 0..=MAX_HEADER_VERSION {
            assert_eq!(set_header_version(&mut hdr, ver), Ok(()));
            assert_eq!(get_header_version(&hdr), ver);
            assert_eq!(hdr[0] & 0x1F, 0x1F);
            assert_eq!(&hdr[1..], &[0xAB, 0xCD, 0xEF, 0x12, 0x34]);
        }

        let before = hdr;
        assert_eq!(
            set_header_version(&mut hdr, MAX_HEADER_VERSION + 1),
            Err(BadArgument)
        );
        assert_eq!(set_header_version(&mut hdr, u16::MAX), Err(BadArgument));
        assert_eq!(hdr, before);
    }

    #[test]
    fn apid_round_trip_and_bounds() {
        let mut hdr = [0xF8, 0x00, 0xAB, 0xCD, 0x12, 0x34];
        assert_eq!(get_apid(&hdr), 0);

        for apid in [0, 1, 0x00FF, 0x0100, 0x05A5, MAX_APID] {
            assert_eq!(set_apid(&mut hdr, apid), Ok(()));
            assert_eq!(get_apid(&hdr), apid);
            assert_eq!(hdr[0] & 0xF8, 0xF8);
            assert_eq!(&hdr[2..], &[0xAB, 0xCD, 0x12, 0x34]);
        }

        let before = hdr;
        assert_eq!(set_apid(&mut hdr, MAX_APID + 1), Err(BadArgument));
        assert_eq!(set_apid(&mut hdr, u16::MAX), Err(BadArgument));
        assert_eq!(hdr, before);
    }

    #[test]
    fn segmentation_flag_round_trip_preserves_other_bits() {
        let mut hdr = [0x12, 0x34, 0x3F, 0xAB, 0xCD, 0xEF];
        assert_eq!(get_segmentation_flag(&hdr), SequenceFlags::Continuation);

        let cases = [
            (SequenceFlags::Continuation, 0b00),
            (SequenceFlags::First, 0b01),
            (SequenceFlags::Last, 0b10),
            (SequenceFlags::Unsegmented, 0b11),
        ];
        for (flag, expected_bits) in cases {
            set_segmentation_flag(&mut hdr, flag);
            assert_eq!(get_segmentation_flag(&hdr), flag);
            assert_eq!(hdr[2] >> 6, expected_bits);
            assert_eq!(hdr[2] & 0x3F, 0x3F);
            assert_eq!(&hdr[..2], &[0x12, 0x34]);
            assert_eq!(&hdr[3..], &[0xAB, 0xCD, 0xEF]);
        }
    }

    #[test]
    fn sequence_count_round_trip_and_bounds() {
        let mut hdr = [0x12, 0x34, 0xC0, 0x00, 0x56, 0x78];
        assert_eq!(get_sequence_count(&hdr), 0);

        for count in [0, 1, 0x00FF, 0x0100, 0x2A5A, MAX_SEQUENCE_COUNT] {
            assert_eq!(set_sequence_count(&mut hdr, count), Ok(()));
            assert_eq!(get_sequence_count(&hdr), count);
            assert_eq!(hdr[2] & 0xC0, 0xC0);
            assert_eq!(&hdr[..2], &[0x12, 0x34]);
            assert_eq!(&hdr[4..], &[0x56, 0x78]);
        }

        let before = hdr;
        assert_eq!(
            set_sequence_count(&mut hdr, MAX_SEQUENCE_COUNT + 1),
            Err(BadArgument)
        );
        assert_eq!(set_sequence_count(&mut hdr, u16::MAX), Err(BadArgument));
        assert_eq!(hdr, before);
    }

    #[test]
    fn fcn_code_round_trip_and_preserves_adjacent_bytes() {
        // Version 0 (000), Telecommand (1), Secondary Header (1), APID 0x123
        let mut hdr = [0x19, 0x23, 0xC0, 0x01, 0x00, 0x01, 0x00, 0xAB];
        assert_eq!(get_fcn_code(&hdr), Ok(0));

        for fc in 0..=MAX_FCN_CODE {
            assert_eq!(set_fcn_code(&mut hdr, fc), Ok(()));
            assert_eq!(get_fcn_code(&hdr), Ok(fc));
            assert_eq!(&hdr[..6], &[0x19, 0x23, 0xC0, 0x01, 0x00, 0x01]);
            assert_eq!(hdr[7], 0xAB);
        }

        // get_fcn_code masks off bit 7 (reserved bit).
        hdr[6] = 0xFF;
        assert_eq!(get_fcn_code(&hdr), Ok(0x7F));
        // set_fcn_code clears bit 7.
        assert_eq!(set_fcn_code(&mut hdr, 0x2A), Ok(()));
        assert_eq!(hdr[6], 0x2A);
    }

    #[test]
    fn fcn_code_bounds_and_wrong_msg_type() {
        let valid_cmd = [0x18, 0x00, 0xC0, 0x00, 0x00, 0x01, 0x55, 0xCC];
        let mut hdr = valid_cmd;

        assert_eq!(
            set_fcn_code(&mut hdr, MAX_FCN_CODE + 1),
            Err(MsgError::BadArgument)
        );
        assert_eq!(set_fcn_code(&mut hdr, u16::MAX), Err(MsgError::BadArgument));
        assert_eq!(hdr, valid_cmd);

        // Wrong CCSDS version (version = 1)
        let mut wrong_ver = valid_cmd;
        wrong_ver[0] |= 0x20;
        assert_eq!(get_fcn_code(&wrong_ver), Err(MsgError::WrongMsgType));
        let before = wrong_ver;
        assert_eq!(set_fcn_code(&mut wrong_ver, 1), Err(MsgError::WrongMsgType));
        assert_eq!(wrong_ver, before);

        // Telemetry packet (type bit = 0)
        let mut tlm = valid_cmd;
        tlm[0] &= !0x10;
        assert_eq!(get_fcn_code(&tlm), Err(MsgError::WrongMsgType));
        let before = tlm;
        assert_eq!(set_fcn_code(&mut tlm, 1), Err(MsgError::WrongMsgType));
        assert_eq!(tlm, before);

        // Missing secondary header flag (sec hdr bit = 0)
        let mut no_sec = valid_cmd;
        no_sec[0] &= !0x08;
        assert_eq!(get_fcn_code(&no_sec), Err(MsgError::WrongMsgType));
        let before = no_sec;
        assert_eq!(set_fcn_code(&mut no_sec, 1), Err(MsgError::WrongMsgType));
        assert_eq!(no_sec, before);

        // BadArgument takes precedence over WrongMsgType in set_fcn_code (matching C).
        assert_eq!(
            set_fcn_code(&mut tlm, MAX_FCN_CODE + 1),
            Err(MsgError::BadArgument)
        );
    }

    #[test]
    fn checksum_generate_and_validate_valid_packets() {
        assert_eq!(compute_checksum(&[]), 0xFF);
        assert_eq!(compute_checksum(&[0xFF]), 0x00);

        // All 0xFFs except length=8 and version=0 (matching Test_MSG_Checksum)
        let mut pkt = [0xFF; COMMAND_HEADER_LEN];
        pkt[0] = 0x1F; // version=0, type=cmd, sec_hdr=1, apid=0x7FF
        pkt[4] = 0x00;
        pkt[5] = 0x01; // size = 1 + 7 = 8
        assert_eq!(validate_checksum(&pkt), Ok(false));
        assert_eq!(generate_checksum(&mut pkt), Ok(()));
        assert_eq!(validate_checksum(&pkt), Ok(true));

        // All 0x00s except cmd, sechdr, size=8 (matching Test_MSG_Checksum)
        let mut pkt_zero = [0x00; COMMAND_HEADER_LEN];
        pkt_zero[0] = 0x18; // version=0, type=cmd, sec_hdr=1
        pkt_zero[5] = 0x01; // size = 8
        assert_eq!(validate_checksum(&pkt_zero), Ok(false));
        assert_eq!(generate_checksum(&mut pkt_zero), Ok(()));
        assert_eq!(validate_checksum(&pkt_zero), Ok(true));

        // Packet with payload (12 bytes: size = 5 + 7 = 12)
        let mut pkt_payload = [
            0x18, 0x01, 0xC0, 0x02, 0x00, 0x05, 0x03, 0x00, 0xAA, 0x55, 0x12, 0x34,
        ];
        assert_eq!(generate_checksum(&mut pkt_payload), Ok(()));
        assert_eq!(validate_checksum(&pkt_payload), Ok(true));

        // Corrupted payload byte fails validation; regenerating restores validity.
        pkt_payload[10] ^= 0x01;
        assert_eq!(validate_checksum(&pkt_payload), Ok(false));
        assert_eq!(generate_checksum(&mut pkt_payload), Ok(()));
        assert_eq!(validate_checksum(&pkt_payload), Ok(true));
    }

    #[test]
    fn checksum_wrong_msg_type_and_short_sizes() {
        let valid_cmd = [0x18, 0x00, 0xC0, 0x00, 0x00, 0x01, 0x00, 0x00];

        // Wrong CCSDS version (version = 1)
        let mut wrong_ver = valid_cmd;
        wrong_ver[0] |= 0x20;
        let before = wrong_ver;
        assert_eq!(
            generate_checksum(&mut wrong_ver),
            Err(MsgError::WrongMsgType)
        );
        assert_eq!(wrong_ver, before);
        assert_eq!(validate_checksum(&wrong_ver), Err(MsgError::WrongMsgType));

        // Telemetry packet
        let mut tlm = valid_cmd;
        tlm[0] &= !0x10;
        assert_eq!(generate_checksum(&mut tlm), Err(MsgError::WrongMsgType));
        assert_eq!(validate_checksum(&tlm), Err(MsgError::WrongMsgType));

        // Missing secondary header
        let mut no_sec = valid_cmd;
        no_sec[0] &= !0x08;
        assert_eq!(generate_checksum(&mut no_sec), Err(MsgError::WrongMsgType));
        assert_eq!(validate_checksum(&no_sec), Err(MsgError::WrongMsgType));

        // Shorter than PRIMARY_HEADER_LEN
        let mut too_short = [0x18; PRIMARY_HEADER_LEN - 1];
        assert_eq!(
            generate_checksum(&mut too_short),
            Err(MsgError::BadArgument)
        );
        assert_eq!(validate_checksum(&too_short), Err(MsgError::BadArgument));

        // 7-byte buffer (between PRIMARY_HEADER_LEN and COMMAND_HEADER_LEN)
        let mut seven_bytes = [0x18, 0x00, 0xC0, 0x00, 0x00, 0x00, 0x00];
        assert_eq!(
            generate_checksum(&mut seven_bytes),
            Err(MsgError::BadArgument)
        );
        // size == 7 fits in 7-byte buffer for validate_checksum
        assert!(validate_checksum(&seven_bytes).is_ok());

        // 8-byte buffer with size == 7 (PacketDataLength == 0): generate_checksum hashes 7 bytes
        // and writes byte 7, matching C.
        let mut eight_bytes_size7 = [0x18, 0x00, 0xC0, 0x00, 0x00, 0x00, 0x05, 0xAA];
        assert_eq!(generate_checksum(&mut eight_bytes_size7), Ok(()));
        let expected = compute_checksum(&eight_bytes_size7[..SIZE_OFFSET]);
        assert_eq!(eight_bytes_size7[7], expected);

        // Buffer shorter than encoded packet size (size = 10, buf = 8)
        let mut truncated = [0x18, 0x00, 0xC0, 0x00, 0x00, 0x03, 0x00, 0x00];
        assert_eq!(
            generate_checksum(&mut truncated),
            Err(MsgError::BadArgument)
        );
        assert_eq!(validate_checksum(&truncated), Err(MsgError::BadArgument));
    }

    #[test]
    fn msg_time_round_trip_truncation_and_preserves_adjacent_bytes() {
        // Version 0 (000), Telemetry (0), Secondary Header (1), APID 0x123
        let pri = [0x09, 0x23, 0xC0, 0x01, 0x00, 0x09];
        assert!(is_telemetry_with_secondary_header(&pri));

        let spare = [0xDE, 0xAD, 0xBE, 0xEF];
        let mut hdr = [
            pri[0], pri[1], pri[2], pri[3], pri[4], pri[5], 0, 0, 0, 0, 0, 0, spare[0], spare[1],
            spare[2], spare[3],
        ];
        assert_eq!(get_msg_time(&hdr), Ok(SysTime::default()));

        let cases = [
            (
                SysTime {
                    seconds: 0,
                    subseconds: 0,
                },
                SysTime {
                    seconds: 0,
                    subseconds: 0,
                },
                [0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
            ),
            (
                SysTime {
                    seconds: 0x1234_5678,
                    subseconds: 0xABCD_EF12,
                },
                SysTime {
                    seconds: 0x1234_5678,
                    subseconds: 0xABCD_0000,
                },
                [0x12, 0x34, 0x56, 0x78, 0xAB, 0xCD],
            ),
            (
                SysTime {
                    seconds: u32::MAX,
                    subseconds: u32::MAX,
                },
                SysTime {
                    seconds: u32::MAX,
                    subseconds: 0xFFFF_0000,
                },
                [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF],
            ),
        ];

        for (input, expected, expected_bytes) in cases {
            assert_eq!(set_msg_time(&mut hdr, input), Ok(()));
            assert_eq!(&hdr[..6], &pri);
            assert_eq!(&hdr[6..12], &expected_bytes);
            assert_eq!(&hdr[12..16], &spare);
            assert_eq!(get_msg_time(&hdr), Ok(expected));
        }

        // All-0xFF header (except version=0, type=Tlm) decodes to max seconds and 0xFFFF0000 subseconds.
        let mut all_ff = [0xFF; TELEMETRY_HEADER_LEN];
        all_ff[0] = 0x0F; // version=0, type=Tlm, sec_hdr=1, apid=0x7FF
        assert_eq!(
            get_msg_time(&all_ff),
            Ok(SysTime {
                seconds: u32::MAX,
                subseconds: 0xFFFF_0000,
            })
        );
    }

    #[test]
    fn msg_time_wrong_msg_type() {
        let valid_tlm = [
            0x08, 0x00, 0xC0, 0x00, 0x00, 0x09, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0xAA, 0xBB,
            0xCC, 0xDD,
        ];
        let new_time = SysTime {
            seconds: 1,
            subseconds: 0x1234_0000,
        };

        // Command packet (type bit = 1)
        let mut cmd = valid_tlm;
        cmd[0] |= 0x10;
        assert!(!is_telemetry_with_secondary_header(&[
            cmd[0], cmd[1], cmd[2], cmd[3], cmd[4], cmd[5],
        ]));
        let before = cmd;
        assert_eq!(get_msg_time(&cmd), Err(MsgError::WrongMsgType));
        assert_eq!(
            set_msg_time(&mut cmd, new_time),
            Err(MsgError::WrongMsgType)
        );
        assert_eq!(cmd, before);

        // Wrong CCSDS version (version = 1)
        let mut wrong_ver = valid_tlm;
        wrong_ver[0] |= 0x20;
        assert!(!is_telemetry_with_secondary_header(&[
            wrong_ver[0],
            wrong_ver[1],
            wrong_ver[2],
            wrong_ver[3],
            wrong_ver[4],
            wrong_ver[5],
        ]));
        let before = wrong_ver;
        assert_eq!(get_msg_time(&wrong_ver), Err(MsgError::WrongMsgType));
        assert_eq!(
            set_msg_time(&mut wrong_ver, new_time),
            Err(MsgError::WrongMsgType)
        );
        assert_eq!(wrong_ver, before);

        // Missing secondary header flag (sec hdr bit = 0)
        let mut no_sec = valid_tlm;
        no_sec[0] &= !0x08;
        assert!(!is_telemetry_with_secondary_header(&[
            no_sec[0], no_sec[1], no_sec[2], no_sec[3], no_sec[4], no_sec[5],
        ]));
        let before = no_sec;
        assert_eq!(get_msg_time(&no_sec), Err(MsgError::WrongMsgType));
        assert_eq!(
            set_msg_time(&mut no_sec, new_time),
            Err(MsgError::WrongMsgType)
        );
        assert_eq!(no_sec, before);
    }
}
