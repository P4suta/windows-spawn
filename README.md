# spawnkit

Advanced Windows process creation: `CreateProcessW` + `STARTUPINFOEX` +
`PROC_THREAD_ATTRIBUTE_LIST`, with the lifetimes of attribute values enforced by
the type system.

> **Status: scaffold.** Every function body is `todo!()`. The types, signatures
> and documentation exist so the shape of the API can be argued about before any
> `unsafe` is written. Nothing here spawns a process yet.

## Prior art

This section comes first on purpose. `spawnkit` is **not the first** crate to
build a `PROC_THREAD_ATTRIBUTE_LIST` safely, and any claim otherwise would be
false.

| Project | What it does with `PROC_THREAD_ATTRIBUTE_LIST` | Status |
| --- | --- | --- |
| `std::os::windows::process::ProcThreadAttributeList` | The real thing, in std | **nightly only** — feature `windows_process_extensions_raw_attribute`, tracking issue [rust-lang/rust#114854](https://github.com/rust-lang/rust/issues/114854), open since 2023. The public API is `unsafe fn raw_attribute`, and "Creating safe interface for setting attributes" is still an unresolved question. Stable std cannot touch attribute lists at all. |
| [`firehazard`](https://crates.io/crates/firehazard) | The most complete prior art: a safe RAII `ThreadAttributeList` builder covering 20+ attributes, including `PARENT_PROCESS`, `HANDLE_LIST`, `MITIGATION_POLICY`, `JOB_LIST` and `PSEUDOCONSOLE` | Version **0.0.0** pre-release, no crates.io release since September 2022. 5,441 downloads. Scope is an entire sandboxing research toolkit, not a process-spawning library. |
| [`conpty`](https://crates.io/crates/conpty) | `PSEUDOCONSOLE` only, handled safely but internally | 385K downloads. Not a general attribute-list API. |
| [`crossmist`](https://crates.io/crates/crossmist) | `HANDLE_LIST` for explicit handle inheritance, internally | 28K downloads. Built for its own IPC model. |
| [`rappct`](https://crates.io/crates/rappct) | Has an RAII `AttrList` internally, for AppContainer/LPAC work | The type is `pub(crate)`; you cannot use it from outside. |
| [`process-wrap`](https://crates.io/crates/process-wrap) | — | 9.9M downloads. Job objects and process groups, but **does not touch `STARTUPINFOEX` attribute lists**. |
| [`win32job`](https://crates.io/crates/win32job) | — | 1.1M downloads. Job objects only, same gap. |
| Application-internal implementations | Ad-hoc `InitializeProcThreadAttributeList` wrappers | `openai/codex` (`windows-sandbox-rs/proc_thread_attr.rs`), `microsoft/openvmm`, `DataDog/libdatadog`, `warp`, `zellij`, and 854 hits for `InitializeProcThreadAttributeList` in GitHub code search. |

## Why spawnkit exists anyway

The gap is not "nobody has done this". The gap is that **there is no default**.

The one comprehensive implementation (`firehazard`) has been parked at `0.0.0`
since 2022 and lives inside a much larger sandboxing toolkit. The mature,
widely-depended-on neighbours (`process-wrap`, `win32job`) deliberately stop
short of attribute lists. So every application that needs explicit handle
inheritance or a mitigation policy writes the two-phase allocation dance again,
by hand, in `unsafe` — 854 times and counting.

`spawnkit` is three bets on filling that slot:

1. **Narrow and deep.** One job: creating a Windows process. Not sandboxing, not
   process supervision, not a terminal. A small API surface is one that can
   actually be stabilised.
2. **Semver discipline.** A real `0.1.0`, then real releases, with a changelog
   and no pre-release limbo. This is the axis on which the existing prior art
   is weakest, and it is the whole reason people copy-paste instead of
   depending.
3. **Composable, not competitive.** `spawnkit` is designed to sit *underneath*
   or *beside* the existing ecosystem: adopt a job handle from `win32job`,
   borrow an `HPCON` from a ConPTY library, hand the resulting process to
   `process-wrap`. It does not want to own your process supervision.

If `firehazard` reaches 0.1 with a stable release cadence, that bet loses and
you should use `firehazard`. That is an honest outcome, and it is written down
in [`docs/adr/0001-why-not-firehazard.md`](docs/adr/0001-why-not-firehazard.md).

## Quick start

This is the API sketch, not working code — every call panics with `todo!()`
today.

```rust,ignore
use spawnkit::{Job, MitigationPolicy, RawHandleRef, WindowsCommand};

// Exactly two handles reach the child. Not "every inheritable handle in the
// process", which is what `bInheritHandles = TRUE` means on its own.
let log = std::fs::File::create("child.log")?;
let data = std::fs::File::open("input.bin")?;
let inherited = [RawHandleRef::borrow(&log), RawHandleRef::borrow(&data)];

// The child joins the job atomically with creation: no window in which it
// exists outside the job.
let job = Job::create()?;
job.kill_on_close(true)?;

let mut child = WindowsCommand::new(r"C:\Windows\System32\cmd.exe")
    .args(["/c", "echo hello"])
    .inherit_handles(&inherited)
    .attach_to_job(&job)
    .mitigation(MitigationPolicy::NO_DYNAMIC_CODE | MitigationPolicy::STRICT_HANDLE_CHECKS)
    .spawn()?;

let status = child.wait()?;
assert!(status.success());
```

The borrow checker is doing real work here: `inherited`, `job` and the files
must all outlive the `WindowsCommand`, because the attribute list stores raw
pointers into them and `CreateProcessW` dereferences those pointers. Drop `log`
early and the code does not compile. In C, it compiles and reads freed memory.

## Non-goals

* **Not a sandboxing framework.** AppContainer, LPAC, restricted tokens, ACL
  manipulation and capability SIDs are out of scope; `rappct` and `firehazard`
  are the right neighbourhood for that. `spawnkit` will expose the mitigation
  policy attribute, which is a process-creation knob, and stop there.
* **Not a cross-platform abstraction.** There is no Unix backend and there never
  will be. `#[cfg(windows)]` gates the entire public API, so a non-Windows build
  fails loudly at the `use` site rather than silently degrading.
* **Not a replacement for `std::process::Command`.** If you do not need an
  attribute list, use std — it is better tested and always will be. `spawnkit`
  is for the cases std cannot express on stable.
* **Not a process supervisor.** No restart policies, no signal forwarding, no
  async runtime integration. Compose with `process-wrap` for that.

## Roadmap

* **v0.1** — `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` (explicit inheritance),
  `PROC_THREAD_ATTRIBUTE_JOB_LIST`, `CREATE_SUSPENDED`, kill-tree-on-drop.
  That is the set that covers most of the ad-hoc implementations in the wild.
* **v0.2+** — `PROC_THREAD_ATTRIBUTE_PARENT_PROCESS`,
  `PROC_THREAD_ATTRIBUTE_MITIGATION_POLICY` (including the second policy word),
  `PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE`.
* **Later** — move the lower layer of
  [`conpty-oxide`](https://crates.io/crates/conpty-oxide) (its job, pipe and
  process-creation code) onto `spawnkit`, so the two crates share one
  audited process-creation path instead of two.

## Design notes

* [ADR 0001 — why a new crate rather than `firehazard`](docs/adr/0001-why-not-firehazard.md)
* [ADR 0002 — why spawnkit owns its `CreateProcessW` call](docs/adr/0002-own-createprocess.md)
* [ADR 0003 — the attribute lifetime model](docs/adr/0003-attribute-lifetime-model.md)
* [ADR 0004 — job attachment: `JOB_LIST` vs `AssignProcessToJobObject`](docs/adr/0004-job-attachment.md)

## License

Licensed under either of

* Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
* MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
