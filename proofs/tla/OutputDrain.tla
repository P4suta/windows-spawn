--------------------------- MODULE OutputDrain ---------------------------
(***************************************************************************)
(* `Child::wait_with_output` in src/child.rs, against a child that writes  *)
(* more to each standard stream than a pipe holds, and a descendant that   *)
(* may keep the standard-output writer open.                               *)
(*                                                                         *)
(* The parent closes standard input, starts one reader for each output     *)
(* pipe, waits for the root to exit, terminates the private Job under      *)
(* `DropPolicy::KillTree`, and joins the readers.  A reader ends on EOF:   *)
(* an empty pipe that no process can still write to.                       *)
(*                                                                         *)
(* OutputDrain.cfg proves that the call returns with every chunk the child *)
(* wrote whenever the policy is `KillTree` or no descendant holds a        *)
(* writer.  The other two configurations are negative controls: waiting    *)
(* before reading deadlocks, and `Detach` with such a descendant never     *)
(* returns.                                                                *)
(***************************************************************************)
EXTENDS Naturals

CONSTANTS
    Chunks,      \* chunks the root writes to each output stream
    Capacity,    \* chunks a pipe holds before a write blocks
    Concurrent   \* TRUE: the readers start before the wait, as the crate does

Streams == {"stdout", "stderr"}

VARIABLES
    killTree,         \* the drop policy is KillTree
    descendant,       \* the root started a descendant that inherited the stdout writer
    stdinOpen,        \* the parent still holds the writer of the child's standard input
    written,          \* chunks the root wrote to each stream
    piped,            \* chunks waiting in each pipe
    collected,        \* chunks each reader read
    rootAlive,
    descendantAlive,
    readers,          \* each reader is "idle", "reading", or "done"
    parent            \* where wait_with_output stands

vars == <<killTree, descendant, stdinOpen, written, piped, collected,
          rootAlive, descendantAlive, readers, parent>>

Phases == {"start", "waiting", "terminating", "joining", "returned"}

TypeOK ==
    /\ killTree \in BOOLEAN
    /\ descendant \in BOOLEAN
    /\ stdinOpen \in BOOLEAN
    /\ written \in [Streams -> 0..Chunks]
    /\ piped \in [Streams -> 0..Capacity]
    /\ collected \in [Streams -> 0..Chunks]
    /\ rootAlive \in BOOLEAN
    /\ descendantAlive \in BOOLEAN
    /\ readers \in [Streams -> {"idle", "reading", "done"}]
    /\ parent \in Phases

\* A process can still write to the pipe of `s`.
HasWriter(s) == rootAlive \/ (s = "stdout" /\ descendantAlive)

Init ==
    /\ killTree \in BOOLEAN
    /\ descendant \in BOOLEAN
    /\ stdinOpen = TRUE
    /\ written = [s \in Streams |-> 0]
    /\ piped = [s \in Streams |-> 0]
    /\ collected = [s \in Streams |-> 0]
    /\ rootAlive = TRUE
    /\ descendantAlive = descendant
    /\ readers = [s \in Streams |-> "idle"]
    /\ parent = "start"

\* The root writes one chunk to `s` while the pipe has room; a full pipe blocks it.
RootWrite(s) ==
    /\ rootAlive
    /\ written[s] < Chunks
    /\ piped[s] < Capacity
    /\ written' = [written EXCEPT ![s] = @ + 1]
    /\ piped' = [piped EXCEPT ![s] = @ + 1]
    /\ UNCHANGED <<killTree, descendant, stdinOpen, collected, rootAlive,
                   descendantAlive, readers, parent>>

\* The root exits after its input reaches EOF and its output is written, which closes its writers.
RootExit ==
    /\ rootAlive
    /\ ~stdinOpen
    /\ \A s \in Streams : written[s] = Chunks
    /\ rootAlive' = FALSE
    /\ UNCHANGED <<killTree, descendant, stdinOpen, written, piped, collected,
                   descendantAlive, readers, parent>>

\* wait_with_output closes standard input and starts both readers before it waits.
StartReaders ==
    /\ parent = "start"
    /\ Concurrent
    /\ stdinOpen' = FALSE
    /\ readers' = [s \in Streams |-> "reading"]
    /\ parent' = "waiting"
    /\ UNCHANGED <<killTree, descendant, written, piped, collected, rootAlive,
                   descendantAlive>>

\* The order the crate avoids: close standard input and wait before any reader starts.
WaitFirst ==
    /\ parent = "start"
    /\ ~Concurrent
    /\ stdinOpen' = FALSE
    /\ parent' = "waiting"
    /\ UNCHANGED <<killTree, descendant, written, piped, collected, rootAlive,
                   descendantAlive, readers>>

\* WaitForSingleObject returns once the root exited; the naive order starts its readers only now.
Wait ==
    /\ parent = "waiting"
    /\ ~rootAlive
    /\ readers' = [s \in Streams |-> "reading"]
    /\ parent' = "terminating"
    /\ UNCHANGED <<killTree, descendant, stdinOpen, written, piped, collected,
                   rootAlive, descendantAlive>>

\* Under KillTree, terminating the private Job ends every descendant and closes its writers.
Terminate ==
    /\ parent = "terminating"
    /\ descendantAlive' = (descendantAlive /\ ~killTree)
    /\ parent' = "joining"
    /\ UNCHANGED <<killTree, descendant, stdinOpen, written, piped, collected,
                   rootAlive, readers>>

\* A reader takes one chunk out of its pipe.
Read(s) ==
    /\ readers[s] = "reading"
    /\ piped[s] > 0
    /\ piped' = [piped EXCEPT ![s] = @ - 1]
    /\ collected' = [collected EXCEPT ![s] = @ + 1]
    /\ UNCHANGED <<killTree, descendant, stdinOpen, written, rootAlive,
                   descendantAlive, readers, parent>>

\* A reader sees EOF once its pipe is empty and no process can write to it.
Eof(s) ==
    /\ readers[s] = "reading"
    /\ piped[s] = 0
    /\ ~HasWriter(s)
    /\ readers' = [readers EXCEPT ![s] = "done"]
    /\ UNCHANGED <<killTree, descendant, stdinOpen, written, piped, collected,
                   rootAlive, descendantAlive, parent>>

\* join_readers returns once both readers finished.
Join ==
    /\ parent = "joining"
    /\ \A s \in Streams : readers[s] = "done"
    /\ parent' = "returned"
    /\ UNCHANGED <<killTree, descendant, stdinOpen, written, piped, collected,
                   rootAlive, descendantAlive, readers>>

\* After the call returns nothing happens, which is not a deadlock.
Returned ==
    /\ parent = "returned"
    /\ UNCHANGED vars

Next ==
    \/ \E s \in Streams : RootWrite(s) \/ Read(s) \/ Eof(s)
    \/ RootExit \/ StartReaders \/ WaitFirst \/ Wait \/ Terminate \/ Join
    \/ Returned

\* Every runnable thread and process eventually takes its next step.
Fairness ==
    /\ \A s \in Streams : WF_vars(RootWrite(s)) /\ WF_vars(Read(s)) /\ WF_vars(Eof(s))
    /\ WF_vars(RootExit) /\ WF_vars(StartReaders) /\ WF_vars(WaitFirst)
    /\ WF_vars(Wait) /\ WF_vars(Terminate) /\ WF_vars(Join)

Spec == Init /\ [][Next]_vars /\ Fairness

\* The cases the crate promises to return from: KillTree, or no descendant holding a writer.
Supported == killTree \/ ~descendant

SupportedSpec == Init /\ Supported /\ [][Next]_vars /\ Fairness

\* Every chunk is written, waiting in its pipe, or read; none is lost or invented.
Conserved == \A s \in Streams : collected[s] + piped[s] = written[s]

\* A returned call holds every chunk the root wrote.
Complete == parent = "returned" => \A s \in Streams : collected[s] = Chunks

\* The call returns.
Returns == <>(parent = "returned")

=============================================================================
