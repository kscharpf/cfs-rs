//! cFE and OSAL status codes.

use core::fmt;

/// A status code returned by cFE or OSAL APIs.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Status(i32);

impl Status {
    /// Success status code.
    pub const SUCCESS: Self = Self(0);

    /// Create a status from a raw integer.
    #[must_use]
    pub const fn from_raw(raw: i32) -> Self {
        Self(raw)
    }

    /// Return the raw integer status code.
    #[must_use]
    pub const fn as_raw(self) -> i32 {
        self.0
    }

    /// Returns `true` if this status represents success (`0`).
    #[must_use]
    pub const fn is_success(self) -> bool {
        self.0 == 0
    }

    /// Returns `true` if this status represents an error (`!= 0`).
    #[must_use]
    pub const fn is_error(self) -> bool {
        self.0 != 0
    }

    /// Convert to a `Result<(), Status>`.
    ///
    /// # Errors
    ///
    /// Returns `Err(self)` if the status code is non-zero (indicating failure).
    pub const fn result(self) -> Result<(), Self> {
        if self.0 == 0 { Ok(()) } else { Err(self) }
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Status({})", self.0)
    }
}
