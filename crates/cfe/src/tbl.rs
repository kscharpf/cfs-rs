//! Table Services (TBL) safe wrappers.

use core::ffi::c_void;
use core::num::NonZeroUsize;

use crate::status::Status;

/// Maximum length of a raw table name including the null terminator.
#[expect(
    clippy::as_conversions,
    reason = "CFE_MISSION_TBL_MAX_NAME_LENGTH is a C bindgen u32 constant used as buffer size"
)]
pub const MAX_TABLE_NAME_LEN: usize = cfe_sys::CFE_MISSION_TBL_MAX_NAME_LENGTH as usize;

/// Maximum length of a table file path including the null terminator.
#[expect(
    clippy::as_conversions,
    reason = "CFE_MISSION_MAX_PATH_LEN is a C bindgen u32 constant used as buffer size"
)]
pub const MAX_PATH_LEN: usize = cfe_sys::CFE_MISSION_MAX_PATH_LEN as usize;

/// Supported table registration options for [`Table::register`].
///
/// Excludes `CFE_TBL_OPT_USR_DEF_ADDR` (no user-defined address load API) and
/// invalid cFE combinations such as double-buffered or critical dump-only tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TableOptions {
    /// Single-buffered, load/dump, non-critical (`CFE_TBL_OPT_DEFAULT`).
    #[default]
    Default,
    /// Single-buffered, load/dump, critical (`CFE_TBL_OPT_CRITICAL`).
    Critical,
    /// Double-buffered, load/dump, non-critical (`CFE_TBL_OPT_DBL_BUFFER`).
    DoubleBuffered,
    /// Double-buffered, load/dump, critical (`CFE_TBL_OPT_DBL_BUFFER | CFE_TBL_OPT_CRITICAL`).
    DoubleBufferedCritical,
    /// Single-buffered, dump-only, non-critical (`CFE_TBL_OPT_DUMP_ONLY`).
    DumpOnly,
}

impl TableOptions {
    /// Returns the raw cFE option bitmask (`CFE_TBL_OPT_*`).
    #[must_use]
    #[expect(
        clippy::cast_possible_truncation,
        reason = "CFE_TBL_OPT_* constants are 16-bit flags stored as u32 by bindgen"
    )]
    #[expect(
        clippy::as_conversions,
        reason = "u16::try_from is not const in stable Rust"
    )]
    pub const fn to_raw(self) -> u16 {
        match self {
            Self::Default => cfe_sys::CFE_TBL_OPT_DEFAULT as u16,
            Self::Critical => cfe_sys::CFE_TBL_OPT_CRITICAL as u16,
            Self::DoubleBuffered => cfe_sys::CFE_TBL_OPT_DBL_BUFFER as u16,
            Self::DoubleBufferedCritical => {
                (cfe_sys::CFE_TBL_OPT_DBL_BUFFER | cfe_sys::CFE_TBL_OPT_CRITICAL) as u16
            }
            Self::DumpOnly => cfe_sys::CFE_TBL_OPT_DUMP_ONLY as u16,
        }
    }
}

/// Sentinel invalid table handle (`CFE_TBL_BAD_TABLE_HANDLE`).
const BAD_TABLE_HANDLE: cfe_sys::CFE_TBL_Handle_t = -1;

#[expect(
    clippy::as_conversions,
    reason = "From is not const in stable Rust (RFC 143874)"
)]
const _: () = {
    assert!(TableOptions::Default.to_raw() as u32 == cfe_sys::CFE_TBL_OPT_DEFAULT);
    assert!(TableOptions::Critical.to_raw() as u32 == cfe_sys::CFE_TBL_OPT_CRITICAL);
    assert!(TableOptions::DoubleBuffered.to_raw() as u32 == cfe_sys::CFE_TBL_OPT_DBL_BUFFER);
    assert!(
        TableOptions::DoubleBufferedCritical.to_raw() as u32
            == (cfe_sys::CFE_TBL_OPT_DBL_BUFFER | cfe_sys::CFE_TBL_OPT_CRITICAL)
    );
    assert!(TableOptions::DumpOnly.to_raw() as u32 == cfe_sys::CFE_TBL_OPT_DUMP_ONLY);
    assert!((BAD_TABLE_HANDLE as i32) == cfe_sys::CFE_TBL_BAD_TABLE_HANDLE);
};

/// Scope guard ensuring `CFE_TBL_ReleaseAddress` is called before `read_bytes` returns.
struct ReleaseGuard {
    handle: cfe_sys::CFE_TBL_Handle_t,
}

impl Drop for ReleaseGuard {
    fn drop(&mut self) {
        // SAFETY: self.handle is a valid registered table handle whose address was obtained via CFE_TBL_GetAddress.
        unsafe {
            let _ = cfe_sys::CFE_TBL_ReleaseAddress(self.handle);
        }
    }
}

/// A registered cFE Table Services (TBL) table handle.
///
/// Automatically unregisters the table via `CFE_TBL_Unregister` when dropped,
/// unless [`Table::unregister`] is called explicitly.
#[derive(Debug)]
pub struct Table {
    handle: cfe_sys::CFE_TBL_Handle_t,
    size: NonZeroUsize,
    // `CFE_TBL_GetAddress`/`ReleaseAddress` tracks a single lock flag rather than a reference
    // count, so `Table` must not be shared or moved across threads (`!Send + !Sync`).
    _marker: core::marker::PhantomData<*const ()>,
}

impl Table {
    /// Registers a table with cFE Table Services.
    ///
    /// `name` is the raw table name (without the `AppName.` prefix); it must be
    /// non-empty, shorter than [`MAX_TABLE_NAME_LEN`] bytes, and free of interior
    /// null bytes.
    ///
    /// # Errors
    ///
    /// Returns `Err(Status)` with `CFE_TBL_ERR_INVALID_NAME` if `name` is invalid,
    /// or the status returned by `CFE_TBL_Register` for any non-`CFE_SUCCESS`
    /// result (including `CFE_TBL_WARN_DUPLICATE`, which aliases an existing handle).
    pub fn register(name: &str, size: NonZeroUsize, options: TableOptions) -> Result<Self, Status> {
        if name.is_empty() || name.len() >= MAX_TABLE_NAME_LEN || name.as_bytes().contains(&0) {
            return Err(Status::from_raw(cfe_sys::CFE_TBL_ERR_INVALID_NAME));
        }

        let mut name_buf = [0u8; MAX_TABLE_NAME_LEN];
        if let Some(dest) = name_buf.get_mut(..name.len()) {
            dest.copy_from_slice(name.as_bytes());
        }

        let mut handle = BAD_TABLE_HANDLE;
        // SAFETY: `handle` is a valid out-pointer and `name_buf` is null-terminated.
        let raw_status = unsafe {
            cfe_sys::CFE_TBL_Register(
                &raw mut handle,
                name_buf.as_ptr().cast(),
                size.get(),
                options.to_raw(),
                None,
            )
        };
        let status = Status::from_raw(raw_status);
        if raw_status != 0 || handle == BAD_TABLE_HANDLE {
            Err(status)
        } else {
            Ok(Self {
                handle,
                size,
                _marker: core::marker::PhantomData,
            })
        }
    }

    /// Returns the registered size of the table in bytes.
    #[must_use]
    pub const fn size(&self) -> NonZeroUsize {
        self.size
    }

    /// Returns the underlying cFE table handle.
    #[must_use]
    pub const fn handle(&self) -> cfe_sys::CFE_TBL_Handle_t {
        self.handle
    }

    /// Loads the table contents from a cFE table image file (`CFE_TBL_SRC_FILE`).
    ///
    /// # Errors
    ///
    /// Returns `Err(Status)` if `path` is empty, contains interior null bytes,
    /// exceeds [`MAX_PATH_LEN`] - 1 bytes, or if `CFE_TBL_Load` returns a non-success
    /// status code.
    pub fn load_from_file(&mut self, path: &str) -> Result<(), Status> {
        if path.is_empty() || path.as_bytes().contains(&0) {
            return Err(Status::from_raw(cfe_sys::CFE_TBL_BAD_ARGUMENT));
        }
        if path.len() >= MAX_PATH_LEN {
            return Err(Status::from_raw(cfe_sys::CFE_TBL_ERR_FILENAME_TOO_LONG));
        }

        let mut path_buf = [0u8; MAX_PATH_LEN];
        if let Some(dest) = path_buf.get_mut(..path.len()) {
            dest.copy_from_slice(path.as_bytes());
        }

        // SAFETY: `self.handle` is valid and `path_buf` is null-terminated for `CFE_TBL_SRC_FILE`.
        let raw_status = unsafe {
            cfe_sys::CFE_TBL_Load(
                self.handle,
                cfe_sys::CFE_TBL_SrcEnum_CFE_TBL_SRC_FILE,
                path_buf.as_ptr().cast(),
            )
        };
        Status::from_raw(raw_status).result()
    }

    /// Performs standard table management (`CFE_TBL_Manage`).
    ///
    /// Returns `Ok(true)` if the table was updated (`CFE_TBL_INFO_UPDATED`) or
    /// `Ok(false)` if no update occurred (`CFE_SUCCESS`).
    ///
    /// # Errors
    ///
    /// Returns `Err(Status)` if `CFE_TBL_Manage` returns any other status code.
    pub fn manage(&mut self) -> Result<bool, Status> {
        const INFO_UPDATED: i32 = cfe_sys::CFE_TBL_INFO_UPDATED.cast_signed();
        // SAFETY: `self.handle` is a valid registered table handle.
        let raw_status = unsafe { cfe_sys::CFE_TBL_Manage(self.handle) };
        match raw_status {
            0 => Ok(false),
            INFO_UPDATED => Ok(true),
            _ => Err(Status::from_raw(raw_status)),
        }
    }

    /// Queries the pending action status of the table (`CFE_TBL_GetStatus`).
    #[must_use]
    pub fn get_status(&self) -> Status {
        // SAFETY: `self.handle` is a valid registered table handle.
        let raw_status = unsafe { cfe_sys::CFE_TBL_GetStatus(self.handle) };
        Status::from_raw(raw_status)
    }

    /// Acquires the current active table buffer, passes its `&[u8]` view of
    /// `self.size().get()` bytes to `f`, and always releases the address via
    /// `CFE_TBL_ReleaseAddress` before returning.
    ///
    /// Accepts both `CFE_SUCCESS` and `CFE_TBL_INFO_UPDATED` from `CFE_TBL_GetAddress`.
    ///
    /// # Errors
    ///
    /// Returns `Err(Status)` if `CFE_TBL_GetAddress` returns an error (such as
    /// `CFE_TBL_ERR_NEVER_LOADED`) or produces a null pointer.
    pub fn read_bytes<R>(&mut self, f: impl FnOnce(&[u8]) -> R) -> Result<R, Status> {
        const INFO_UPDATED: i32 = cfe_sys::CFE_TBL_INFO_UPDATED.cast_signed();
        let mut ptr: *mut c_void = core::ptr::null_mut();
        // SAFETY: `ptr` is a valid out-pointer and `self.handle` is a valid registered handle.
        let raw_status = unsafe { cfe_sys::CFE_TBL_GetAddress(&raw mut ptr, self.handle) };

        if raw_status != 0 && raw_status != INFO_UPDATED {
            if !ptr.is_null() {
                // SAFETY: `CFE_TBL_GetAddress` returned a non-null pointer that must be released.
                unsafe {
                    let _ = cfe_sys::CFE_TBL_ReleaseAddress(self.handle);
                }
            }
            return Err(Status::from_raw(raw_status));
        }

        if ptr.is_null() {
            return Err(Status::from_raw(cfe_sys::CFE_TBL_ERR_UNREGISTERED));
        }

        let _guard = ReleaseGuard {
            handle: self.handle,
        };

        // SAFETY: `CFE_TBL_GetAddress` succeeded and locked the active buffer of `self.size` bytes;
        // `_guard` guarantees `CFE_TBL_ReleaseAddress` runs when leaving this scope, `self` is
        // exclusively borrowed (`&mut self`) so `f` cannot reentrantly call `read_bytes`, `manage`,
        // or `load_from_file`, `Table` is `!Send + !Sync`, and the slice reference cannot escape `f`.
        let slice = unsafe { core::slice::from_raw_parts(ptr.cast::<u8>(), self.size.get()) };
        Ok(f(slice))
    }

    /// Explicitly unregisters the table via `CFE_TBL_Unregister`.
    ///
    /// # Errors
    ///
    /// Returns `Err(Status)` if `CFE_TBL_Unregister` fails.
    pub fn unregister(mut self) -> Result<(), Status> {
        let handle = self.handle;
        self.handle = BAD_TABLE_HANDLE;
        // SAFETY: `handle` is a valid registered table handle.
        let raw_status = unsafe { cfe_sys::CFE_TBL_Unregister(handle) };
        Status::from_raw(raw_status).result()
    }
}

impl Drop for Table {
    fn drop(&mut self) {
        if self.handle != BAD_TABLE_HANDLE {
            // SAFETY: `self.handle` is a valid registered table handle not yet unregistered.
            unsafe {
                let _ = cfe_sys::CFE_TBL_Unregister(self.handle);
            }
        }
    }
}
