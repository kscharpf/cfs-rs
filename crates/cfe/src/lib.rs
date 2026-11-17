//! Safe wrappers for cFE and OSAL APIs in `cfs-rs`.

#![no_std]
#![expect(unsafe_code, reason = "safe wrapper over cfe-sys C ABI")]

pub mod es;
pub mod evs;
pub mod msg;
pub mod osal;
pub mod sb;
pub mod status;
pub mod tbl;

#[cfg(test)]
mod tests;
