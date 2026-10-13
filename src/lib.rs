#![doc = include_str!("../docs/crate.md")]
#![deny(unsafe_code)]

/// Runs the README examples as doctests; the rendered crate docs come from `docs/crate.md`.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
mod readme_examples {}

#[cfg(windows)]
mod child;
#[cfg(windows)]
mod command;
#[cfg(any(windows, test, kani))]
mod domain;
#[cfg(windows)]
mod error;
#[cfg(windows)]
#[cfg(test)]
mod failure_tests;
#[cfg(windows)]
mod handles;
#[cfg(any(windows, test, kani))]
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
mod transaction;

#[cfg(windows)]
pub use crate::child::{Child, ChildStderr, ChildStdin, ChildStdout, SuspendedChild};
#[cfg(windows)]
pub use crate::command::Command;
#[cfg(windows)]
pub use crate::error::{
    CleanupError, InputField, Operation, Phase, Result, SpawnError, ValidationError, WindowsError,
};
#[cfg(windows)]
pub use crate::handles::{AsPseudoConsole, BorrowedPseudoConsole, Job, ParentProcess, Stdio};
#[cfg(any(windows, test, kani))]
pub use crate::mitigation::{
    AtlThunkEmulation, BlockNonCetBinaries, CetShadowStacks, ControlFlowGuard, Dep, DynamicCode,
    LoaderIntegrity, Mitigation, MitigationPolicy, ModuleTampering, NonSystemFonts, RelocateImages,
    Sehop, SignedBinaries, UserCetContextIpValidation,
};
#[cfg(windows)]
pub use crate::options::{ConsoleMode, PriorityClass, SpawnOptions};
