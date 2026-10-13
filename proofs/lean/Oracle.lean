-- SPDX-FileCopyrightText: 2026 Yasunobu Sakashita
--
-- SPDX-License-Identifier: MIT OR Apache-2.0

import WindowsSpawn

/-!
# The oracle

`lake exe oracle DIRECTORY` evaluates the proved models over fixed inputs and writes the vectors the Rust tests read:

- `command_line_encoding.tsv`: every argument over the units `a`, space, tab, double quote, backslash, and vertical bar,
  up to four units long, and the units `encodeArg` writes for it.
- `command_line_parsing.tsv`: every line over the units `a`, space, double quote, and backslash, up to six units long,
  after the program token `p`, and the arguments each dialect reads from it.

Units are hexadecimal and separated by spaces; arguments are separated by `|`.
The inputs come from the oracle alone, so the files change only with the models.
-/

open WindowsSpawn.CommandLine

/-- Every list over `alphabet` of at most `length` units, shortest first. -/
def words (alphabet : List Nat) : Nat → List (List Nat)
  | 0 => [[]]
  | length + 1 =>
    let shorter := words alphabet length
    let longest := shorter.filter (·.length = length)
    shorter ++ (longest.flatMap fun word => alphabet.map fun unit => word ++ [unit])

/-- Units as lowercase hexadecimal, separated by spaces. -/
def hex (units : List Nat) : String :=
  " ".intercalate (units.map fun unit => String.ofList (Nat.toDigits 16 unit))

/-- Arguments as hexadecimal units, separated by `|`. -/
def hexArguments (arguments : List (List Nat)) : String :=
  "|".intercalate (arguments.map hex)

/-- The encoding vectors. -/
def encoding : List String :=
  (words [0x61, space, tab, quote, backslash, bar] 4).map fun arg =>
    s!"{hex arg}\t{hex (encodeArg arg)}"

/-- The parsing vectors. -/
def parsing : List String :=
  (words [0x61, space, quote, backslash] 6).map fun tail =>
    let line := [0x70, space] ++ tail
    s!"{hex line}\t{hexArguments (split .ucrt line)}\t{hexArguments (split .msvcrt line)}"

/-- Writes the vectors into the directory the first argument names. -/
def main (arguments : List String) : IO UInt32 := do
  let [directory] := arguments | do
    IO.eprintln "usage: lake exe oracle DIRECTORY"
    return 2
  IO.FS.createDirAll directory
  IO.FS.writeFile s!"{directory}/command_line_encoding.tsv" ("\n".intercalate encoding ++ "\n")
  IO.FS.writeFile s!"{directory}/command_line_parsing.tsv" ("\n".intercalate parsing ++ "\n")
  return 0
