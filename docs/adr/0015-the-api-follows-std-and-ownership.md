# 0015 — The 0.2 API follows std and ownership

Status: accepted (2026-10-13); supersedes the `DropPolicy` of ADR 0004 and the unsafe trait of ADR 0006

## Context

0.1 kept weak representations.
`CreationFlags` accepted console combinations that Windows rejects, three mitigation setters took a bare `bool`, and a pseudoconsole travelled as a raw `isize`.
Every failure was an `io::Error` that had lost the operation that failed.
A running spawn resumed its child before the crate released the attribute list and the transferred duplicates.

PR #14 designed a 0.2 without these representations, but it named its choices after Win32 and left the common tasks as long as before.
0.2 breaks the API anyway, so it was designed again from what its users do:
replace `std::process::Command`, end a process tree, run a console tool from a GUI program, attach a pseudoconsole, and hand handles to children.

## Decision

- What `std::process` offers keeps its name, signature shape, and behavior.
  `Child::kill` returns `Ok` once the process has exited, as std's does, and `Command` prints with `Debug` as std's does.
  `Stdio` converts from a child's pipe end and from `io::stdout` and `io::stderr`, and the handle types implement `AsRawHandle`.
- Ownership decides placement.
  `Command` owns every creation choice that borrows nothing: `own_process_tree`, `console`, `priority`, `mitigation`, and one setter per creation flag.
  `SpawnOptions<'a>` lends only what one spawn borrows: Jobs, an alternate parent, and a pseudoconsole.
- `Command::own_process_tree` replaces `DropPolicy` and the `JobClosePolicy` of #14.
  Its private Job makes `kill`, drop, and output collection end the whole tree, and `Child::wait_tree` waits until every process of the tree has exited.
  `Child::cleanup` from #14 is not added, because `kill` and drop cover it.
- A pseudoconsole follows the I/O-safety pattern of std.
  `BorrowedPseudoConsole::borrow_raw` is the one unsafe step, `AsPseudoConsole` is a safe trait, and `SpawnOptions::pseudo_console` takes a reference or a borrow.
  `borrow_raw` lives in `sys`, so `handles.rs` holds no unsafe code.
- `SpawnError` keeps the category: rejected input, a failed Windows operation with its phase, or failed releases with the failure that made them necessary.
  Every error type is a leaf with a complete message, so a printer of error chains shows each cause once.
  A Windows failure converts into the `io::Error` that Windows reported, so `kind` and `raw_os_error` are those of std.
  The workspace denies `clippy::error_impl_error`, so the type isn't named `Error`.
- Every spawn creates its process suspended, releases what it acquired for the creation, and only then resumes the main thread or returns a `SuspendedChild`.
- A console mode beside a pseudoconsole, and a command line longer than `CreateProcessW` accepts, are rejected before Windows is asked.

Rejected:

- A timed wait, which ADR 0010 bans; `AsHandle` with `SYNCHRONIZE` serves a caller that needs one.
- Tree ownership by default, because a fire-and-forget spawn, such as opening an editor, would die with its `Child`.
- A runner for batch files, because quoting for `cmd.exe` is the BatBadBut hazard, which the proofs don't cover.
  The documentation shows how to name the shell instead.
- The program name inside errors, which std and tokio leave to the caller's context.

## Consequences

- Each common task is one expression, and the crate documentation leads with them.
- `docs/migration-0.2.md` maps every 0.1 item to its replacement.
- The Quint lifecycle model drops the `Denied` outcome of `kill`, and its negative control claims that dropping a `Child` ends its process.
- The Alloy model of the handle transfer gains the resume step, and a design that resumes before the release, which its checks refute.
- Kani proves the priority class and the conflict between a console mode and a pseudoconsole.
