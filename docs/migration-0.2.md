# Migrating to 0.2

Version 0.2 deliberately removes weak representations. Compiler errors identify
each call site that needs a decision.

| 0.1 API | 0.2 replacement |
| --- | --- |
| `DropPolicy::Detach` | `JobClosePolicy::PreserveProcesses` |
| `DropPolicy::KillTree` | `JobClosePolicy::TerminateProcesses` |
| `SpawnOptions::drop_policy` | `SpawnOptions::job_close_policy` |
| `CreationFlags::NEW_CONSOLE` | `SpawnOptions::terminal(TerminalMode::Console(ConsoleMode::NewConsole))` |
| Other supported creation bits | Dedicated `SpawnOptions` setters |
| `MitigationPolicy::dep(bool)` | `MitigationPolicy::dep(DepPolicy)` |
| `MitigationPolicy::dep_atl_thunk(bool)` | `MitigationPolicy::dep_atl_thunk(AtlThunkPolicy)` |
| `MitigationPolicy::sehop(bool)` | `MitigationPolicy::sehop(SehopPolicy)` |
| `AsPseudoConsole::raw_pseudoconsole` | `AsPseudoConsole::as_pseudo_console` returning `BorrowedPseudoConsole` |
| Public `io::Result<T>` | `windows_spawn::Result<T>` |

`BorrowedPseudoConsole::from_raw` is the sole raw pseudoconsole constructor. It
is unsafe and requires a borrow of the owner so the capability cannot outlive
that owner.

Ordinary spawn behavior remains available through `Command`, `Child`, and
`SuspendedChild`. The internal implementation always creates the process
suspended, reclaims temporary resources, and only then resumes it. Callers that
request suspension still receive a consuming `SuspendedChild::resume`
transition.
