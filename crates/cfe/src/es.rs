//! Executive Services (ES) safe wrappers.

/// Application run status for the ES run loop.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum RunStatus {
    /// Undefined run status.
    Undefined = cfe_sys::CFE_ES_RunStatus_CFE_ES_RunStatus_UNDEFINED,
    /// Normal application execution.
    AppRun = cfe_sys::CFE_ES_RunStatus_CFE_ES_RunStatus_APP_RUN,
    /// Normal exit request.
    AppExit = cfe_sys::CFE_ES_RunStatus_CFE_ES_RunStatus_APP_EXIT,
    /// Application encountered an error.
    AppError = cfe_sys::CFE_ES_RunStatus_CFE_ES_RunStatus_APP_ERROR,
    /// Exception occurred.
    SysException = cfe_sys::CFE_ES_RunStatus_CFE_ES_RunStatus_SYS_EXCEPTION,
    /// Restart system.
    SysRestart = cfe_sys::CFE_ES_RunStatus_CFE_ES_RunStatus_SYS_RESTART,
    /// Reload application.
    SysReload = cfe_sys::CFE_ES_RunStatus_CFE_ES_RunStatus_SYS_RELOAD,
    /// Delete application.
    SysDelete = cfe_sys::CFE_ES_RunStatus_CFE_ES_RunStatus_SYS_DELETE,
    /// Core application initialization error.
    CoreAppInitError = cfe_sys::CFE_ES_RunStatus_CFE_ES_RunStatus_CORE_APP_INIT_ERROR,
    /// Core application runtime error.
    CoreAppRuntimeError = cfe_sys::CFE_ES_RunStatus_CFE_ES_RunStatus_CORE_APP_RUNTIME_ERROR,
}

impl RunStatus {
    /// Convert from a raw `uint32`.
    #[must_use]
    pub const fn from_raw(raw: u32) -> Option<Self> {
        match raw {
            cfe_sys::CFE_ES_RunStatus_CFE_ES_RunStatus_UNDEFINED => Some(Self::Undefined),
            cfe_sys::CFE_ES_RunStatus_CFE_ES_RunStatus_APP_RUN => Some(Self::AppRun),
            cfe_sys::CFE_ES_RunStatus_CFE_ES_RunStatus_APP_EXIT => Some(Self::AppExit),
            cfe_sys::CFE_ES_RunStatus_CFE_ES_RunStatus_APP_ERROR => Some(Self::AppError),
            cfe_sys::CFE_ES_RunStatus_CFE_ES_RunStatus_SYS_EXCEPTION => Some(Self::SysException),
            cfe_sys::CFE_ES_RunStatus_CFE_ES_RunStatus_SYS_RESTART => Some(Self::SysRestart),
            cfe_sys::CFE_ES_RunStatus_CFE_ES_RunStatus_SYS_RELOAD => Some(Self::SysReload),
            cfe_sys::CFE_ES_RunStatus_CFE_ES_RunStatus_SYS_DELETE => Some(Self::SysDelete),
            cfe_sys::CFE_ES_RunStatus_CFE_ES_RunStatus_CORE_APP_INIT_ERROR => {
                Some(Self::CoreAppInitError)
            }
            cfe_sys::CFE_ES_RunStatus_CFE_ES_RunStatus_CORE_APP_RUNTIME_ERROR => {
                Some(Self::CoreAppRuntimeError)
            }
            _ => None,
        }
    }

    /// Convert to the raw `uint32`.
    #[must_use]
    pub const fn to_raw(self) -> u32 {
        match self {
            Self::Undefined => cfe_sys::CFE_ES_RunStatus_CFE_ES_RunStatus_UNDEFINED,
            Self::AppRun => cfe_sys::CFE_ES_RunStatus_CFE_ES_RunStatus_APP_RUN,
            Self::AppExit => cfe_sys::CFE_ES_RunStatus_CFE_ES_RunStatus_APP_EXIT,
            Self::AppError => cfe_sys::CFE_ES_RunStatus_CFE_ES_RunStatus_APP_ERROR,
            Self::SysException => cfe_sys::CFE_ES_RunStatus_CFE_ES_RunStatus_SYS_EXCEPTION,
            Self::SysRestart => cfe_sys::CFE_ES_RunStatus_CFE_ES_RunStatus_SYS_RESTART,
            Self::SysReload => cfe_sys::CFE_ES_RunStatus_CFE_ES_RunStatus_SYS_RELOAD,
            Self::SysDelete => cfe_sys::CFE_ES_RunStatus_CFE_ES_RunStatus_SYS_DELETE,
            Self::CoreAppInitError => {
                cfe_sys::CFE_ES_RunStatus_CFE_ES_RunStatus_CORE_APP_INIT_ERROR
            }
            Self::CoreAppRuntimeError => {
                cfe_sys::CFE_ES_RunStatus_CFE_ES_RunStatus_CORE_APP_RUNTIME_ERROR
            }
        }
    }
}

/// Checks the run status and increments the task counter.
///
/// Returns `true` if the application should continue running.
#[must_use]
pub fn run_loop(run_status: &mut RunStatus) -> bool {
    let mut raw = run_status.to_raw();
    // SAFETY: CFE_ES_RunLoop takes a valid pointer to a uint32.
    let cont = unsafe { cfe_sys::CFE_ES_RunLoop(&raw mut raw) };
    if let Some(updated) = RunStatus::from_raw(raw) {
        *run_status = updated;
    }
    cont
}

/// Waits for cFE startup sync before proceeding with app execution.
pub fn wait_for_startup_sync(timeout_ms: u32) {
    // SAFETY: CFE_ES_WaitForStartupSync takes a timeout in milliseconds.
    unsafe {
        cfe_sys::CFE_ES_WaitForStartupSync(timeout_ms);
    }
}

/// Exits the calling application with the specified status.
pub fn exit_app(status: RunStatus) {
    // SAFETY: CFE_ES_ExitApp takes an exit status integer.
    unsafe {
        cfe_sys::CFE_ES_ExitApp(status.to_raw());
    }
}
