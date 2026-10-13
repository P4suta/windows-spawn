-- SPDX-FileCopyrightText: 2026 Yasunobu Sakashita
--
-- SPDX-License-Identifier: MIT OR Apache-2.0

/-!
# The audit canary

Three proofs the axiom audit must refuse, one for each way a proof can rest on something the kernel did not check.
`cargo xtask proofs lean` builds this library and requires `lake exe audit Canary` to fail and to name all three.
-/

namespace Canary

/-- An unfinished proof, which rests on `sorryAx`. -/
theorem unfinished : 1 = 2 := by sorry

/-- A proof by compiled code, which rests on an axiom `native_decide` adds. -/
theorem compiled : 2 + 2 = 4 := by native_decide

/-- An axiom a source declared. -/
axiom declared : 1 = 2

/-- A proof that rests on the declared axiom. -/
theorem assumed : 1 = 2 := declared

end Canary
