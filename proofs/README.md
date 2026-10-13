# Formal verification

Each layer checks the decisions it fits, and ties what it proves to the crate's code, as ADR 0014 records.
`cargo xtask proofs` runs every layer, and `cargo xtask proofs LAYER…` runs some of them.
Each layer runs a pinned tool, accepts only the complete expected result, and runs a negative control that the tool must refuse.

| Layer | Tool | What it proves | How it reaches the code |
| --- | --- | --- | --- |
| `kani` | Kani 0.68.0 | The decision cores in `src/domain` and the mitigation layout, for every input within each harness's bounds | It runs the crate's own functions |
| `lean` | Lean 4.35.0 | Both C runtime parsers read back every command line the encoder writes, for arguments of every length | Vectors from the proved model, which the Rust tests and the Windows parsers check |
| `verus` | Verus 0.2026.10.04 | The encoder writes what the Lean model writes, for every argument | A copy that must equal the crate's functions token for token |
| `tla` | `TLC` 1.7.4 | `Child::wait_with_output` returns with every byte, and each of two plausible mistakes deadlocks or never returns | A model of the protocol |
| `quint` | Quint 0.33.0 with Apalache 0.62.4 | The child lifecycle keeps its invariant after any number of steps | Seeded traces that `tests/lifecycle` replays through the crate on Windows |
| `alloy` | Alloy 6.2.0 | The handle transfer gives the child exactly the requested objects and leaves no duplicate behind, and the design needs each of its decisions | A model of `HandleTransfer` |
| `prolog` | `swipl` | The program search of Rust's standard library, stated as relations apart from the code | Vectors that a Windows unit test runs through the crate's search |

## Kani

The harnesses sit beside the functions they prove, under `cfg(kani)`.
`xtask/src/proofs/kani.rs` lists every proof with the number of `kani::cover!` points it must reach, and every negative control with the assertion it must fail.
`#[kani::unwind]` bounds each loop over a symbolic length, and an unwinding assertion fails the proof when a bound falls short.

## Lean

`lean/WindowsSpawn/CommandLine.lean` models `append_program`, `append_argument`, and the two C runtime parsers.
`split_encodeLine` proves that both parsers return the program and the arguments the encoder took.
The proofs rest on `propext`, `Classical.choice`, and `Quot.sound` alone.
The audit lists every theorem with its axioms, and refuses `Canary.lean`, which uses `sorry`, `native_decide`, and an axiom of its own.
`leanchecker` and nanoda, a kernel independent of Lean's, replay the library.
The Lean 4.35.0 toolchain that `lean/lean-toolchain` pins ships `leanchecker`, `leanexport`, and `nanoda_bin`, so `elan` installs every tool the gate runs.
`lake exe oracle` writes the vectors in `lean/vectors`.
The Rust tests compare the encoder and the parser models with them.
On Windows, `tests/command_line_parsers.rs` splits every parsing row with `CommandLineToArgvW`.
It also splits every third row in a child process with the Universal C runtime, `msvcrt.dll`, `CommandLineToArgvW`, and `std`.
Then it sends 400 seeded random command lines over every unit but NUL through the crate's encoder into such children.
Each child must read every argument back, which tests the models' assumption that the parsers copy every unit they give no meaning.
`CommandLineToArgvW` and `msvcrt.dll` read two double quotes inside a quoted region as the C runtime did before Visual C++ 2008, and the theorem covers both readings.

## Verus

`verus/command_line.rs` holds the encoder's four functions with Verus specifications.
It proves that `append_argument` writes `encode`, a transcription of the Lean model's `encodeArg`.
The gate removes the Verus attributes, the `verus!` blocks, and the proof blocks, and requires the remaining tokens of each function to equal the crate's.
Its negative control drops the doubling of the backslashes before a closing quote, which Verus must refuse.
The release needs the Rust toolchain its `version.json` names, and the gate installs it with `rustup`.

## Output draining

`tla/OutputDrain.tla` models `wait_with_output` with a child that writes more than a pipe holds.
It covers a child that owns its tree and one that doesn't, and a descendant that may keep a writer open.
`OutputDrain.cfg` proves that every supported case returns with every chunk.
`OutputDrainWaitFirst.cfg` must deadlock, and `OutputDrainUnowned.cfg` must violate the liveness property.

## Child lifecycle

`quint/lifecycle.qnt` models a spawned process through `spawn_suspended`, `resume`, `wait`, `try_wait`, `kill`, and the drops, with and without `Command::own_process_tree`.
It also injects a fault from outside the crate: another process terminates the probe, paused or running, at any step.
Apalache proves that the invariant `inductive` holds at the start and after every step, and refutes `droppingEndsTheProcess`.
`cargo xtask proof-traces` regenerates `quint/traces/lifecycle.tsv` from seeded simulations, and the gate requires the held table to equal them.
On Windows, `tests/lifecycle` replays every trace through the public API.
After each step it compares the call's result and the process's state with the model, and observes the process through handles the crate doesn't own.
The replay also runs four contradicting traces, which it must reject.
After an exit that no call observed, `kill` returns `Ok`, as `std::process::Child::kill` does, and the model and the replay hold that behavior.

## Handle transfer

`alloy/handle_transfer.als` models `HandleTransfer`.
It covers the duplicates in the process the child inherits from, `PROC_THREAD_ATTRIBUTE_HANDLE_LIST`, the release of every duplicate when the spawn ends or fails, and the resume of the main thread.
Its checks hold for every configuration of four atoms and eight steps, including that the child runs no code before every duplicate is released.
Four negative controls each drop one decision, and the solver must find the defect.
One inherits without the list, one duplicates into the creator while the spawn names a parent, one keeps the parent's duplicates, and one resumes the child before the release.

## Program search

`prolog/executable_search.pl` states the search of `std::process::Command` on Windows from its documentation and source.
`prolog/oracle.pl` writes `prolog/vectors/executable_search.tsv` over a grid of programs, `PATH` values, and existing files.
It also requires each of eight mistaken searches to change some row.
On Windows, the unit test `executable_search_follows_the_prolog_model` runs the crate's search on every row with a stand-in file system.

## Checks that reach the system

The proofs hold for the code and its models, and these Windows checks tie them to the system the crate calls.

- The library tests fail each Win32 call of twelve operations in turn, and require the same error back with no handle left behind, as ADR 0012 records.
- `tests/argv_roundtrip.rs` sends 150 seeded random environments through the crate's block builder into children, which must hold exactly the variables their commands set.
- `cargo xtask verifier` runs every Windows test under `appverif.exe`, which stops a process on handle misuse, heap corruption, lock misuse, or a leak.
  Three tests misuse a handle on purpose and must stop.
  CI runs it on a hosted Windows runner, which ships `appverif.exe`.

## Running the layers

The gates download `TLC`, Alloy, and Verus into `target/proofs/tools`, and check each one's SHA-256.
The layers also need Kani, Lean through `elan`, Java, Node.js with npm, and `swipl`.
The tests that read the vectors and traces compile with `--cfg windows_spawn_vectors`, which `cargo xtask test` passes, because the vectors stay outside the package.
