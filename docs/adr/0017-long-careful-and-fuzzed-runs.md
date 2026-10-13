# 0017 — Long, careful, and fuzzed runs check what bounded tests can't

Status: accepted (2026-10-13)

## Context

The tests run each behavior a few times, in one configuration, on x64 and i686.
A handle that leaks once in many spawns, an unsafe precondition that holds by luck, a fault on ARM64, and an encoder fault beyond the vectors and the bounds of Kani all pass them.
PR #14 ran a stress test, `cargo careful`, an ARM64 smoke test, Miri, AddressSanitizer, and two fuzz targets, and main dropped them with #14.

## Decision

- An ignored test, `spawn_cleanup_stress`, spawns children in thirteen modes.
  The modes cover both commits, a dropped suspended child, collected output, written input, a killed tree, transferred handles, a Job, an alternate parent, and two failed spawns.
  It requires the process's handle count to end where it started, after a warm-up that runs each mode twice.
  `WINDOWS_SPAWN_STRESS_CASES` sets the number of spawns: 1300 on every PR on x64 and i686, and 20000 on x64 and 5000 on i686 every night.
- CI runs the Windows tests under `cargo careful`, whose standard library checks the preconditions of the unsafe operations in `sys`, such as those of `slice::from_raw_parts`.
- CI runs `cargo xtask test` on a Windows 11 ARM64 runner.
- A `fuzz` crate compiles the encoder and the block builder from `src/domain`, and requires the parser models to read back what they wrote.
  The parser models and the block scan compile under `cfg(fuzzing)` for it.
  Each target runs for ten minutes on every PR, and for fifteen minutes every night.
  The fuzz crate is its own workspace, outside the package, the MSRV, and the workspace lints, and the repository gate checks its targets.
- Miri and AddressSanitizer run the platform-neutral decisions on Linux.
  Those decisions hold no unsafe code, so these runs guard against a change that brings some in, at the cost of minutes.
- `WINDOWS_SPAWN_RANDOM_SCALE` multiplies the random command lines and environments, which keep their seeds, and the nightly workflow runs them at ten times their size.

Rejected:

- The fuzz targets of #14, which checked only that the code doesn't panic.
- A stress test that measures time, which ADR 0010 bans.
  A handle count decides by state.
- ARM64 through emulation, which would test the emulator.

## Consequences

- CI gains six jobs, and `required` waits for each of them.
- A leak in every spawn fails a PR, and a rarer leak fails the nightly run, whose 20000 spawns let it add up.
- A failed fuzz run uploads the input that failed.
- The nightly workflow runs on its schedule or on demand, spends about an hour of runner time, and gates no PR.
