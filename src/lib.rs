#![doc = include_str!("../docs/crate.md")]
#![deny(unsafe_code)]
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

#[cfg(doctest)]
#[doc = include_str!("../README.md")]
mod readme_examples {}

mod core_logic;

#[cfg(windows)]
mod backend;
#[cfg(windows)]
mod child;
#[cfg(windows)]
mod command;
#[cfg(windows)]
mod error;
#[cfg(windows)]
mod handles;
#[cfg(windows)]
mod mitigation;
#[cfg(windows)]
mod options;
#[cfg(windows)]
mod plan;
#[cfg(windows)]
mod resource;
#[cfg(windows)]
#[allow(unsafe_code)]
mod sys;
#[cfg(windows)]
mod trace;
#[cfg(windows)]
mod transaction;

#[cfg(windows)]
pub use crate::child::{Child, ChildStderr, ChildStdin, ChildStdout, SuspendedChild};
#[cfg(windows)]
pub use crate::command::Command;
#[cfg(windows)]
pub use crate::error::{
    CleanupError, Error, InputField, Operation, Phase, Result, ValidationError, WindowsError,
};
#[cfg(windows)]
pub use crate::handles::{AsPseudoConsole, BorrowedPseudoConsole, Job, ParentProcess, Stdio};
#[cfg(windows)]
pub use crate::mitigation::{
    AtlThunkPolicy, BlockNonCetBinaries, CetShadowStacks, ControlFlowGuard, DepPolicy, DynamicCode,
    FontDisable, LoaderIntegrity, Mitigation, MitigationPolicy, ModuleTampering, RelocateImages,
    SehopPolicy, SignedBinaries, UserCetContextIpValidation,
};
#[cfg(windows)]
pub use crate::options::{ConsoleMode, JobClosePolicy, SpawnOptions, TerminalMode};
