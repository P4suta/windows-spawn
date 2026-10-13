# Migrating from 0.1 to 0.2

0.2 keeps the API of `std::process::Command` and moves every Windows choice to the type that owns it.
Most programs change in three places: the error type, the tree policy, and the creation flags.
[ADR 0015](adr/0015-the-api-follows-std-and-ownership.md) records why.

0.2 requires Rust 1.84, where 0.1 required Rust 1.75; [ADR 0018](adr/0018-lint-exceptions-expire-and-the-msrv-is-rust-1-84.md) records why.

## Errors

Fallible calls return `windows_spawn::Result<T>`, whose error is `SpawnError`, instead of `std::io::Result<T>`.

- `?` still works in a function that returns `io::Result`, because `SpawnError` converts into `io::Error`.
  A Windows failure converts into the `io::Error` Windows reported, so `kind()` and `raw_os_error()` keep their values.
- `SpawnError::kind()` and `SpawnError::raw_os_error()` answer as they do for an `io::Error`.
- Match the category instead of the kind where it matters:
  `SpawnError::Validation(ValidationError::BatchFile)` for a rejected batch file,
  `SpawnError::Windows(error)` with `error.operation()` and `error.phase()` for a failed Windows call,
  and `SpawnError::Cleanup(error)` for failed releases.
- A suspend count other than one on resume is a Windows error of `Operation::ResumeThread` with kind `InvalidData`.

## Process trees

| 0.1 | 0.2 |
| --- | --- |
| `SpawnOptions::new().drop_policy(DropPolicy::KillTree)` | `Command::own_process_tree()` |
| `DropPolicy::Detach`, the default | no call, the default |
| `Child::kill` ends the root only | `Child::kill` ends the whole owned tree |
| `Child::kill` after an exit nobody observed returns `ERROR_ACCESS_DENIED` | `Child::kill` returns `Ok`, as std does |
| no way to wait for descendants | `Child::wait_tree()` |
| `Job::set_kill_on_close(bool)` | unchanged |
| `Job::assign(&Child)` | `Job::assign(&P)` for any `P: AsHandle`, so existing calls compile |

## Creation flags and the console

The setters that borrow nothing moved from `SpawnOptions` to `Command`, as `creation_flags` lives on std's `CommandExt`.

| 0.1 | 0.2 |
| --- | --- |
| `SpawnOptions::creation_flags(CreationFlags::NEW_PROCESS_GROUP)` | `Command::new_process_group()` |
| `CreationFlags::INHERIT_PARENT_AFFINITY` | `Command::inherit_parent_affinity()` |
| `CreationFlags::BREAKAWAY_FROM_JOB` | `Command::breakaway_from_job()` |
| `CreationFlags::PRESERVE_CODE_AUTHZ_LEVEL` | `Command::preserve_code_authorization()` |
| `CreationFlags::DEFAULT_ERROR_MODE` | `Command::default_error_mode()` |
| `CreationFlags::DETACHED_PROCESS` | `Command::console(ConsoleMode::NoConsole)` |
| `CreationFlags::NEW_CONSOLE` | `Command::console(ConsoleMode::NewConsole)` |
| `CreationFlags::NO_WINDOW` | `Command::console(ConsoleMode::NoWindow)` |
| none | `Command::priority(PriorityClass::BelowNormal)` and the other classes |
| `SpawnOptions::mitigation(policy)` | `Command::mitigation(policy)` |

Conflicting console flags can't be stated any more, because `console` takes one mode.
A console mode other than `ConsoleMode::Inherit` beside a pseudoconsole fails validation with `ValidationError::PseudoConsoleWithConsoleMode`.

## Mitigation policies

| 0.1 | 0.2 |
| --- | --- |
| `MitigationPolicy::dep(true)` | `MitigationPolicy::dep(Dep::Enable)` |
| `MitigationPolicy::dep_atl_thunk(true)` | `MitigationPolicy::dep_atl_thunk(AtlThunkEmulation::Enable)` |
| `MitigationPolicy::sehop(true)` | `MitigationPolicy::sehop(Sehop::Enable)` |
| `MitigationPolicy::font_disable(FontDisable::Block)` | `MitigationPolicy::non_system_fonts(NonSystemFonts::Block)` |

## Pseudoconsoles

| 0.1 | 0.2 |
| --- | --- |
| `SpawnOptions::pseudoconsole(&owner)` | `SpawnOptions::pseudo_console(&owner)` |
| `unsafe impl AsPseudoConsole` with `raw_pseudoconsole() -> isize` | `impl AsPseudoConsole` with `as_pseudo_console() -> BorrowedPseudoConsole<'_>` |
| none | `unsafe { BorrowedPseudoConsole::borrow_raw(hpcon) }` for a raw `HPCON` |

A terminal library that implemented the trait moves its unsafe block into the method:

```rust,ignore
impl AsPseudoConsole for MyPseudoConsole {
    fn as_pseudo_console(&self) -> BorrowedPseudoConsole<'_> {
        // SAFETY: `self` owns the HPCON until it drops.
        unsafe { BorrowedPseudoConsole::borrow_raw(self.hpcon) }
    }
}
```

## Other renames and additions

- `SuspendedChild::primary_thread_handle` is `main_thread_handle`, the name std uses.
- `ChildStdin`, `ChildStdout`, and `ChildStderr` convert into `Stdio` and into `OwnedHandle`, and `&ChildStdin` implements `Write`.
- `Stdio` converts from `io::stdout()` and `io::stderr()`.
- `Child`, `SuspendedChild`, the pipe ends, `Job`, and `ParentProcess` implement `AsRawHandle`; `Job` and `ParentProcess` convert into `OwnedHandle`.
- `Debug` prints a `Command` as std does; `{:#?}` prints every setting.
- A command line longer than `CreateProcessW` accepts fails with `ValidationError::CommandLineTooLong`, and `CreateProcessW` never runs.
  A spawn finds the length once it has lowered the line's handles, so it has acquired its pipes and duplicates by then and releases them.
  `Command::to_command_line` acquires nothing before it rejects the line.

## Behavior

Every spawn now creates its process suspended and resumes the main thread only after it has released the attribute list and the transferred duplicates.
A release that fails terminates the process before it runs any code and returns `SpawnError::Cleanup`.
