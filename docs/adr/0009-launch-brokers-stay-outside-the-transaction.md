# 0009 — Launch brokers stay outside the transaction

Status: accepted (2026-09-25)

## Context

Some hosts put a session in a Job that ends with the session.
Windows OpenSSH does this; breakaway was refused in the setup that prompted this record.
A process that must outlive the session has three ways out:

- `CreationFlags::BREAKAWAY_FROM_JOB`.
  The child leaves its immediate Job only if that Job allows breakaway, then each enclosing Job up to the first that does not.
- `ParentProcess`.
  The child inherits the Job, token, handles, and quotas of the chosen parent.
  It needs `PROCESS_CREATE_PROCESS` on a process outside the Job.
- A launch broker, such as WMI `Win32_Process.Create`, the Task Scheduler, or a service.

A broker takes a command line and returns at most a process ID.
It cannot take a handle list, standard handles, attributes, an environment block, or a Job, and the broker becomes the parent.
Nothing the crate owns can roll back a failed broker launch.

## Decision

The crate does not drive brokers, for the reasons ADR 0005 rejects a helper process.

`Command::to_command_line` renders the command line.
The first token is the quoted absolute path of the executable a spawn would run, so a broker cannot resolve a different program.
Arguments are encoded as for a spawn.
Handle arguments and a `PATH` set from a handle are rejected; handle values exist only inside a spawn.

## Consequences

- Callers own the broker contract: opening the process by ID, the environment it supplies, and cleanup on failure.
- The rendered line is tested by launching it with a null application name and comparing the decoded arguments.
- Breakaway and `ParentProcess` remain subject to the host's Jobs.
