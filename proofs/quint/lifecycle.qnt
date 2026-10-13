// -*- mode: Bluespec; -*-
// SPDX-FileCopyrightText: 2026 Yasunobu Sakashita
// SPDX-License-Identifier: MIT OR Apache-2.0

/// The lifecycle of one spawned process through the public API of windows-spawn:
/// `spawn_suspended` and `spawn_with`, `SuspendedChild::resume` and its drop,
/// `Child::wait`, `try_wait`, `kill`, and its drop under each `DropPolicy`.
///
/// Each action is one API call or one event of the process, and `outcome` is what the call returned.
/// `tests/lifecycle_traces.rs` replays traces of this model against the crate on Windows,
/// and after every step requires the call's result and the process's real state to match.
module lifecycle {
  /// The exit codes the probe process chooses for itself when the test lets it go.
  pure val CODES = Set(0, 3, 77)

  /// The exit code `TerminateProcess` gets from `Child::kill` and from dropping a `SuspendedChild`.
  pure val TERMINATED = 1

  /// The exit code another process gives the probe when it terminates the probe from outside the crate.
  pure val EXTERNAL = 9

  /// The most extra suspensions a trace stacks on the primary thread.
  pure val EXTRA_SUSPENSIONS = 2

  type Policy = Detach | KillTree

  /// Which wrapper owns the process handle.
  type Owner = Suspended | Owned | Dropped

  /// The process as Windows sees it; `Killed` ends with a code the probe did not choose.
  type Process = Paused | Running | Exited(int) | Killed

  /// What `Child` remembers of the exit status.
  type Cache = Unseen | Seen(int)

  /// What the last call returned.
  type Outcome =
    | Started
    | Done
    | Resumed
    | InvalidData
    | Status(int)
    | NoStatus
    | Denied

  var policy: Policy
  var owner: Owner
  var process: Process
  var suspensions: int
  var cache: Cache
  var outcome: Outcome

  /// The exit code of a process that has exited, as `GetExitCodeProcess` reports it.
  pure def code(p: Process): int = match p {
    | Exited(c) => c
    | _ => -1
  }

  pure def exited(p: Process): bool = match p {
    | Exited(_) => true
    | Killed => true
    | _ => false
  }

  action init = {
    nondet chosen = Set(Detach, KillTree).oneOf()
    nondet suspended = Set(true, false).oneOf()
    all {
      policy' = chosen,
      owner' = if (suspended) Suspended else Owned,
      process' = if (suspended) Paused else Running,
      suspensions' = if (suspended) 1 else 0,
      cache' = Unseen,
      outcome' = Started,
    }
  }

  /// A test suspends the primary thread of a live process once more through `primary_thread_handle`.
  action suspendAgain = all {
    owner == Suspended,
    process == Paused,
    suspensions <= EXTRA_SUSPENSIONS,
    suspensions' = suspensions + 1,
    outcome' = Done,
    policy' = policy,
    owner' = owner,
    process' = process,
    cache' = cache,
  }

  /// `resume` runs the process only when the previous suspend count was one; otherwise it fails,
  /// and dropping the consumed `SuspendedChild` terminates the process.
  /// The primary thread of a process terminated from outside reads a suspend count of zero, so `resume` refuses it,
  /// and the process keeps the exit it had.
  action resume = all {
    owner == Suspended,
    if (process == Paused and suspensions == 1) all {
      owner' = Owned,
      process' = Running,
      suspensions' = 0,
      outcome' = Resumed,
    } else all {
      owner' = Dropped,
      process' = if (process == Paused) Exited(TERMINATED) else process,
      suspensions' = if (process == Paused) suspensions - 1 else 0,
      outcome' = InvalidData,
    },
    policy' = policy,
    cache' = cache,
  }

  /// Dropping a `SuspendedChild` terminates the process before it runs, unless it has already exited.
  action dropSuspended = all {
    owner == Suspended,
    owner' = Dropped,
    process' = if (process == Paused) Exited(TERMINATED) else process,
    outcome' = Done,
    policy' = policy,
    suspensions' = suspensions,
    cache' = cache,
  }

  /// Another process terminates the probe, paused or running, through a handle of its own.
  action externalKill = all {
    process == Paused or process == Running,
    process' = Exited(EXTERNAL),
    outcome' = Done,
    policy' = policy,
    owner' = owner,
    suspensions' = suspensions,
    cache' = cache,
  }

  /// The test lets the probe exit by itself with one of its codes.
  action exits = all {
    process == Running,
    nondet chosen = CODES.oneOf()
    process' = Exited(chosen),
    outcome' = Done,
    policy' = policy,
    owner' = owner,
    suspensions' = suspensions,
    cache' = cache,
  }

  /// `wait` returns the exit status and caches it; a trace waits only for a process that has exited.
  action wait = all {
    owner == Owned,
    exited(process),
    val status = match cache {
      | Seen(c) => c
      | Unseen => code(process)
    }
    all {
      outcome' = Status(status),
      cache' = Seen(status),
    },
    policy' = policy,
    owner' = owner,
    process' = process,
    suspensions' = suspensions,
  }

  /// `try_wait` returns the cached status, the status of an exited process, or none.
  action tryWait = all {
    owner == Owned,
    match cache {
      | Seen(c) => all { outcome' = Status(c), cache' = cache }
      | Unseen =>
        if (exited(process)) all { outcome' = Status(code(process)), cache' = Seen(code(process)) }
        else all { outcome' = NoStatus, cache' = cache }
    },
    policy' = policy,
    owner' = owner,
    process' = process,
    suspensions' = suspensions,
  }

  /// `kill` terminates a running process, does nothing after an observed exit,
  /// and returns the error of `TerminateProcess` for an exit nobody observed.
  action kill = all {
    owner == Owned,
    match cache {
      | Seen(_) => all { outcome' = Done, process' = process }
      | Unseen =>
        if (process == Running) all { outcome' = Done, process' = Exited(TERMINATED) }
        else all { outcome' = Denied, process' = process }
    },
    policy' = policy,
    owner' = owner,
    suspensions' = suspensions,
    cache' = cache,
  }

  /// Dropping a `Child` detaches by default; under `KillTree` its private Job ends the process.
  action dropChild = all {
    owner == Owned,
    owner' = Dropped,
    process' = if (process == Running and policy == KillTree) Killed else process,
    outcome' = Done,
    policy' = policy,
    suspensions' = suspensions,
    cache' = cache,
  }

  action step = any {
    suspendAgain,
    resume,
    dropSuspended,
    exits,
    wait,
    tryWait,
    kill,
    dropChild,
    externalKill,
  }

  /// A `SuspendedChild` owns a process whose primary thread has not run, unless another process terminated it.
  val suspendedIsPaused = owner == Suspended implies (process == Paused or process == Exited(EXTERNAL))

  /// The cached status is the status Windows reports.
  val cacheIsTrue = match cache {
    | Seen(c) => process == Exited(c)
    | Unseen => true
  }

  /// A refused resume leaves no process running.
  val refusedResumeTerminates = outcome == InvalidData implies exited(process)

  /// A dropped `KillTree` child leaves no process running.
  val killTreeLeavesNothing =
    (owner == Dropped and policy == KillTree) implies exited(process)

  /// `try_wait` and `wait` report a status only once the process has exited.
  val statusOnlyAfterExit = match outcome {
    | Status(c) => process == Exited(c)
    | _ => true
  }

  val invariant = and {
    suspendedIsPaused,
    cacheIsTrue,
    refusedResumeTerminates,
    killTreeLeavesNothing,
    statusOnlyAfterExit,
    suspensions >= 0,
  }

  /// Every exit code a trace can produce.
  pure val ALL_CODES = CODES.union(Set(TERMINATED, EXTERNAL))

  /// The values each variable can take.
  val typeOK = and {
    policy.in(Set(Detach, KillTree)),
    owner.in(Set(Suspended, Owned, Dropped)),
    process.in(Set(Paused, Running, Killed).union(ALL_CODES.map(c => Exited(c)))),
    suspensions.in(0.to(EXTRA_SUSPENSIONS + 1)),
    cache.in(Set(Unseen).union(ALL_CODES.map(c => Seen(c)))),
    outcome.in(
      Set(Started, Done, Resumed, InvalidData, NoStatus, Denied).union(ALL_CODES.map(c => Status(c)))
    ),
  }

  /// An invariant that every step preserves from any state that satisfies it, so it holds after any number of steps.
  val inductive = and {
    typeOK,
    invariant,
    owner == Suspended implies suspensions >= 1,
    owner == Owned implies suspensions == 0,
    process == Paused implies owner == Suspended,
    process == Killed implies (owner == Dropped and policy == KillTree),
  }

  /// A false claim the bounded check must refute: `kill` fails after an exit nobody observed.
  val killNeverFails = outcome != Denied

  /// Traces reach every outcome; `quint run --witnesses` counts the traces that do.
  val reachesRefusedResume = outcome == InvalidData
  val reachesDeniedKill = outcome == Denied
  val reachesCachedStatus = match outcome {
    | Status(c) => cache == Seen(c)
    | _ => false
  }
  val reachesKilledTree = process == Killed
  val reachesRefusedResumeAfterOutsideKill = outcome == InvalidData and process == Exited(EXTERNAL)
}
