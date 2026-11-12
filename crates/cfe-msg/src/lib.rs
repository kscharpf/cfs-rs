//! Safe Rust core of a replacement for the cFE MSG module.
//!
//! Each function here is the behaviour of one `CFE_MSG_*` function of cFE, without pointers or
//! C types. The `cfe-msg-ffi` crate exports them under the C names. Space Packet behaviour comes
//! from Aldis; what cFS adds to the Blue Book is implemented here.

#![no_std]
#![forbid(unsafe_code)]

use aldis::core::ids::{PacketVersion, SeqCount};
pub use aldis::spp::{
    MAX_PACKET_LEN as MAX_SIZE, MIN_PACKET_LEN as SIZE_OFFSET, PRIMARY_HEADER_LEN, PacketType,
};

/// Maximum 3-bit CCSDS Packet Version Number.
pub const MAX_HEADER_VERSION: u16 = PacketVersion::MAX as u16;

/// Byte index of the high octet of the Stream ID in the CCSDS Primary Header.
const STREAM_ID_HI_IDX: usize = 0;

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

/// Error returned when an input argument is outside the valid CCSDS/cFE range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BadArgument;

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

#[cfg(test)]
mod tests {
    use super::{
        BadArgument, MAX_HEADER_VERSION, MAX_SIZE, PacketType, SIZE_OFFSET,
        get_has_secondary_header, get_header_version, get_size, get_type, next_sequence_count,
        set_has_secondary_header, set_header_version, set_size, set_type,
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
}
