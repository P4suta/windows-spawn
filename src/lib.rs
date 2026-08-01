//! Advanced Windows process creation.
//!
//! `spawnkit` wraps [`CreateProcessW`] with a `STARTUPINFOEX` /
//! `PROC_THREAD_ATTRIBUTE_LIST` so that the interesting knobs of Windows
//! process creation are reachable from safe Rust:
//!
//! * explicit handle inheritance (`PROC_THREAD_ATTRIBUTE_HANDLE_LIST`) instead
//!   of the process-wide `bInheritHandles` shotgun,
//! * re-parenting (`PROC_THREAD_ATTRIBUTE_PARENT_PROCESS`),
//! * process creation mitigation policies
//!   (`PROC_THREAD_ATTRIBUTE_MITIGATION_POLICY`),
//! * atomic job object attachment (`PROC_THREAD_ATTRIBUTE_JOB_LIST`),
//! * pseudoconsole attachment (`PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE`).
//!
//! The central design constraint is that **an attribute list stores raw
//! pointers into the caller's memory**: `UpdateProcThreadAttribute` copies the
//! pointer you hand it, not the value behind it, and every one of those values
//! must still be alive and unmoved when `CreateProcessW` reads the list. In C
//! this is a footgun; here it is the lifetime `'a` on [`AttributeList`] and
//! [`WindowsCommand`]. See `docs/adr/0003-attribute-lifetime-model.md`.
//!
//! # Status
//!
//! **This crate is a scaffold.** Every function body is `todo!()`; the types,
//! signatures and documentation exist so that the shape of the API can be
//! reviewed before any `unsafe` is written. Nothing here spawns a process yet.
//!
//! # Prior art
//!
//! `spawnkit` is not the first crate to touch `PROC_THREAD_ATTRIBUTE_LIST`, and
//! it does not claim to be. `firehazard` already covers 20+ attributes behind a
//! safe RAII builder, `conpty` and `crossmist` solve one attribute each
//! internally, and `process-wrap` / `win32job` own the job-object story without
//! ever touching an attribute list. The gap `spawnkit` aims at is the *default*
//! slot, not the *first* slot. `README.md` has the full table, and
//! `docs/adr/0001-why-not-firehazard.md` has the argument.
//!
//! # Safety policy
//!
//! The crate is `#![deny(unsafe_code)]` today, and the intent is to keep it
//! that way for everything except a single future `sys` module:
//!
//! ```ignore
//! #[allow(unsafe_code)]
//! mod sys; // the only place a raw Win32 call may appear
//! ```
//!
//! Every other module stays in safe Rust and talks to Windows through `sys`.
//!
//! # Platform support
//!
//! Windows only. On other targets the crate compiles to nothing at all — the
//! API is `#[cfg(windows)]` rather than stubbed out, so a non-Windows build
//! fails at the `use` site instead of at run time.
//!
//! [`CreateProcessW`]: https://learn.microsoft.com/windows/win32/api/processthreadsapi/nf-processthreadsapi-createprocessw

#![deny(unsafe_code)]
#![warn(missing_docs)]
#![warn(missing_debug_implementations)]
// Scaffold: every body is `todo!()`, so the private fields that model the
// eventual state are never read and the parameters are never used. Delete both
// allows when the implementation lands — they are load-bearing for exactly as
// long as the bodies are.
#![allow(dead_code)]
#![allow(unused_variables)]

#[cfg(windows)]
mod attributes;
#[cfg(windows)]
mod child;
#[cfg(windows)]
mod command;
#[cfg(windows)]
mod error;
#[cfg(windows)]
mod mitigation;

#[cfg(windows)]
pub use crate::attributes::{
    AttributeList, AttributeListBuilder, Job, ParentProcess, Pcon, RawHandleRef, RawPseudoconsole,
};
#[cfg(windows)]
pub use crate::child::{Child, ExitStatus};
#[cfg(windows)]
pub use crate::command::{Stdio, WindowsCommand};
#[cfg(windows)]
pub use crate::error::{Error, Result};
#[cfg(windows)]
pub use crate::mitigation::MitigationPolicy;
