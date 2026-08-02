#![doc = include_str!("../docs/crate.md")]
#![deny(unsafe_code)]

#[cfg(windows)]
mod child;
#[cfg(windows)]
mod command;
#[cfg(windows)]
#[allow(unsafe_code)]
mod handles;
#[cfg(windows)]
mod mitigation;
#[cfg(windows)]
mod options;
#[cfg(windows)]
mod plan;
#[cfg(windows)]
#[allow(unsafe_code)]
mod sys;
#[cfg(windows)]
mod transaction;

#[cfg(windows)]
pub use crate::child::{Child, ChildStderr, ChildStdin, ChildStdout, SuspendedChild};
#[cfg(windows)]
pub use crate::command::Command;
#[cfg(windows)]
pub use crate::handles::{AsPseudoConsole, Job, ParentProcess, Stdio};
#[cfg(windows)]
pub use crate::mitigation::{
    BlockNonCetBinaries, CetShadowStacks, ControlFlowGuard, DynamicCode, FontDisable,
    LoaderIntegrity, Mitigation, MitigationPolicy, ModuleTampering, RelocateImages, SignedBinaries,
    UserCetContextIpValidation,
};
#[cfg(windows)]
pub use crate::options::{CreationFlags, DropPolicy, SpawnOptions};
