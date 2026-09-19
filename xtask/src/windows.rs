use windows_sys::Win32::System::Diagnostics::Debug::{
    GetErrorMode, SetErrorMode, SEM_FAILCRITICALERRORS, SEM_NOGPFAULTERRORBOX,
};

pub(crate) struct FaultDialogGuard(u32);

impl FaultDialogGuard {
    pub(crate) fn suppress() -> Self {
        // SAFETY: GetErrorMode has no preconditions and only reads process state.
        let previous = unsafe { GetErrorMode() };
        // SAFETY: SetErrorMode accepts every bit pattern represented by its u32 mode.
        unsafe {
            SetErrorMode(previous | SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX);
        }
        Self(previous)
    }
}

impl Drop for FaultDialogGuard {
    fn drop(&mut self) {
        // SAFETY: the saved value was returned by GetErrorMode in this process.
        unsafe {
            SetErrorMode(self.0);
        }
    }
}
