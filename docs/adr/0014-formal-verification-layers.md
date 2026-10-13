# 0014 — Formal verification runs in layers, each tied to the code

Status: accepted (2026-10-13)

## Context

Tests try chosen inputs, and the crate's riskiest decisions have input spaces no test covers.
Those spaces hold every argument a command line can carry, every environment block, every order of lifecycle calls, and every configuration of handles and processes.
No single verifier fits all those spaces.
A bounded model checker runs the real code but only within bounds.
A proof assistant covers every length but proves a model.
A protocol model checker explores interleavings but not code.
A proof also says nothing when it proves the wrong thing.
A vacuous property, a model that drifted from the code, and a tool that skipped a check without a word all pass.

## Decision

The tool that fits each decision verifies it, and every layer reaches the code through a mechanism that CI runs.

- The decisions that depend only on their inputs live in `src/domain`, apart from Win32, so verifiers off Windows reach the crate's own functions.
- Kani proves those functions for every input within each harness's bounds.
  `#[kani::unwind]` bounds each loop over a symbolic length, and an unwinding assertion fails the proof when a bound falls short.
- Lean proves, for arguments of every length, that both C runtime parsers read back what the encoder writes.
  The proofs rest on Lean's three standard axioms, and two kernels besides Lean's replay them.
  Vectors from the proved model tie it to the Rust encoder, to the parser models, and to the parsers Windows ships.
- Verus proves that the encoder's Rust code writes what the Lean model writes, for every argument.
  Without its Verus annotations, the verified copy must equal the crate's functions token for token.
- `TLA+` checks the output-draining protocol of `wait_with_output` for deadlock and liveness.
- Quint states the child lifecycle.
  Apalache proves its invariant inductive, and its seeded traces replay through the crate on Windows.
- Alloy checks the handle-transfer design in every small configuration.
- Prolog states the program search of Rust's standard library apart from the code, and its vectors run through the crate's search.

Every gate obeys the same rules.

- The gate pins the tool's version, and checks a downloaded tool's SHA-256.
- The gate accepts only the complete expected result: the inventory of proofs, theorems, commands, or rows, and the count each one must reach.
- Every layer runs a negative control, a defect the tool must refuse, so a gate that checks nothing fails.
- The repository holds the generated vectors and traces, and the gate requires the generator to reproduce them byte for byte.

## Consequences

- `cargo xtask proofs` runs every layer, and a change to a verified decision updates its proof, model, or vectors in the same change.
- The vectors and traces stay outside the package.
  The tests that read them compile with `--cfg windows_spawn_vectors`, which `cargo xtask test` passes.
- The verification found and fixed two defects.
  `Command::to_command_line` refused a `PATH` set from a handle that a later operation replaced or removed.
  An attribute-list size beyond one allocation panicked.
- It also confirmed four behaviors the crate keeps.
  After an exit that no call observed, `Child::kill` returns the `ERROR_ACCESS_DENIED` of `TerminateProcess`, and `std::process::Child::kill` returns `Ok`.
  After another process terminates a suspended child, `resume` returns `InvalidData`, because the primary thread then reads a suspend count of zero.
  `CommandLineToArgvW` and `msvcrt.dll` read two double quotes inside a quoted region as the C runtime did before Visual C++ 2008, and both readings return the crate's command lines unchanged.
  `ParentProcess::from_handle` and `Job::from_handle` check a handle's kind with an API of that kind, which `appverif.exe` reports as an incorrect object type when the handle has another kind.
- Each proof holds as far as its statement reaches.
  Kani holds within its bounds, Lean and Verus hold for their model and copy, and the Windows tests tie each to the system.
