//! Safe Rust core of a replacement for the cFE MSG module.
//!
//! Each function here is the behaviour of one `CFE_MSG_*` function of cFE, without pointers or
//! C types. The `cfe-msg-ffi` crate exports them under the C names. Space Packet behaviour comes
//! from Aldis; what cFS adds to the Blue Book is implemented here.

#![no_std]
#![forbid(unsafe_code)]

use aldis::core::ids::SeqCount;

/// The Packet Sequence Count that follows `count`: the behaviour of
/// `CFE_MSG_GetNextSequenceCount`.
///
/// A valid count advances modulo 16 384 (CCSDS 133.0, Packet Sequence Count). cFE takes any
/// `uint16`, and for a value above 16 383 it returns 0; this function does the same.
#[must_use]
pub fn next_sequence_count(count: u16) -> u16 {
    match SeqCount::new(count) {
        Some(valid) => valid.next().get(),
        None => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::next_sequence_count;

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
}
