//! Process creation mitigation policies.

use std::ops::{BitOr, BitOrAssign};

/// A set of process creation mitigations
/// (`PROC_THREAD_ATTRIBUTE_MITIGATION_POLICY`).
///
/// Win32 models this as a `DWORD64` of two-bit fields — each mitigation can be
/// `DEFER` (0), `ALWAYS_ON` (1) or `ALWAYS_OFF` (2). `spawnkit` exposes it as an
/// opaque set rather than a raw integer or a `bitflags` type, for two reasons:
///
/// * the encoding is not a flat bitmask, so `bitflags`' set semantics would be
///   subtly wrong (`ALWAYS_ON | ALWAYS_OFF` for the same mitigation is a
///   contradiction, not a union);
/// * keeping the representation private means the second policy word
///   (`cbSize == 16`, the `..._POLICY2` constants) can be added later without a
///   breaking change.
///
/// Only the `ALWAYS_ON` half of each pair is exposed today; `ALWAYS_OFF`
/// constants and the second policy word are future work. Constants match
/// `PROCESS_CREATION_MITIGATION_POLICY_*` in `WinBase.h`.
///
/// Note that mitigations are applied by the kernel at process creation and
/// cannot be relaxed afterwards. Turning on
/// [`NO_DYNAMIC_CODE`](Self::NO_DYNAMIC_CODE) for a child that JITs will not
/// produce a helpful error message — it will produce a crash inside the JIT.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct MitigationPolicy {
    /// The `DWORD64` handed to `UpdateProcThreadAttribute`.
    bits: u64,
}

impl MitigationPolicy {
    /// No mitigations requested; the child gets the system defaults.
    pub const NONE: Self = Self { bits: 0 };

    /// `PROCESS_CREATION_MITIGATION_POLICY_PROHIBIT_DYNAMIC_CODE_ALWAYS_ON`.
    ///
    /// The child may not allocate executable memory or make existing pages
    /// executable. Fatal to JIT compilers, which is the point.
    pub const NO_DYNAMIC_CODE: Self = Self { bits: 1 << 36 };

    /// `PROCESS_CREATION_MITIGATION_POLICY_BLOCK_NON_MICROSOFT_BINARIES_ALWAYS_ON`.
    ///
    /// Blocks images that are not Microsoft-signed from loading into the child.
    /// Effective against injected shims and third-party hooking DLLs.
    pub const BLOCK_NON_MICROSOFT_BINARIES: Self = Self { bits: 1 << 44 };

    /// `PROCESS_CREATION_MITIGATION_POLICY_IMAGE_LOAD_NO_REMOTE_ALWAYS_ON`.
    ///
    /// The child may not load images from a UNC path.
    pub const NO_REMOTE_IMAGE_LOAD: Self = Self { bits: 1 << 52 };

    /// `PROCESS_CREATION_MITIGATION_POLICY_EXTENSION_POINT_DISABLE_ALWAYS_ON`.
    ///
    /// Blocks legacy extension points — AppInit DLLs, window hooks, IMEs — from
    /// injecting into the child.
    pub const EXTENSION_POINT_DISABLE: Self = Self { bits: 1 << 32 };

    /// `PROCESS_CREATION_MITIGATION_POLICY_WIN32K_SYSTEM_CALL_DISABLE_ALWAYS_ON`.
    ///
    /// Removes the `win32k.sys` attack surface. Only usable by children that
    /// never touch USER32/GDI32.
    pub const WIN32K_SYSTEM_CALL_DISABLE: Self = Self { bits: 1 << 28 };

    /// `PROCESS_CREATION_MITIGATION_POLICY_STRICT_HANDLE_CHECKS_ALWAYS_ON`.
    ///
    /// Turns an invalid handle reference in the child into an immediate
    /// exception instead of a silently ignored error.
    pub const STRICT_HANDLE_CHECKS: Self = Self { bits: 1 << 24 };

    /// `PROCESS_CREATION_MITIGATION_POLICY_CONTROL_FLOW_GUARD_ALWAYS_ON`.
    ///
    /// Requires the child's images to be CFG-instrumented; images that are not
    /// will fail to load.
    pub const CONTROL_FLOW_GUARD: Self = Self { bits: 1 << 40 };

    /// The raw policy word, as `UpdateProcThreadAttribute` wants it.
    pub fn bits(self) -> u64 {
        todo!("return the raw policy word")
    }

    /// Whether every mitigation in `other` is present in `self`.
    pub fn contains(self, other: Self) -> bool {
        todo!("bitwise containment test")
    }

    /// The union of two policy sets.
    pub fn union(self, other: Self) -> Self {
        todo!("bitwise or")
    }
}

impl BitOr for MitigationPolicy {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        todo!("self.union(rhs)")
    }
}

impl BitOrAssign for MitigationPolicy {
    fn bitor_assign(&mut self, rhs: Self) {
        todo!("*self = self.union(rhs)")
    }
}
