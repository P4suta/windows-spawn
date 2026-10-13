# Windows process creation with explicit ownership

`windows-spawn` creates Windows processes with the API of [`std::process::Command`], and adds what `CreateProcessW` offers beyond it: process trees that end with their owner, Jobs, pseudoconsoles, handle handoff, suspended inspection, alternate parents, priority classes, and mitigation policies.
Each spawn is one transaction.
The child inherits only the handles the command names, it runs no code before the spawn has released its temporary resources, and a step that fails releases everything the spawn acquired.

Use [`std::process::Command`] for portable child processes.
Non-Windows targets expose no API; they support dependency-graph checks only.

# Getting started

[`Command`], [`Child`], and [`Stdio`] work like their `std::process` namesakes, so switching is mostly an import change:

```
use windows_spawn::Command;

let output = Command::new("cmd.exe").args(["/D", "/C", "echo hello"]).output()?;
assert!(output.status.success());
# Ok::<(), windows_spawn::SpawnError>(())
```

Fallible calls return [`SpawnError`], which keeps whether the input was rejected, which Windows operation failed, or which release failed.
It converts into [`std::io::Error`], so `?` works in a function that returns `io::Result`, and [`SpawnError::kind`] and [`SpawnError::raw_os_error`] answer as they do for an `io::Error`.

Where the crate differs from std:

- A `.bat` or `.cmd` program is rejected; [Run a batch script](#run-a-batch-script) shows how to name the shell instead.
- The child inherits its standard streams and the handles the command transfers, never every inheritable handle of the caller.
- [`Child::kill`] ends the whole process tree when the child owns it.

# Common tasks

## End a process tree with its owner

A build tool or a test runner that starts a child usually wants nothing the child starts to outlive it.
[`Command::own_process_tree`] puts the child in a private Job that the [`Child`] owns:
[`Child::kill`] then ends every process of the tree, dropping the `Child` ends them too, and [`Child::wait_tree`] waits until all of them have exited.

```
use windows_spawn::{Command, Stdio};

let mut task = Command::new("cmd.exe");
task.args(["/D", "/C", "set /p line="])
    .stdin(Stdio::piped())
    .own_process_tree();

let mut child = task.spawn()?;
child.kill()?;
child.wait_tree()?;
# Ok::<(), windows_spawn::SpawnError>(())
```

With an owned tree, [`Command::output`] also ends the descendants once the child exits, so a descendant that inherited an output pipe can't keep the call from returning.

## Run a console tool from a GUI program

A GUI program that runs a console tool would flash a console window.
[`ConsoleMode::NoWindow`] gives the tool a console without a window, which the tool's own console children share:

```
use windows_spawn::{Command, ConsoleMode};

let output = Command::new("cmd.exe")
    .args(["/D", "/C", "ver"])
    .console(ConsoleMode::NoWindow)
    .output()?;
assert!(output.status.success());
# Ok::<(), windows_spawn::SpawnError>(())
```

## Run a batch script

`cmd.exe` parses a batch file's arguments by rules that no quoting makes safe for arbitrary text, so the crate refuses to run a `.bat` or `.cmd` program with [`ValidationError::BatchFile`].
Name the shell instead, and pass the whole command line as one raw argument:

```no_run
use windows_spawn::Command;

let shell = std::env::var_os("COMSPEC").unwrap_or_else(|| "cmd.exe".into());
let status = Command::new(shell)
    .args(["/D", "/S", "/C"])
    .raw_arg(r#""npm.cmd install --no-fund""#)
    .status()?;
# Ok::<(), windows_spawn::SpawnError>(())
```

`/S` makes `cmd.exe` strip the outer quotes and run the rest as written.
That rest is `cmd.exe` syntax: `%`, `!`, `^`, `&`, `|`, `<`, `>`, and parentheses keep their meaning, so build it only from text you control.

## Connect two children

A child's [`ChildStdout`] converts into [`Stdio`], so it can feed another child's input, as the [`ChildStdout`] example shows.
`Stdio::from(std::io::stderr())` gives a child the caller's standard error as its output.

## Attach a pseudoconsole

A terminal emulator creates a pseudoconsole and attaches each child to it with [`SpawnOptions::pseudo_console`].
A terminal library implements [`AsPseudoConsole`] for its pseudoconsole type, so an application passes a reference.
An application that holds a raw `HPCON` borrows it with [`BorrowedPseudoConsole::borrow_raw`]:

```no_run
use windows_spawn::{BorrowedPseudoConsole, Command, SpawnOptions};

# let hpcon: isize = 0;
// SAFETY: `hpcon` came from CreatePseudoConsole and stays open while the spawn runs.
let console = unsafe { BorrowedPseudoConsole::borrow_raw(hpcon) };
let mut child = Command::new("cmd.exe").spawn_with(SpawnOptions::new().pseudo_console(console))?;
child.wait()?;
# Ok::<(), windows_spawn::SpawnError>(())
```

The pseudoconsole supplies the child's console and its standard streams, so the command chooses no console mode and configures no stream.

## Place children in a Job

[`SpawnOptions::job`] attaches the child to a [`Job`] during creation, before it runs any code; Jobs nest in call order.

```
use windows_spawn::{Command, Job, SpawnOptions};

let job = Job::create()?;
job.set_kill_on_close(true)?;
let mut child = Command::new("cmd.exe")
    .args(["/D", "/C", "exit 0"])
    .spawn_with(SpawnOptions::new().job(&job))?;
child.wait()?;
# Ok::<(), windows_spawn::SpawnError>(())
```

[`Job::assign`] adds a process that already runs.

## Hand a handle to a child

[`Command::arg_handle`] and [`Command::env_handle`] transfer a handle and pass its value in the child's handle table as an argument or an environment variable; the child's own protocol adopts it.
The `handle_handoff` example shows a whole round trip.

## Inspect a process before it runs

[`Command::spawn_suspended`] returns a [`SuspendedChild`] whose main thread hasn't run yet.

```
use windows_spawn::Command;

let suspended = Command::new("cmd.exe").args(["/D", "/C", "exit 0"]).spawn_suspended()?;
assert_ne!(suspended.id(), 0);
let mut child = suspended.resume()?;
child.wait()?;
# Ok::<(), windows_spawn::SpawnError>(())
```

## Spawn on behalf of another process

[`SpawnOptions::parent_process`] makes another process the child's logical parent, from which the child inherits.
That parent holds none of the caller's standard streams, so the command configures all three:

```no_run
use windows_spawn::{Command, ParentProcess, SpawnOptions, Stdio};

# let pid = 0;
let parent = ParentProcess::open(pid)?;
let child = Command::new("notepad.exe")
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .spawn_with(SpawnOptions::new().parent_process(&parent))?;
# Ok::<(), windows_spawn::SpawnError>(())
```

## Lower a child's priority

```
use windows_spawn::{Command, PriorityClass};

let status = Command::new("cmd.exe")
    .args(["/D", "/C", "exit 0"])
    .priority(PriorityClass::BelowNormal)
    .status()?;
assert!(status.success());
# Ok::<(), windows_spawn::SpawnError>(())
```

## Harden a child with mitigation policies

```
use windows_spawn::{Command, Mitigation, MitigationPolicy};

let policy = MitigationPolicy::new().disable_extension_points(Mitigation::AlwaysOn);
let status = Command::new("cmd.exe")
    .args(["/D", "/C", "exit 0"])
    .mitigation(policy)
    .status()?;
assert!(status.success());
# Ok::<(), windows_spawn::SpawnError>(())
```

## Render a command line for a launcher

[`Command::to_command_line`] renders the command line a spawn would pass, for a launcher that accepts only a command line, such as WMI `Win32_Process.Create`.

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
- A command line longer than `CreateProcessW` accepts is rejected with [`ValidationError::CommandLineTooLong`].
- Raw attributes, raw creation flags, and raw mitigation values are not exposed; each choice has its own setter.

# Handle and capability ownership

[`Command`] stores reusable launch intent, including every creation choice it owns.
[`Command::arg_handle`] and [`Command::env_handle`] keep a private, non-inheritable duplicate, so the source may be closed; each spawn transfers a new duplicate.

Just before `CreateProcessW`, the crate creates inheritable duplicates only for standard I/O and handle handoff.
It lists exactly those in `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` and closes them before the child runs.
Source handles are never made inheritable in place, and no API retains an inheritable duplicate.

Handle handoff is an application protocol:

- `arg_handle` appends a decimal value to the command line; `env_handle` stores one in an environment variable.
- The child parses the value and adopts or borrows the handle as its protocol defines.
- The value belongs to the child's handle table, not the source process's.
- With an alternate parent, the handle is duplicated into that parent's table before the value is lowered.

Windows keeps a process-wide reverse race: an unrelated broad-inheritance spawn can receive a short-lived inheritable duplicate.
Avoid concurrent broad inheritance when transferred handles are sensitive.
The crate uses no helper process, which would change parent identity and failure semantics; see [ADR 0005](https://github.com/P4suta/windows-spawn/blob/main/docs/adr/0005-handle-transfer-and-reverse-race.md).

[`SpawnOptions`] borrows one spawn's capabilities: Jobs, an alternate parent, a pseudoconsole.
Jobs are attached during creation, before the child runs any code; a [`SuspendedChild`] is already in every requested Job.
A borrowed pseudoconsole stays owned by its owner, which defines when terminal pipes close and EOF occurs.
With a pseudoconsole, creation sets `STARTF_USESTDHANDLES` with all three standard handles null and lists none of them, so the child cannot fall back to standard handles the parent redirected.

# Drop, wait, and EOF contract

- Every spawn creates its process suspended, releases the attribute list and the transferred duplicates, and only then resumes the main thread or returns a [`SuspendedChild`].
  A release that fails terminates the process before it runs and reports a [`CleanupError`].
- Dropping a [`Child`] leaves the process running, like [`std::process::Child`].
- [`Command::own_process_tree`] adds a private innermost Job that the `Child` owns.
  [`Child::kill`] and dropping the `Child` terminate the root and its descendants, and so does the exit of the process that holds the `Child`.
  The Job allows no breakaway.
- [`Child::kill`] returns `Ok` once the process has exited, as std does.
- [`Child::wait_tree`] waits for the child and every process of its owned tree; the waits end on process exits only.
- [`Child::wait_with_output`] drains stdout and stderr concurrently.
  With an owned tree, it terminates remaining descendants after the root exits, so pipes reach EOF even if a descendant holds a writer.
  Both readers are joined even if one fails or panics.
- Dropping a [`SuspendedChild`] before [`SuspendedChild::resume`] terminates the process.
  The ID, process handle, and main-thread handle are available before resume.
  `resume(self)` consumes the value, so a second resume does not compile.
  It requires a previous suspend count of exactly one; otherwise it fails and terminates the process.
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

# Verification

Kani proves the command-line encoder, the environment block, the creation flags, the standard I/O plan, the attribute-list size, and the mitigation layout.
Lean and Verus prove, for arguments of every length, that the C runtime parsers read back every command line the crate writes.
Models in `TLA+`, Quint, and Alloy check output draining, the child lifecycle, and the handle transfer, and Windows tests replay the models' traces through the crate.
See [the verification layers](https://github.com/P4suta/windows-spawn/blob/main/proofs/README.md).

# Further reading

- [Migrating from 0.1](https://github.com/P4suta/windows-spawn/blob/main/docs/migration-0.2.md)
- [Examples](https://github.com/P4suta/windows-spawn/tree/main/examples)
- [Architecture decisions](https://github.com/P4suta/windows-spawn/tree/main/docs/adr)
- [Verification layers](https://github.com/P4suta/windows-spawn/blob/main/proofs/README.md)
- [Security policy](https://github.com/P4suta/windows-spawn/security/policy)
