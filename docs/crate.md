# Windows process creation with explicit ownership

`windows-spawn` runs a complete `CreateProcessW` transaction with explicit handle transfer, ordered Job attachment, typed mitigation policies, `ConPTY`, and suspended inspection.
Use [`std::process::Command`] for portable child processes.

Non-Windows targets expose no API; they support dependency-graph checks only.

# Platform contract

Process creation and `ConPTY` require Windows 10 version 1809 or later.
Some [`MitigationPolicy`] fields need a newer Windows, a specific architecture, hardware support, or compatible executable metadata; CET policies are an example, and pointer authentication is ARM64-only.

Requested mitigations are not preflighted or weakened.
If Windows rejects a combination, spawning returns the `CreateProcessW` error.
Nested Jobs remain subject to Jobs the host imposes.

# Commands and executable lookup

- `.bat` and `.cmd` programs are rejected; invoke `cmd.exe` explicitly for a shell.
- Lookup follows Rust's safe Windows search, skips the current directory, and passes the resolved path as `lpApplicationName`.
- [`Command::raw_arg`] appends already-encoded command-line text.
  It invokes no shell; the text must suit the target's parser.
- Raw attributes, raw creation flags, and raw mitigation values are not exposed.

# Handle and capability ownership

[`Command`] stores reusable launch intent.
[`Command::arg_handle`] and [`Command::env_handle`] keep a private, non-inheritable duplicate, so the source may be closed; each spawn transfers a new duplicate.

Just before `CreateProcessW`, the crate creates inheritable duplicates only for standard I/O and handle handoff.
It lists exactly those in `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` and closes them when creation returns.
Source handles are never made inheritable in place, and no API retains an inheritable duplicate.

Handle handoff is an application protocol:

- `arg_handle` appends a decimal value to the command line; `env_handle` stores one in an environment variable.
- The child parses the value and adopts or borrows the handle as its protocol defines.
- The value belongs to the child's handle table, not the source process's.
- With an alternate parent, the handle is duplicated into that parent's table before the value is lowered.

Windows keeps a process-wide reverse race: an unrelated broad-inheritance spawn can receive a short-lived inheritable duplicate.
Avoid concurrent broad inheritance when transferred handles are sensitive.
The crate uses no helper process, which would change parent identity and failure semantics; see [ADR 0005](https://github.com/P4suta/windows-spawn/blob/main/docs/adr/0005-handle-transfer-and-reverse-race.md).

[`SpawnOptions`] borrows one-spawn capabilities: Jobs, an alternate parent, a pseudoconsole.
Jobs are attached during creation, before the child runs any code; a [`SuspendedChild`] is already in every requested Job.
A borrowed `ConPTY` stays owned by the library implementing [`AsPseudoConsole`], which defines when terminal pipes close and EOF occurs.
With a pseudoconsole, creation sets `STARTF_USESTDHANDLES` with all three standard handles null and lists none of them, so the child cannot fall back to standard handles the parent redirected.

# Drop, wait, and EOF contract

- Dropping a [`Child`] detaches by default, like [`std::process::Child`].
- [`DropPolicy::KillTree`] adds a private innermost Job; dropping the child terminates the root and its descendants.
- [`Child::wait_with_output`] drains stdout and stderr concurrently.
  With `KillTree`, it terminates remaining descendants after the root exits, so pipes reach EOF even if a descendant holds a writer.
  Both readers are joined even if one fails or panics.
- Dropping a [`SuspendedChild`] before [`SuspendedChild::resume`] terminates the process.
  The ID, process handle, and primary-thread handle are available before resume.
  `resume(self)` consumes the value, so a second resume does not compile.
  It requires a previous suspend count of exactly one; otherwise it fails and rolls back.
- [`Child`] and [`SuspendedChild`] expose their process handle through `AsHandle`.
  It has full access, including `SYNCHRONIZE`, and stays valid while the value lives, so it can be waited on with other objects.
  The crate has no timed waits.

# Launch brokers

Some launchers accept only a command line, such as WMI `Win32_Process.Create`.
[`Command::to_command_line`] renders one whose first token is the absolute path of the executable a spawn would run.
The crate does not drive brokers; see [ADR 0009](https://github.com/P4suta/windows-spawn/blob/main/docs/adr/0009-launch-brokers-stay-outside-the-transaction.md).

# Transaction and security boundary

A validation plan and an owning transaction create each process.
The transaction owns pipes, temporary duplicates, attributes, Jobs, and process and thread handles.
On success, durable resources move to [`Child`] or [`SuspendedChild`]; on error, everything rolls back.
The plan and transaction are typed by running or suspended state, so a mismatched commit does not compile.

The crate is not a sandbox, a cross-platform facade, an async runtime, or a supervisor.
Tokens, ACLs, `AppContainer`, LPAC, capability SIDs, and async supervision are out of scope; callers building an isolation boundary must supply and audit them.

# Further reading

- [Examples](https://github.com/P4suta/windows-spawn/tree/main/examples)
- [Architecture decisions](https://github.com/P4suta/windows-spawn/tree/main/docs/adr)
- [Security policy](https://github.com/P4suta/windows-spawn/security/policy)
