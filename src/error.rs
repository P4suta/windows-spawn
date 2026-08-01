//! Error type.

use std::io;

/// The result type used throughout `spawnkit`.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Everything that can go wrong while building an attribute list or spawning a
/// process.
///
/// Win32 failures are kept as an [`io::Error`] built from `GetLastError`, so the
/// original `DWORD` survives round-tripping and can be recovered with
/// [`Error::raw_os_error`]. The `operation` field names the Win32 entry point
/// that failed, because "the operation completed successfully" (error 0 after a
/// failed call) and "access denied" are both far more useful when you know
/// whether they came from `InitializeProcThreadAttributeList`,
/// `UpdateProcThreadAttribute` or `CreateProcessW`.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// A Win32 call failed.
    #[error("{operation} failed")]
    Win32 {
        /// Name of the Win32 function that failed, e.g. `"CreateProcessW"`.
        operation: &'static str,
        /// The underlying `GetLastError` value, as an [`io::Error`].
        #[source]
        source: io::Error,
    },

    /// A string that has to cross into Win32 as a NUL-terminated `PCWSTR`
    /// contains an interior NUL.
    #[error("{field} contains an interior NUL byte")]
    InteriorNul {
        /// Which input was rejected, e.g. `"program"`, `"argument"`, `"environment value"`.
        field: &'static str,
    },

    /// The same `PROC_THREAD_ATTRIBUTE_*` was set twice.
    ///
    /// `UpdateProcThreadAttribute` rejects duplicate attributes with
    /// `ERROR_OBJECT_NAME_EXISTS`; `spawnkit` catches it earlier, at the
    /// builder, where the caller still has a useful stack.
    #[error("the {attribute} attribute was set more than once")]
    DuplicateAttribute {
        /// Human-readable attribute name, e.g. `"PROC_THREAD_ATTRIBUTE_HANDLE_LIST"`.
        attribute: &'static str,
    },

    /// A combination of attributes that Windows does not accept was requested.
    ///
    /// The canonical example: `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` requires
    /// `bInheritHandles = TRUE`, and every handle in the list must be
    /// inheritable — an ordinary [`OwnedHandle`](std::os::windows::io::OwnedHandle)
    /// usually is not.
    #[error("incompatible spawn configuration: {reason}")]
    Incompatible {
        /// What conflicted, phrased for a human.
        reason: &'static str,
    },
}

impl Error {
    /// The raw Win32 error code, when this error came from a Win32 call.
    ///
    /// Equivalent to `io::Error::raw_os_error` on the wrapped source.
    pub fn raw_os_error(&self) -> Option<i32> {
        todo!("expose the GetLastError value behind Error::Win32")
    }

    /// Wrap the current thread's `GetLastError` value.
    ///
    /// Called immediately after a failing Win32 call, before anything else has
    /// had a chance to clobber the thread-local error state.
    pub(crate) fn last_os_error(operation: &'static str) -> Self {
        todo!("build Error::Win32 from operation + io::Error::last_os_error()")
    }
}
