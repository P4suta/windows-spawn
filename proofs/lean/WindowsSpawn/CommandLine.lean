-- SPDX-FileCopyrightText: 2026 Yasunobu Sakashita
--
-- SPDX-License-Identifier: MIT OR Apache-2.0

/-!
# Command lines

A model of `append_program` and `append_argument` in `src/domain/command_line_rules.rs`,
and of the two C runtime parsers that read a command line back.

`split_encodeLine` proves that both parsers return exactly the program name and the arguments the encoder took,
for every list of arguments of every length.
A unit is a natural number: the encoder and the parsers compare units only with the special ones and copy every other unit.
The parsers stop at a NUL unit, which no argument may hold, so the model leaves NUL out.
-/

namespace WindowsSpawn.CommandLine

/-- The double quote, which opens and closes a quoted region. -/
def quote : Nat := 0x22

/-- The backslash, which escapes a double quote. -/
def backslash : Nat := 0x5c

/-- The space, which separates arguments outside a quoted region. -/
def space : Nat := 0x20

/-- The horizontal tab, which also separates arguments outside a quoted region. -/
def tab : Nat := 0x09

/-- The vertical bar, which the encoder quotes for command shells. -/
def bar : Nat := 0x7c

/-- A unit that separates arguments outside a quoted region. -/
def blank (u : Nat) : Bool := u == space || u == tab

/-- A unit that sends an argument inside double quotes. -/
def special (u : Nat) : Bool := blank u || u == quote || u == bar

/-- `needs_quotes`: an empty argument, or one that holds a special unit. -/
def needsQuotes (arg : List Nat) : Bool := arg.isEmpty || arg.any special

/-- The units the loop of `append_argument` writes for `arg` when `n` backslashes precede it. -/
def bodyUnits : List Nat → Nat → List Nat
  | [], _ => []
  | u :: rest, n =>
    if u = backslash then backslash :: bodyUnits rest (n + 1)
    else if u = quote then List.replicate (n + 1) backslash ++ quote :: bodyUnits rest 0
    else u :: bodyUnits rest 0

/-- The backslashes that end `arg` when `n` backslashes precede it. -/
def trailing : List Nat → Nat → Nat
  | [], n => n
  | u :: rest, n => if u = backslash then trailing rest (n + 1) else trailing rest 0

/-- `append_argument`: an argument that needs quotes gets them, and the backslashes before its closing quote double. -/
def encodeArg (arg : List Nat) : List Nat :=
  if needsQuotes arg then
    quote :: (bodyUnits arg 0 ++ List.replicate (trailing arg 0) backslash ++ [quote])
  else bodyUnits arg 0

/-- `append_program`. -/
def encodeProgram (program : List Nat) : List Nat := quote :: (program ++ [quote])

/-- The arguments of `build_command_line`, each after a space. -/
def encodeArgs : List (List Nat) → List Nat
  | [] => []
  | arg :: rest => space :: (encodeArg arg ++ encodeArgs rest)

/-- The command line `build_command_line` writes, without its NUL. -/
def encodeLine (program : List Nat) (args : List (List Nat)) : List Nat :=
  encodeProgram program ++ encodeArgs args

/-- The two C runtime parsers, which differ on two double quotes inside a quoted region. -/
inductive Dialect where
  /-- Visual C++ 2008 and later, and the Universal CRT: a literal double quote, and the region stays open. -/
  | ucrt
  /-- Visual C++ before 2008: a literal double quote, and the region closes. -/
  | msvcrt
  deriving DecidableEq, Repr

/-- Whether a region stays quoted after two double quotes inside it. -/
def Dialect.staysQuoted : Dialect → Bool
  | .ucrt => true
  | .msvcrt => false

/-- Reads one argument from `l`, inside a quoted region when `q`, with `n` backslashes read and not yet written.

`2n` backslashes before a double quote stand for `n` backslashes and a quote that toggles the region,
`2n + 1` stand for `n` backslashes and a literal quote, and backslashes before any other unit stand for themselves.
Returns the argument and the rest of the line, which starts at the blank that ended the argument. -/
def argument (d : Dialect) : List Nat → Bool → Nat → List Nat × List Nat
  | [], _, n => (List.replicate n backslash, [])
  | u :: rest, q, n =>
    if u = backslash then argument d rest q (n + 1)
    else if u = quote then
      if n % 2 = 1 then
        (List.replicate (n / 2) backslash ++ quote :: (argument d rest q 0).1,
          (argument d rest q 0).2)
      else if q = true ∧ rest.head? = some quote then
        (List.replicate (n / 2) backslash ++ quote :: (argument d rest.tail d.staysQuoted 0).1,
          (argument d rest.tail d.staysQuoted 0).2)
      else
        (List.replicate (n / 2) backslash ++ (argument d rest (!q) 0).1,
          (argument d rest (!q) 0).2)
    else if q = false ∧ blank u = true then (List.replicate n backslash, u :: rest)
    else
      (List.replicate n backslash ++ u :: (argument d rest q 0).1, (argument d rest q 0).2)
termination_by l => l.length

/-- Skips the blanks before an argument. -/
def skipBlanks : List Nat → List Nat
  | [] => []
  | u :: rest => if blank u = true then skipBlanks rest else u :: rest

private theorem skipBlanks_length (l : List Nat) : (skipBlanks l).length ≤ l.length := by
  induction l with
  | nil => simp [skipBlanks]
  | cons u rest ih =>
    simp only [skipBlanks]
    split <;> simp_all <;> omega

private theorem skipBlanks_head {l : List Nat} {u : Nat} {rest : List Nat}
    (h : skipBlanks l = u :: rest) : blank u = false := by
  induction l with
  | nil => simp [skipBlanks] at h
  | cons v tail ih =>
    simp only [skipBlanks] at h
    split at h
    · exact ih h
    · simp_all

private theorem argument_length (d : Dialect) (l : List Nat) (q : Bool) (n : Nat) :
    (argument d l q n).2.length ≤ l.length := by
  fun_induction argument d l q n <;> simp_all <;> grind

private theorem quote_ne_backslash : quote ≠ backslash := by decide
private theorem backslash_ne_quote : backslash ≠ quote := by decide
private theorem blank_quote : blank quote = false := by decide
private theorem blank_backslash : blank backslash = false := by decide
private theorem blank_space : blank space = true := by decide

/-- Reading a backslash out of an argument consumes a unit, and so does any unit that does not end it. -/
private theorem argument_consumes (d : Dialect) (u : Nat) (rest : List Nat) (n : Nat)
    (hu : blank u = false) : (argument d (u :: rest) false n).2.length < (u :: rest).length := by
  have hr := argument_length d rest
  have ht := argument_length d rest.tail
  have hl : rest.tail.length ≤ rest.length := by simp
  rw [argument.eq_2]
  by_cases hb : u = backslash
  · have := hr false (n + 1); simp [hb]; omega
  · by_cases hq : u = quote
    · subst hq
      have := hr false 0
      have := hr true 0
      have := ht d.staysQuoted 0
      simp only [quote_ne_backslash, ite_false, ite_true]
      split <;> (try split) <;> simp <;> omega
    · have := hr false 0
      simp [hb, hq, hu]
      omega

/-- Splits the arguments that follow the program name. -/
def arguments (d : Dialect) (l : List Nat) : List (List Nat) :=
  match hs : skipBlanks l with
  | [] => []
  | u :: rest =>
    have : (argument d (u :: rest) false 0).2.length < l.length := by
      have := skipBlanks_length l
      have := argument_consumes d u rest 0 (skipBlanks_head hs)
      rw [hs] at *
      omega
    (argument d (u :: rest) false 0).1 :: arguments d (argument d (u :: rest) false 0).2
termination_by l.length

/-- The program name as the Universal CRT reads it: double quotes toggle a quoted region, and an unquoted blank ends the name. -/
def programUcrt : List Nat → Bool → List Nat × List Nat
  | [], _ => ([], [])
  | u :: rest, q =>
    if u = quote then programUcrt rest (!q)
    else if q = false ∧ blank u = true then ([], rest)
    else ((u :: (programUcrt rest q).1), (programUcrt rest q).2)

/-- The program name from a leading double quote to the next one. -/
def untilQuote : List Nat → List Nat × List Nat
  | [] => ([], [])
  | u :: rest => if u = quote then ([], rest) else (u :: (untilQuote rest).1, (untilQuote rest).2)

/-- The program name up to the first blank. -/
def untilBlank : List Nat → List Nat × List Nat
  | [] => ([], [])
  | u :: rest => if blank u = true then ([], rest) else (u :: (untilBlank rest).1, (untilBlank rest).2)

/-- The program name as the C runtime before Visual C++ 2008 reads it, and as `CommandLineToArgvW` documents it. -/
def programMsvcrt : List Nat → List Nat × List Nat
  | [] => ([], [])
  | u :: rest => if u = quote then untilQuote rest else untilBlank (u :: rest)

/-- The program name and the rest of the line, as the C runtime of `d` reads them. -/
def program (d : Dialect) (line : List Nat) : List Nat × List Nat :=
  match d with
  | .ucrt => programUcrt line false
  | .msvcrt => programMsvcrt line

/-- The program name and the arguments of `line`, as the C runtime of `d` reads them. -/
def split (d : Dialect) (line : List Nat) : List (List Nat) :=
  (program d line).1 :: arguments d (program d line).2

/-- A run of backslashes adds to the backslashes read and not yet written. -/
private theorem argument_backslashes (d : Dialect) (k : Nat) (l : List Nat) (q : Bool) (n : Nat) :
    argument d (List.replicate k backslash ++ l) q n = argument d l q (n + k) := by
  induction k generalizing n with
  | zero => simp
  | succ k ih =>
    rw [List.replicate_succ, List.cons_append, argument.eq_2]
    simp only [ite_true]
    rw [ih]
    congr 1
    omega

/-- A double quote after an odd number of backslashes is a literal quote. -/
private theorem argument_escaped (d : Dialect) (rest : List Nat) (q : Bool) (n : Nat)
    (hn : n % 2 = 1) :
    argument d (quote :: rest) q n =
      (List.replicate (n / 2) backslash ++ quote :: (argument d rest q 0).1,
        (argument d rest q 0).2) := by
  rw [argument.eq_2]
  simp [quote_ne_backslash, hn]

/-- A double quote after an even number of backslashes toggles the region, unless it opens a doubled quote inside one. -/
private theorem argument_toggle (d : Dialect) (rest : List Nat) (q : Bool) (n : Nat)
    (hn : n % 2 = 0) (hrest : ¬(q = true ∧ rest.head? = some quote)) :
    argument d (quote :: rest) q n =
      (List.replicate (n / 2) backslash ++ (argument d rest (!q) 0).1,
        (argument d rest (!q) 0).2) := by
  rw [argument.eq_2]
  have hodd : ¬(n % 2 = 1) := by omega
  simp only [quote_ne_backslash, ite_false, ite_true, hodd, hrest]

/-- Any other unit inside a quoted region joins the argument after the backslashes before it. -/
private theorem argument_inside (d : Dialect) (u : Nat) (rest : List Nat) (n : Nat)
    (hb : u ≠ backslash) (hq : u ≠ quote) :
    argument d (u :: rest) true n =
      (List.replicate n backslash ++ u :: (argument d rest true 0).1, (argument d rest true 0).2) := by
  rw [argument.eq_2]
  simp [hb, hq]

/-- A line that ends, or goes on at a blank, ends the argument. -/
def Ends (rest : List Nat) : Prop := rest = [] ∨ ∃ v after, rest = v :: after ∧ blank v = true

private theorem ends_head {rest : List Nat} (h : Ends rest) : rest.head? ≠ some quote := by
  rcases h with h | ⟨v, after, rfl, hv⟩
  · simp [h]
  · intro hq
    simp at hq
    subst hq
    simp [blank_quote] at hv

/-- At the end of an argument the parser writes the pending backslashes and stops. -/
private theorem argument_ends (d : Dialect) (rest : List Nat) (n : Nat) (h : Ends rest) :
    argument d rest false n = (List.replicate n backslash, rest) := by
  rcases h with rfl | ⟨v, after, rfl, hv⟩
  · simp [argument.eq_1]
  · have hb : v ≠ backslash := by intro e; subst e; simp [blank_backslash] at hv
    have hq : v ≠ quote := by intro e; subst e; simp [blank_quote] at hv
    rw [argument.eq_2]
    simp [hb, hq, hv]

/-- The body of a quoted argument, its doubled trailing backslashes, and its closing quote decode to the argument. -/
private theorem argument_quoted (d : Dialect) (rest : List Nat) (hrest : rest.head? ≠ some quote) :
    ∀ (arg : List Nat) (n : Nat),
      argument d
          (List.replicate n backslash ++
            (bodyUnits arg n ++ (List.replicate (trailing arg n) backslash ++ quote :: rest)))
          true 0 =
        (List.replicate n backslash ++ (arg ++ (argument d rest false 0).1),
          (argument d rest false 0).2)
  | [], n => by
    have hq : ¬(true = true ∧ rest.head? = some quote) := by simpa using hrest
    rw [bodyUnits, trailing, List.nil_append, ← List.append_assoc,
      List.replicate_append_replicate, argument_backslashes,
      argument_toggle d rest true (0 + (n + n)) (by omega) hq]
    have : (0 + (n + n)) / 2 = n := by omega
    rw [this]
    simp
  | u :: arg, n => by
    by_cases hb : u = backslash
    · subst hb
      have ih := argument_quoted d rest hrest arg (n + 1)
      simp only [bodyUnits, trailing, ite_true]
      rw [List.replicate_succ'] at ih
      simpa [List.append_assoc] using ih
    · by_cases hq : u = quote
      · subst hq
        have ih := argument_quoted d rest hrest arg 0
        simp only [bodyUnits, trailing, quote_ne_backslash, ite_false, ite_true,
          List.append_assoc, List.cons_append]
        rw [← List.append_assoc, List.replicate_append_replicate, argument_backslashes,
          argument_escaped d _ true _ (by omega)]
        simp only [List.replicate_zero, List.nil_append] at ih
        have : (0 + (n + (n + 1))) / 2 = n := by omega
        rw [ih, this]
      · have ih := argument_quoted d rest hrest arg 0
        simp only [bodyUnits, trailing, hb, hq, ite_false]
        rw [argument_backslashes, List.cons_append, argument_inside d u _ (0 + n) hb hq]
        simp only [List.replicate_zero, List.nil_append] at ih
        rw [ih]
        simp

/-- The loop of `append_argument` copies an argument without double quotes. -/
private theorem bodyUnits_plain : ∀ (arg : List Nat) (n : Nat), quote ∉ arg → bodyUnits arg n = arg
  | [], _, _ => rfl
  | u :: arg, n, h => by
    have hq : u ≠ quote := by intro e; subst e; simp at h
    have harg : quote ∉ arg := fun m => h (List.mem_cons_of_mem u m)
    by_cases hb : u = backslash
    · subst hb; simp [bodyUnits, bodyUnits_plain arg _ harg]
    · simp [bodyUnits, hb, hq, bodyUnits_plain arg _ harg]

/-- An argument without special units decodes to itself. -/
private theorem argument_plain (d : Dialect) (rest : List Nat) (hrest : Ends rest) :
    ∀ (arg : List Nat) (n : Nat), (∀ u ∈ arg, special u = false) →
      argument d (arg ++ rest) false n = (List.replicate n backslash ++ arg, rest)
  | [], n, _ => by simpa using argument_ends d rest n hrest
  | u :: arg, n, h => by
    have hu := h u (List.mem_cons_self ..)
    have harg : ∀ v ∈ arg, special v = false := fun v m => h v (List.mem_cons_of_mem u m)
    have hq : u ≠ quote := by intro e; subst e; simp [special] at hu
    have hblank : blank u = false := by simp [special] at hu; simp [hu]
    by_cases hb : u = backslash
    · subst hb
      have ih := argument_plain d rest hrest arg (n + 1) harg
      rw [List.replicate_succ'] at ih
      rw [List.cons_append, argument.eq_2]
      simpa [List.append_assoc] using ih
    · have ih := argument_plain d rest hrest arg 0 harg
      rw [List.cons_append, argument.eq_2]
      simp [hb, hq, hblank, ih]

/-- `append_argument` writes what the parsers read back as exactly the argument, up to the blank or the end that follows. -/
theorem argument_encodeArg (d : Dialect) (arg rest : List Nat) (hrest : Ends rest) :
    argument d (encodeArg arg ++ rest) false 0 = (arg, rest) := by
  unfold encodeArg
  split
  · rw [List.cons_append, List.append_assoc, List.append_assoc, List.singleton_append,
      argument_toggle d _ false 0 (by omega) (by simp)]
    have h := argument_quoted d rest (ends_head hrest) arg 0
    simp only [List.replicate_zero, List.nil_append] at h
    simp [h, argument_ends d rest 0 hrest]
  · rename_i hq
    have hspecial : ∀ u ∈ arg, special u = false := by
      intro u m
      simp [needsQuotes] at hq
      exact hq.2 u m
    have hquote : quote ∉ arg := by
      intro m
      have := hspecial quote m
      simp [special] at this
    rw [bodyUnits_plain arg 0 hquote]
    simpa using argument_plain d rest hrest arg 0 hspecial

private theorem encodeArgs_ends (args : List (List Nat)) : Ends (encodeArgs args) := by
  cases args with
  | nil => exact Or.inl rfl
  | cons arg rest => exact Or.inr ⟨space, _, rfl, blank_space⟩

/-- `append_argument` writes at least one unit, and the first one is no blank. -/
private theorem encodeArg_head (arg : List Nat) :
    ∃ u t, encodeArg arg = u :: t ∧ blank u = false := by
  unfold encodeArg
  split
  · exact ⟨quote, _, rfl, blank_quote⟩
  · rename_i hq
    cases arg with
    | nil => simp [needsQuotes] at hq
    | cons u rest =>
      have hu : special u = false := by
        simp only [needsQuotes, List.isEmpty_cons, Bool.false_or, Bool.not_eq_true,
          List.any_eq_false] at hq
        simpa using hq u (List.mem_cons_self ..)
      have hblank : blank u = false := by simp [special] at hu; simp [hu]
      have hq : u ≠ quote := by intro e; subst e; simp [special] at hu
      by_cases hb : u = backslash
      · subst hb
        exact ⟨backslash, bodyUnits rest 1, by simp [bodyUnits], blank_backslash⟩
      · exact ⟨u, bodyUnits rest 0, by simp [bodyUnits, hb, hq], hblank⟩

private theorem skipBlanks_nonblank (u : Nat) (t : List Nat) (hu : blank u = false) :
    skipBlanks (u :: t) = u :: t := by
  simp [skipBlanks, hu]

private theorem arguments_nil (d : Dialect) : arguments d [] = [] := by
  rw [arguments.eq_def]
  split
  · rfl
  · rename_i hs
    simp [skipBlanks] at hs

/-- An argument starts at a unit that is no blank. -/
private theorem arguments_nonblank (d : Dialect) (u : Nat) (t : List Nat) (hu : blank u = false) :
    arguments d (u :: t) =
      (argument d (u :: t) false 0).1 :: arguments d (argument d (u :: t) false 0).2 := by
  rw [arguments.eq_def]
  split
  · rename_i hs
    simp [skipBlanks_nonblank u t hu] at hs
  · rename_i v rest hs
    rw [skipBlanks_nonblank u t hu] at hs
    cases hs
    rfl

/-- Blanks before an argument are skipped. -/
private theorem arguments_blank (d : Dialect) (v : Nat) (t : List Nat) (hv : blank v = true) :
    arguments d (v :: t) = arguments d t := by
  have hs : skipBlanks (v :: t) = skipBlanks t := by simp [skipBlanks, hv]
  conv => lhs; rw [arguments.eq_def]
  conv => rhs; rw [arguments.eq_def]
  split
  · rename_i h
    split
    · rfl
    · rename_i u rest h'
      rw [hs, h'] at h
      cases h
  · rename_i u rest h
    split
    · rename_i h'
      rw [hs, h'] at h
      cases h
    · rename_i u' rest' h'
      rw [hs, h'] at h
      cases h
      rfl

/-- The arguments `build_command_line` writes split back into exactly those arguments. -/
private theorem arguments_encodeArgs (d : Dialect) :
    ∀ args : List (List Nat), arguments d (encodeArgs args) = args
  | [] => by simp [encodeArgs, arguments_nil]
  | arg :: rest => by
    obtain ⟨u, t, he, hu⟩ := encodeArg_head arg
    rw [encodeArgs, arguments_blank d space _ blank_space, he, List.cons_append,
      arguments_nonblank d u _ hu, ← List.cons_append, ← he,
      argument_encodeArg d arg (encodeArgs rest) (encodeArgs_ends rest),
      arguments_encodeArgs d rest]

private theorem programUcrt_quoted (x : List Nat) :
    ∀ p : List Nat, quote ∉ p →
      programUcrt (p ++ quote :: x) true = (p ++ (programUcrt x false).1, (programUcrt x false).2)
  | [], _ => by simp [programUcrt]
  | u :: p, h => by
    have hu : u ≠ quote := by intro e; subst e; simp at h
    have hp : quote ∉ p := fun m => h (List.mem_cons_of_mem u m)
    simp [programUcrt, hu, programUcrt_quoted x p hp]

private theorem untilQuote_quoted (x : List Nat) :
    ∀ p : List Nat, quote ∉ p → untilQuote (p ++ quote :: x) = (p, x)
  | [], _ => by simp [untilQuote]
  | u :: p, h => by
    have hu : u ≠ quote := by intro e; subst e; simp at h
    have hp : quote ∉ p := fun m => h (List.mem_cons_of_mem u m)
    simp [untilQuote, hu, untilQuote_quoted x p hp]

/-- What follows the program name splits into the arguments, whichever way the dialect ends the name. -/
private theorem arguments_after_program (d : Dialect) (args : List (List Nat)) :
    arguments d (programUcrt (encodeArgs args) false).2 = args := by
  cases args with
  | nil => simp [encodeArgs, programUcrt, arguments_nil]
  | cons arg rest =>
    simp only [encodeArgs, programUcrt, show space ≠ quote by decide, blank_space, ite_false,
      and_self, ite_true]
    rw [← arguments_blank d space _ blank_space]
    exact arguments_encodeArgs d (arg :: rest)

private theorem program_name_ucrt (args : List (List Nat)) :
    (programUcrt (encodeArgs args) false).1 = [] := by
  cases args with
  | nil => simp [encodeArgs, programUcrt]
  | cons arg rest =>
    simp [encodeArgs, programUcrt, show space ≠ quote by decide, blank_space]

/-- Both C runtimes split the line `build_command_line` writes into exactly its program name and arguments.

The program name holds no double quote, which `validate_command` rejects; nothing else constrains the arguments. -/
theorem split_encodeLine (d : Dialect) (p : List Nat) (args : List (List Nat))
    (hp : quote ∉ p) : split d (encodeLine p args) = p :: args := by
  cases d with
  | ucrt =>
    simp only [split, program, encodeLine, encodeProgram, List.cons_append, List.append_assoc,
      programUcrt, ite_true, Bool.not_false, List.nil_append]
    rw [programUcrt_quoted _ p hp]
    simp [program_name_ucrt, arguments_after_program]
  | msvcrt =>
    simp only [split, program, encodeLine, encodeProgram, List.cons_append, List.append_assoc,
      programMsvcrt, ite_true, List.nil_append]
    rw [untilQuote_quoted _ p hp]
    simp [arguments_encodeArgs]

/-- An argument without special units goes into the line unchanged, as `std::process::Command` writes it. -/
theorem encodeArg_plain (arg : List Nat) (h : needsQuotes arg = false) : encodeArg arg = arg := by
  have hquote : quote ∉ arg := by
    intro m
    simp [needsQuotes] at h
    have := h.2 quote m
    simp [special] at this
  simp [encodeArg, h, bodyUnits_plain arg 0 hquote]

private theorem split_encodeLine_witness :
    split .ucrt (encodeLine [0x70] [[], [quote], [backslash, space], [backslash, quote]]) =
      [[0x70], [], [quote], [backslash, space], [backslash, quote]] :=
  split_encodeLine .ucrt [0x70] [[], [quote], [backslash, space], [backslash, quote]] (by decide)

private theorem encodeArg_plain_witness : encodeArg [0x61, backslash] = [0x61, backslash] :=
  encodeArg_plain [0x61, backslash] (by decide)

private theorem argument_encodeArg_witness :
    argument .msvcrt (encodeArg [backslash, quote] ++ [space]) false 0 = ([backslash, quote], [space]) :=
  argument_encodeArg .msvcrt [backslash, quote] [space] (Or.inr ⟨space, [], rfl, by decide⟩)

end WindowsSpawn.CommandLine
