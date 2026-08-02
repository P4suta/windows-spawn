# Windows process creation with explicit ownership

`windows-spawn` owns one complete `CreateProcessW` transaction for Windows
features that stable [`std::process`] cannot express safely. Use
[`std::process::Command`] for ordinary portable child processes. Use this crate
when process creation needs explicit handle transfer, ordered Job attachment,
typed mitigation policies, `ConPTY`, or suspended inspection.

The crate intentionally exposes no public API on non-Windows targets. This
allows cross-platform dependency graphs to be checked without implying runtime
support outside Windows.

# Platform contract

The core process-creation transaction and `ConPTY` integration require Windows 10
version 1809 or later. Individual [`MitigationPolicy`] fields can require newer
Windows versions, a particular processor architecture, hardware support, or
compatible executable metadata. CET policies are a notable example, and
pointer authentication is ARM64-specific.

The crate does not preflight or silently weaken requested mitigations. When
Windows cannot apply a requested combination, spawning returns the operating
system error from `CreateProcessW`. Nested Job behavior likewise remains
subject to Jobs already imposed by the host.

# Commands and executable lookup

- `.bat` and `.cmd` programs are rejected. Invoke `cmd.exe` explicitly when a
  shell boundary is intended.
- Executable lookup follows Rust's safe Windows search behavior, does not
  search the current directory, and passes the resolved path as
  `lpApplicationName`.
- [`Command::raw_arg`] appends already-encoded Windows command-line syntax. It
  does not invoke a shell and must only receive syntax appropriate for the
  target executable's parser.
- Raw attribute injection, raw creation flags, and raw mitigation constructors
  are intentionally absent.

# Handle and capability ownership

[`Command`] is reusable and stores execution intent. [`Command::arg_handle`]
and [`Command::env_handle`] take a private, non-inheritable duplicate when they
are configured. The source handle may therefore be closed immediately, and
each later spawn can perform a fresh transfer.

Immediately before `CreateProcessW`, the crate creates only the inheritable
duplicates required for standard I/O and argument or environment handoff. It
lists exactly those values in `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` and closes
the temporary duplicates as soon as process creation returns. The source
handle is never made inheritable in place, and there is no public escape hatch
for retaining an arbitrary inheritable duplicate.

Handle-handoff values form an application protocol:

- `arg_handle` appends a decimal value to the child's command line;
  `env_handle` writes one into an environment variable.
- The receiving program must parse the value and adopt or borrow it according
  to that protocol.
- A child-visible numeric value belongs to the child's handle table and must
  not be assumed to match the source process's value.
- With an alternate parent, the resource is duplicated into the effective
  parent's handle table before the child-visible value is lowered.

Windows retains a process-wide reverse race: while the short-lived inheritable
duplicates exist, unrelated code in the same source process that performs
broad handle inheritance can receive one. Avoid concurrent broad-inheritance
spawns when the handles are sensitive. A helper process could close the race,
but would change parent identity and the failure model; version 0.1 deliberately
does not use one. See
[ADR 0005](https://github.com/P4suta/windows-spawn/blob/main/docs/adr/0005-handle-transfer-and-reverse-race.md).

[`SpawnOptions`] borrows one-spawn capabilities such as Jobs, an alternate
parent, or a pseudoconsole. A borrowed `ConPTY` remains owned by the terminal
library implementing [`AsPseudoConsole`]. That library defines when terminal
pipes close and when terminal EOF occurs.

# Drop, wait, and EOF contract

- Dropping a normal [`Child`] detaches by default, matching
  [`std::process::Child`].
- [`DropPolicy::KillTree`] owns a private innermost Job. Dropping the child
  terminates the root and its descendants.
- [`Child::wait_with_output`] drains stdout and stderr concurrently. With
  `KillTree`, it terminates remaining descendants after the root exits before
  joining the readers. This guarantees pipe EOF even when a grandchild
  inherited a writer.
- Dropping [`SuspendedChild`] before [`SuspendedChild::resume`] terminates the
  suspended process. Its ID, process handle, and primary-thread handle are
  available before resume. `resume(self)` is consuming, so a second transition
  is unrepresentable.

# Transaction and security boundary

Process creation is split into a pure validation plan and an owning
transaction. The transaction owns pipes, temporary duplicates, attributes,
Jobs, and process/thread handles until success commits exactly the durable
resources to [`Child`] or [`SuspendedChild`]. Every error path rolls back the
rest.

This crate is not a sandbox, cross-platform process facade, async runtime, or
process supervisor. Tokens, ACLs, `AppContainer`, LPAC, capability SIDs, and
async supervision are outside its scope. Callers building an isolation boundary
must supply and audit those controls separately.

# Further reading

- [Compile-checked examples](https://github.com/P4suta/windows-spawn/tree/main/examples)
- [Architecture decision records](https://github.com/P4suta/windows-spawn/tree/main/docs/adr)
- [Security policy](https://github.com/P4suta/windows-spawn/security/policy)
