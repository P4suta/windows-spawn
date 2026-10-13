// SPDX-FileCopyrightText: 2026 Yasunobu Sakashita
// SPDX-License-Identifier: MIT OR Apache-2.0

/*
 * How a spawn hands handles to its child: `HandleTransfer` in src/transaction.rs.
 *
 * Every handle a command transfers (a standard handle, an argument handle, or a variable handle) is a request.
 * The transfer duplicates each request's source into the process the child inherits from:
 * the creator itself, or the parent that `SpawnOptions::parent` names.
 * Each duplicate is inheritable, and its value goes onto the command line or into the environment.
 * CreateProcessW inherits handles only when the transfer made some, and then exactly the values of
 * PROC_THREAD_ATTRIBUTE_HANDLE_LIST, which lists every duplicate.
 * After CreateProcessW returns, or when the spawn fails before it, dropping the transfer closes every duplicate:
 * a local one directly, and a remote one by duplicating it back with DUPLICATE_CLOSE_SOURCE.
 * The child is created suspended, and its primary thread resumes only after the transfer is released.
 *
 * `Correct` is that design.
 * The other designs each drop one of its decisions, and the checks below require each of them to fail,
 * so the model shows that every decision matters:
 * `InheritsAll` sets bInheritHandles without the list, `LowersLocally` duplicates into the creator even when a
 * parent is named, `KeepsRemote` never closes the duplicates it made in the parent, and `ResumesFirst` lets the
 * child run before the transfer is released.
 */
module handle_transfer

/** A kernel object a handle refers to: a file, a pipe end, a device. */
sig Object {}

/** A handle value; each process maps the values it holds to objects, and two processes may hold the same value. */
sig Value {}

abstract sig Process {
  var table: Value -> lone Object,
  var inheritable: set Value,
}

/** The process that calls CreateProcessW. */
one sig Creator extends Process {}

/** The parent that PROC_THREAD_ATTRIBUTE_PARENT_PROCESS names, when the spawn names one. */
lone sig Parent extends Process {}

one sig Child extends Process {}

/** A handle the command transfers, held by the creator. */
sig Request {
  source: one Value,
}

abstract sig Design {}
one sig Correct, InheritsAll, LowersLocally, KeepsRemote, ResumesFirst extends Design {}

one sig Chosen {
  design: one Design,
}

abstract sig Phase {}
one sig Lowering, Created, Done extends Phase {}

one sig Spawn {
  var phase: one Phase,
  /** The duplicate each request got, whose value the child reads. */
  var lowered: Request -> lone Value,
  /** PROC_THREAD_ATTRIBUTE_HANDLE_LIST. */
  var listed: set Value,
  var born: lone Child,
  /** The child once its primary thread resumed. */
  var running: lone Child,
}

/** The tables before the spawn. */
one sig Initial {
  held: Process -> Value -> Object,
}

/** The process whose handles the child inherits. */
fun inheritor: Process {
  some Parent implies Parent else Creator
}

/** The process that receives the duplicates. */
fun receiver: Process {
  (some Parent and Chosen.design != LowersLocally) implies Parent else Creator
}

pred init {
  Spawn.phase = Lowering
  no Spawn.lowered
  no Spawn.listed
  no Spawn.born
  no Spawn.running
  no Child.table
  no Child.inheritable
  all p: Process | p.inheritable in p.table.Object
  Request.source in Creator.table.Object
  Initial.held = table
}

/** Duplicates one request's source, as `HandleTransfer::lower` does. */
pred lower[r: Request, v: Value] {
  Spawn.phase = Lowering
  no Spawn.lowered[r]
  v not in receiver.table.Object
  table' = table + receiver -> v -> Creator.table[r.source]
  inheritable' = inheritable + receiver -> v
  Spawn.lowered' = Spawn.lowered + r -> v
  Spawn.listed' = Spawn.listed + v
  Spawn.phase' = Spawn.phase
  Spawn.born' = Spawn.born
  Spawn.running' = Spawn.running
}

/** CreateProcessW with CREATE_SUSPENDED, which requires every listed value to name an inheritable handle of the inheritor. */
pred create {
  Spawn.phase = Lowering
  Request in Spawn.lowered.Value
  Spawn.listed in inheritor.inheritable
  Chosen.design = InheritsAll implies
    Child.table' = inheritor.inheritable <: inheritor.table
  else
    Child.table' = Spawn.listed <: inheritor.table
  all p: Process - Child | p.table' = p.table
  inheritable' = inheritable
  Spawn.lowered' = Spawn.lowered
  Spawn.listed' = Spawn.listed
  Spawn.phase' = Created
  Spawn.born' = Child
  Spawn.running' = Spawn.running
}

/** Drops the transfer after CreateProcessW returns, or when the spawn fails before it. */
pred release {
  Spawn.phase in Lowering + Created
  let made = Spawn.lowered[Request],
      closed = (Chosen.design = KeepsRemote and some Parent) implies none else made {
    table' = table - receiver -> closed -> Object
    inheritable' = inheritable - receiver -> closed
  }
  Spawn.lowered' = Spawn.lowered
  Spawn.listed' = Spawn.listed
  Spawn.phase' = Done
  Spawn.born' = Spawn.born
  Spawn.running' = Spawn.running
}

/** ResumeThread on the primary thread, which `Correct` calls only after the release. */
pred resume {
  some Spawn.born
  no Spawn.running
  Chosen.design = ResumesFirst implies Spawn.phase in Created + Done else Spawn.phase = Done
  table' = table
  inheritable' = inheritable
  Spawn.lowered' = Spawn.lowered
  Spawn.listed' = Spawn.listed
  Spawn.phase' = Spawn.phase
  Spawn.born' = Spawn.born
  Spawn.running' = Spawn.born
}

pred stutter {
  table' = table
  inheritable' = inheritable
  Spawn.lowered' = Spawn.lowered
  Spawn.listed' = Spawn.listed
  Spawn.phase' = Spawn.phase
  Spawn.born' = Spawn.born
  Spawn.running' = Spawn.running
}

fact traces {
  init
  always (stutter or create or release or resume or some r: Request, v: Value | lower[r, v])
}

/** The child holds exactly the transferred objects, each under the value its request wrote. */
pred inheritsExactly {
  always (some Spawn.born implies {
    all r: Request | Child.table[Spawn.lowered[r]] = Creator.table[r.source]
    Child.table.Object = Spawn.lowered[Request]
  })
}

/** Each request gets its own value, because a handle list must not name a value twice. */
pred listsEachValueOnce {
  always (all disj r, s: Request | no Spawn.lowered[r] & Spawn.lowered[s])
}

/** Once the spawn ends, every process but the child holds what it held before. */
pred leavesNoDuplicate {
  always (Spawn.phase = Done implies all p: Process - Child | p.table = Initial.held[p])
}

/** The child runs no code while a duplicate of the transfer is open, so a failed release can still end it unrun. */
pred runsOnlyAfterRelease {
  always (some Spawn.running implies {
    Spawn.phase = Done
    all p: Process - Child | p.table = Initial.held[p]
  })
}

check CorrectInheritsExactly {
  Chosen.design = Correct implies inheritsExactly
} for 4 but 8 steps expect 0

check CorrectListsEachValueOnce {
  Chosen.design = Correct implies listsEachValueOnce
} for 4 but 8 steps expect 0

check CorrectLeavesNoDuplicate {
  Chosen.design = Correct implies leavesNoDuplicate
} for 4 but 8 steps expect 0

check CorrectRunsOnlyAfterRelease {
  Chosen.design = Correct implies runsOnlyAfterRelease
} for 4 but 8 steps expect 0

/** A spawn through a parent that already holds an unrelated inheritable handle, with two requests of one source. */
run SpawnThroughABusyParent {
  Chosen.design = Correct
  some Parent
  #Request = 2
  one Request.source
  some Parent.inheritable
  eventually (some Spawn.born and Spawn.phase = Done)
} for 4 but 8 steps expect 1

/** A spawn that fails after it made a duplicate. */
run RollBackAfterADuplicate {
  Chosen.design = Correct
  some Request
  eventually (Spawn.phase = Done and no Spawn.born and some Spawn.lowered)
} for 4 but 8 steps expect 1

/** Without the list, the child also inherits the inheritor's unrelated inheritable handles. */
check NegativeControlInheritsAll {
  Chosen.design = InheritsAll implies inheritsExactly
} for 4 but 8 steps expect 1

/** Duplicated into the creator, a value can name another object in the parent, which the child then inherits. */
check NegativeControlLowersLocally {
  Chosen.design = LowersLocally implies inheritsExactly
} for 4 but 8 steps expect 1

/** Duplicates left in the parent outlive the spawn. */
check NegativeControlKeepsRemote {
  Chosen.design = KeepsRemote implies leavesNoDuplicate
} for 4 but 8 steps expect 1

/** Resumed before the release, the child runs while the duplicates are still open. */
check NegativeControlResumesFirst {
  Chosen.design = ResumesFirst implies runsOnlyAfterRelease
} for 4 but 8 steps expect 1
