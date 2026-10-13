-- SPDX-FileCopyrightText: 2026 Yasunobu Sakashita
--
-- SPDX-License-Identifier: MIT OR Apache-2.0

import Lean
import WindowsSpawn

/-!
# The axiom audit

`lake exe audit [LIBRARY]` imports `WindowsSpawn`, or `LIBRARY`, and prints one line for each theorem its sources state:
the theorem, `private` or `public`, and the axioms it rests on, separated by tabs.
It exits 1 when any theorem of the library, stated or derived, rests on an axiom outside `propext`, `Classical.choice`, and `Quot.sound`,
which is how an unfinished proof (`sorryAx`), compiled code (`Lean.ofReduceBool`), or a declared axiom shows,
and 2 when it finds no theorem, so an empty report never reads as a clean one.
-/

open Lean

/-- The axioms a proof may rest on. -/
def allowed : List Name := [``propext, ``Classical.choice, ``Quot.sound]

/-- Runs `action` against the imported library. -/
def core {α : Type} (env : Environment) (action : CoreM α) : IO α := do
  let (result, _) ← action.toIO { fileName := "audit", fileMap := default } { env }
  return result

/-- The axioms that `name` rests on. -/
def axiomsOf (env : Environment) (name : Name) : IO (Array Name) :=
  core env (collectAxioms name)

/-- Returns true if a source file states `name`, rather than the elaborator deriving it. -/
def stated (env : Environment) (name : Name) : IO Bool :=
  core env do return (← findDeclarationRanges? name).isSome

/-- Returns true if `name` comes from one of the modules of `library`. -/
def inLibrary (env : Environment) (library : Name) (name : Name) : Bool :=
  match env.getModuleIdxFor? name with
  | some index => library.isPrefixOf (env.header.moduleNames[index.toNat]!)
  | none => false

/-- Audits every theorem of the library the first argument names, `WindowsSpawn` unless one is given. -/
def main (arguments : List String) : IO UInt32 := do
  let library := match arguments with
    | [name] => name.toName
    | _ => `WindowsSpawn
  initSearchPath (← findSysroot)
  let env ← importModules #[{ module := library }] {} (trustLevel := 1024)
  let mut lines : Array String := #[]
  let mut refused := false
  for (name, info) in env.constants.map₁.toList do
    if !inLibrary env library name then continue
    let .thmInfo _ := info | continue
    let axioms ← axiomsOf env name
    let outside := axioms.filter (fun used => !allowed.contains used)
    if !outside.isEmpty then
      refused := true
      IO.eprintln s!"{name} rests on {outside.toList}"
    if !(← stated env name) then continue
    let visibility := if isPrivateName name then "private" else "public"
    let shown := (privateToUserName? name).getD name
    let names := axioms.toList.map toString |>.toArray.qsort (· < ·)
    lines := lines.push s!"{shown}\t{visibility}\t{",".intercalate names.toList}"
  if lines.isEmpty then
    IO.eprintln "the library declares no theorem"
    return 2
  for line in lines.qsort (· < ·) do
    IO.println line
  return if refused then 1 else 0
