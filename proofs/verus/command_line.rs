// SPDX-FileCopyrightText: 2026 Yasunobu Sakashita
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The command-line encoder of `src/domain/command_line_rules.rs`, verified by Verus for every argument.
//!
//! `needs_quotes`, `append_program`, `append_argument`, and `put_backslashes` below are the crate's functions:
//! `cargo xtask proofs verus` requires each to equal its source token for token once the Verus attributes and proof blocks are removed.
//! Their specifications say they write what the Lean model writes: `body_units`, `trailing`, and `encode` transcribe `bodyUnits`, `trailing`, and `encodeArg` of `proofs/lean/WindowsSpawn/CommandLine.lean`,
//! for which `split_encodeLine` proves that both C runtime parsers read every argument back.

#![feature(proc_macro_hygiene)]

use vstd::prelude::*;

/// The double quote, which opens and closes a quoted region.
#[verus_verify]
pub const QUOTE: u16 = 0x22;
/// The backslash, which escapes a double quote.
#[verus_verify]
pub const BACKSLASH: u16 = 0x5c;
/// The space, which separates arguments outside a quoted region.
#[verus_verify]
pub const SPACE: u16 = 0x20;
/// The horizontal tab, which also separates arguments outside a quoted region.
#[verus_verify]
pub const TAB: u16 = 0x09;
/// The vertical bar, which a command shell reads as a pipe outside a quoted region.
#[verus_verify]
pub const VERTICAL_BAR: u16 = 0x7c;

verus! {

broadcast use vstd::slice::group_slice_axioms;

/// `UnitSink` of `src/domain/mod.rs`, whose `put` appends exactly one unit.
pub trait UnitSink {
    spec fn units(&self) -> Seq<u16>;

    fn put(&mut self, unit: u16)
        ensures
            final(self).units() == old(self).units().push(unit),
    ;
}

/// The sink a spawn writes into.
impl UnitSink for Vec<u16> {
    open spec fn units(&self) -> Seq<u16> {
        self@
    }

    fn put(&mut self, unit: u16) {
        self.push(unit);
    }
}

/// `special` of the Lean model.
pub open spec fn special(u: u16) -> bool {
    u == SPACE || u == TAB || u == QUOTE || u == VERTICAL_BAR
}

/// `needsQuotes` of the Lean model.
pub open spec fn needs_quotes_spec(arg: Seq<u16>) -> bool {
    arg.len() == 0 || exists|i: int| 0 <= i < arg.len() && special(#[trigger] arg[i])
}

/// `List.replicate n backslash`.
pub open spec fn repeat_backslash(n: nat) -> Seq<u16> {
    Seq::new(n, |_i: int| BACKSLASH)
}

/// `bodyUnits` of the Lean model: the units the loop of `append_argument` writes for `arg` when `n` backslashes precede it.
pub open spec fn body_units(arg: Seq<u16>, n: nat) -> Seq<u16>
    decreases arg.len(),
{
    if arg.len() == 0 {
        Seq::empty()
    } else if arg[0] == BACKSLASH {
        seq![BACKSLASH] + body_units(arg.drop_first(), n + 1)
    } else if arg[0] == QUOTE {
        repeat_backslash(n + 1) + seq![QUOTE] + body_units(arg.drop_first(), 0)
    } else {
        seq![arg[0]] + body_units(arg.drop_first(), 0)
    }
}

/// `trailing` of the Lean model: the backslashes that end `arg` when `n` backslashes precede it.
pub open spec fn trailing(arg: Seq<u16>, n: nat) -> nat
    decreases arg.len(),
{
    if arg.len() == 0 {
        n
    } else if arg[0] == BACKSLASH {
        trailing(arg.drop_first(), n + 1)
    } else {
        trailing(arg.drop_first(), 0)
    }
}

/// `encodeArg` of the Lean model.
pub open spec fn encode(arg: Seq<u16>) -> Seq<u16> {
    if needs_quotes_spec(arg) {
        seq![QUOTE] + body_units(arg, 0) + repeat_backslash(trailing(arg, 0)) + seq![QUOTE]
    } else {
        body_units(arg, 0)
    }
}

/// What `append_argument` writes for one unit after `n` backslashes.
pub open spec fn step(u: u16, n: nat) -> Seq<u16> {
    if u == BACKSLASH {
        seq![BACKSLASH]
    } else if u == QUOTE {
        repeat_backslash(n + 1) + seq![QUOTE]
    } else {
        seq![u]
    }
}

/// The backslashes that precede the unit after `u`.
pub open spec fn after(u: u16, n: nat) -> nat {
    if u == BACKSLASH {
        n + 1
    } else {
        0
    }
}

pub open spec fn opening(quoted: bool) -> Seq<u16> {
    if quoted {
        seq![QUOTE]
    } else {
        Seq::empty()
    }
}

/// The Lean definitions read an argument from its start, and the loop extends what it has read at the end.
proof fn lemma_append(arg: Seq<u16>, n: nat, u: u16)
    ensures
        body_units(arg.push(u), n) == body_units(arg, n) + step(u, trailing(arg, n)),
        trailing(arg.push(u), n) == after(u, trailing(arg, n)),
    decreases arg.len(),
{
    let pushed = arg.push(u);
    if arg.len() == 0 {
        let empty = Seq::<u16>::empty();
        assert(pushed[0] == u);
        assert(pushed.drop_first() =~= empty);
        assert(body_units(empty, n + 1) == empty);
        assert(body_units(empty, 0) == empty);
        assert(trailing(empty, n + 1) == n + 1);
        assert(trailing(empty, 0) == 0);
        assert(arg =~= empty);
        assert(body_units(arg, n) == empty);
        assert(trailing(arg, n) == n);
        assert(body_units(pushed, n) =~= body_units(arg, n) + step(u, trailing(arg, n)));
        assert(trailing(pushed, n) == after(u, trailing(arg, n)));
    } else {
        let rest = arg.drop_first();
        assert(pushed[0] == arg[0]);
        assert(pushed.drop_first() =~= rest.push(u));
        lemma_append(rest, after(arg[0], n), u);
        assert(trailing(arg, n) == trailing(rest, after(arg[0], n)));
        assert(trailing(pushed, n) == trailing(rest.push(u), after(arg[0], n)));
        if arg[0] == BACKSLASH {
            assert(body_units(pushed, n) =~= body_units(arg, n) + step(u, trailing(arg, n)));
        } else if arg[0] == QUOTE {
            assert(body_units(pushed, n) =~= body_units(arg, n) + step(u, trailing(arg, n)));
        } else {
            assert(body_units(pushed, n) =~= body_units(arg, n) + step(u, trailing(arg, n)));
        }
    }
}

} // verus!

#[verus_spec(r =>
    ensures
        r == needs_quotes_spec(argument@),
)]
pub(crate) fn needs_quotes(argument: &[u16]) -> bool {
    argument.is_empty()
        || argument
            .iter()
            .any(#[verus_spec(r: bool => ensures r == special(*unit))] |unit| matches!(*unit, SPACE | TAB | QUOTE | VERTICAL_BAR))
}

#[verus_spec(
    ensures
        final(line).units() == old(line).units() + seq![QUOTE] + program@ + seq![QUOTE],
)]
pub(crate) fn append_program(line: &mut impl UnitSink, program: &[u16]) {
    line.put(QUOTE);
    #[verus_spec(iter =>
        invariant
            line.units() == old(line).units() + seq![QUOTE] + program@.take(iter.index() as int),
    )]
    for &unit in program {
        proof! {
            assert(program@.take(iter.index() as int + 1) =~= program@.take(iter.index() as int).push(unit));
        }
        line.put(unit);
    }
    proof! {
        assert(program@.take(program@.len() as int) =~= program@);
    }
    line.put(QUOTE);
}

#[verus_spec(
    ensures
        final(line).units() == old(line).units() + encode(argument@),
)]
pub(crate) fn append_argument(line: &mut impl UnitSink, argument: &[u16]) {
    let quoted = needs_quotes(argument);
    if quoted {
        line.put(QUOTE);
    }
    let mut backslashes = 0_usize;
    #[verus_spec(iter =>
        invariant
            quoted == needs_quotes_spec(argument@),
            backslashes == trailing(argument@.take(iter.index() as int), 0),
            backslashes <= iter.index(),
            line.units() == old(line).units() + opening(quoted) + body_units(argument@.take(iter.index() as int), 0),
    )]
    for &unit in argument {
        proof! {
            let read = argument@.take(iter.index() as int);
            assert(argument.len() as int == argument@.len());
            assert(argument@.take(iter.index() as int + 1) =~= read.push(unit));
            lemma_append(read, 0, unit);
        }
        if unit == BACKSLASH {
            backslashes = backslashes.saturating_add(1);
        } else {
            if unit == QUOTE {
                put_backslashes(line, backslashes.saturating_add(1));
            }
            backslashes = 0;
        }
        line.put(unit);
    }
    proof! {
        assert(argument@.take(argument@.len() as int) =~= argument@);
    }
    if quoted {
        put_backslashes(line, backslashes);
        line.put(QUOTE);
    }
}

#[verus_spec(
    ensures
        final(line).units() == old(line).units() + repeat_backslash(count as nat),
)]
fn put_backslashes(line: &mut impl UnitSink, count: usize) {
    #[verus_spec(iter =>
        invariant
            line.units() == old(line).units() + repeat_backslash(iter.index() as nat),
    )]
    for _ in 0..count {
        proof! {
            assert(repeat_backslash(iter.index() as nat + 1) =~= repeat_backslash(iter.index() as nat).push(BACKSLASH));
        }
        line.put(BACKSLASH);
    }
}

fn main() {
}
