//! Typed process-creation mitigation policy encoding.

/// The ordinary two-bit mitigation states used by the Windows SDK.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(kani, derive(kani::Arbitrary))]
#[repr(u64)]
pub enum Mitigation {
    /// Let the child executable and operating system choose.
    Defer = 0,
    /// Force the mitigation on.
    AlwaysOn = 1,
    /// Force the mitigation off.
    AlwaysOff = 2,
}

/// Mandatory-ASLR modes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(kani, derive(kani::Arbitrary))]
#[repr(u64)]
pub enum RelocateImages {
    /// Defer to the child.
    Defer = 0,
    /// Relocate images even when they are not dynamic-base compatible.
    AlwaysOn = 1,
    /// Do not force relocation.
    AlwaysOff = 2,
    /// Relocate images and reject images without relocation data.
    RequireRelocations = 3,
}

/// Dynamic-code policy modes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(kani, derive(kani::Arbitrary))]
#[repr(u64)]
pub enum DynamicCode {
    /// Defer to the child.
    Defer = 0,
    /// Prohibit dynamic code.
    Prohibit = 1,
    /// Do not prohibit dynamic code.
    Allow = 2,
    /// Prohibit dynamic code while allowing the child to opt out.
    ProhibitWithOptOut = 3,
}

/// Control Flow Guard modes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(kani, derive(kani::Arbitrary))]
#[repr(u64)]
pub enum ControlFlowGuard {
    /// Defer to the child.
    Defer = 0,
    /// Enable Control Flow Guard.
    AlwaysOn = 1,
    /// Disable Control Flow Guard.
    AlwaysOff = 2,
    /// Enable Control Flow Guard with export suppression.
    ExportSuppression = 3,
}

/// Microsoft-signed binary policy modes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(kani, derive(kani::Arbitrary))]
#[repr(u64)]
pub enum SignedBinaries {
    /// Defer to the child.
    Defer = 0,
    /// Permit only Microsoft-signed binaries.
    MicrosoftOnly = 1,
    /// Do not restrict binary signatures.
    AlwaysOff = 2,
    /// Permit Microsoft Store binaries in addition to Microsoft binaries.
    MicrosoftAndStore = 3,
}

/// Non-system-font policy modes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(kani, derive(kani::Arbitrary))]
#[repr(u64)]
pub enum FontDisable {
    /// Defer to the child.
    Defer = 0,
    /// Block non-system fonts.
    Block = 1,
    /// Allow non-system fonts.
    Allow = 2,
    /// Audit non-system font loads.
    Audit = 3,
}

/// Loader integrity continuity modes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(kani, derive(kani::Arbitrary))]
#[repr(u64)]
pub enum LoaderIntegrity {
    /// Defer to the child.
    Defer = 0,
    /// Enforce loader integrity continuity.
    AlwaysOn = 1,
    /// Disable enforcement.
    AlwaysOff = 2,
    /// Audit violations.
    Audit = 3,
}

/// Module-tampering protection modes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(kani, derive(kani::Arbitrary))]
#[repr(u64)]
pub enum ModuleTampering {
    /// Defer to the child.
    Defer = 0,
    /// Enable module-tampering protection.
    AlwaysOn = 1,
    /// Disable protection.
    AlwaysOff = 2,
    /// Enable protection without inheriting it into descendants.
    NoInherit = 3,
}

/// CET user shadow-stack modes.
///
/// Support depends on the Windows release, architecture, hardware, and child executable.
/// A representable value may still be rejected by the host.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(kani, derive(kani::Arbitrary))]
#[repr(u64)]
pub enum CetShadowStacks {
    /// Defer to the child.
    Defer = 0,
    /// Enable user shadow stacks.
    AlwaysOn = 1,
    /// Disable user shadow stacks.
    AlwaysOff = 2,
    /// Enable strict shadow-stack mode.
    Strict = 3,
}

/// CET set-context instruction-pointer validation modes.
///
/// Support depends on the Windows release, architecture, hardware, and child executable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(kani, derive(kani::Arbitrary))]
#[repr(u64)]
pub enum UserCetContextIpValidation {
    /// Defer to the child.
    Defer = 0,
    /// Enable validation.
    AlwaysOn = 1,
    /// Disable validation.
    AlwaysOff = 2,
    /// Enable relaxed validation.
    Relaxed = 3,
}

/// Modes for blocking binaries without CET or EH continuation metadata.
///
/// Support depends on the Windows release, architecture, and executable metadata.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(kani, derive(kani::Arbitrary))]
#[repr(u64)]
pub enum BlockNonCetBinaries {
    /// Defer to the child.
    Defer = 0,
    /// Block binaries without CET metadata.
    AlwaysOn = 1,
    /// Disable blocking.
    AlwaysOff = 2,
    /// Block binaries without EH continuation metadata.
    NonEhContinuation = 3,
}

/// The two-bit state that defers to the child.
const DEFER: u64 = 0b00;
/// The two-bit `ALWAYS_ON` state.
const ALWAYS_ON: u64 = 0b01;
/// The two-bit `ALWAYS_OFF` state.
const ALWAYS_OFF: u64 = 0b10;
/// The fourth two-bit state, whose meaning each field defines.
const FOURTH_STATE: u64 = 0b11;

/// Gives a two-bit policy field its encoding and its `Defer` default.
macro_rules! policy_field {
    ($name:ident { $($variant:ident => $bits:ident),+ $(,)? }) => {
        impl $name {
            const fn bits(self) -> u64 {
                match self {
                    $(Self::$variant => $bits,)+
                }
            }
        }

        impl Default for $name {
            /// Defers to the child.
            fn default() -> Self {
                Self::Defer
            }
        }
    };
}

policy_field!(Mitigation { Defer => DEFER, AlwaysOn => ALWAYS_ON, AlwaysOff => ALWAYS_OFF });
policy_field!(RelocateImages {
    Defer => DEFER,
    AlwaysOn => ALWAYS_ON,
    AlwaysOff => ALWAYS_OFF,
    RequireRelocations => FOURTH_STATE,
});
policy_field!(DynamicCode {
    Defer => DEFER,
    Prohibit => ALWAYS_ON,
    Allow => ALWAYS_OFF,
    ProhibitWithOptOut => FOURTH_STATE,
});
policy_field!(ControlFlowGuard {
    Defer => DEFER,
    AlwaysOn => ALWAYS_ON,
    AlwaysOff => ALWAYS_OFF,
    ExportSuppression => FOURTH_STATE,
});
policy_field!(SignedBinaries {
    Defer => DEFER,
    MicrosoftOnly => ALWAYS_ON,
    AlwaysOff => ALWAYS_OFF,
    MicrosoftAndStore => FOURTH_STATE,
});
policy_field!(FontDisable {
    Defer => DEFER,
    Block => ALWAYS_ON,
    Allow => ALWAYS_OFF,
    Audit => FOURTH_STATE,
});
policy_field!(LoaderIntegrity {
    Defer => DEFER,
    AlwaysOn => ALWAYS_ON,
    AlwaysOff => ALWAYS_OFF,
    Audit => FOURTH_STATE,
});
policy_field!(ModuleTampering {
    Defer => DEFER,
    AlwaysOn => ALWAYS_ON,
    AlwaysOff => ALWAYS_OFF,
    NoInherit => FOURTH_STATE,
});
policy_field!(CetShadowStacks {
    Defer => DEFER,
    AlwaysOn => ALWAYS_ON,
    AlwaysOff => ALWAYS_OFF,
    Strict => FOURTH_STATE,
});
policy_field!(UserCetContextIpValidation {
    Defer => DEFER,
    AlwaysOn => ALWAYS_ON,
    AlwaysOff => ALWAYS_OFF,
    Relaxed => FOURTH_STATE,
});
policy_field!(BlockNonCetBinaries {
    Defer => DEFER,
    AlwaysOn => ALWAYS_ON,
    AlwaysOff => ALWAYS_OFF,
    NonEhContinuation => FOURTH_STATE,
});

/// The word of a policy that holds a field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Word {
    First,
    Second,
}

/// One policy field: the word that holds it, its lowest bit, and the mask of its bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Field {
    word: Word,
    shift: u32,
    mask: u64,
}

/// The mask of a legacy one-bit field.
const ONE_BIT: u64 = 0b1;
/// The mask of a two-bit field.
const TWO_BITS: u64 = 0b11;

impl Field {
    const fn legacy(shift: u32) -> Self {
        Self {
            word: Word::First,
            shift,
            mask: ONE_BIT,
        }
    }

    const fn in_first(shift: u32) -> Self {
        Self {
            word: Word::First,
            shift,
            mask: TWO_BITS,
        }
    }

    const fn in_second(shift: u32) -> Self {
        Self {
            word: Word::Second,
            shift,
            mask: TWO_BITS,
        }
    }

    /// The bits the field occupies in its word.
    const fn bits(self) -> u64 {
        self.mask << self.shift
    }

    /// Replaces the field's bits in `word` with `value`, cut to the field's width.
    const fn replace(self, word: u64, value: u64) -> u64 {
        (word & !self.bits()) | ((value & self.mask) << self.shift)
    }
}

const DEP: Field = Field::legacy(0);
const DEP_ATL_THUNK: Field = Field::legacy(1);
const SEHOP: Field = Field::legacy(2);
const RELOCATE_IMAGES: Field = Field::in_first(8);
const HEAP_TERMINATE: Field = Field::in_first(12);
const BOTTOM_UP_ASLR: Field = Field::in_first(16);
const HIGH_ENTROPY_ASLR: Field = Field::in_first(20);
const STRICT_HANDLE_CHECKS: Field = Field::in_first(24);
const WIN32K_SYSTEM_CALLS: Field = Field::in_first(28);
const EXTENSION_POINTS: Field = Field::in_first(32);
const DYNAMIC_CODE: Field = Field::in_first(36);
const CONTROL_FLOW_GUARD: Field = Field::in_first(40);
const SIGNED_BINARIES: Field = Field::in_first(44);
const FONT_DISABLE: Field = Field::in_first(48);
const REMOTE_IMAGES: Field = Field::in_first(52);
const LOW_LABEL_IMAGES: Field = Field::in_first(56);
const PREFER_SYSTEM32_IMAGES: Field = Field::in_first(60);
const LOADER_INTEGRITY: Field = Field::in_second(4);
const STRICT_CONTROL_FLOW_GUARD: Field = Field::in_second(8);
const MODULE_TAMPERING: Field = Field::in_second(12);
const INDIRECT_BRANCH_PREDICTION: Field = Field::in_second(16);
const DYNAMIC_CODE_DOWNGRADE: Field = Field::in_second(20);
const SPECULATIVE_STORE_BYPASS: Field = Field::in_second(24);
const CET_SHADOW_STACKS: Field = Field::in_second(28);
const CET_CONTEXT_IP_VALIDATION: Field = Field::in_second(32);
const NON_CET_BINARIES: Field = Field::in_second(36);
const EXTENDED_CONTROL_FLOW_GUARD: Field = Field::in_second(40);
const POINTER_AUTHENTICATION: Field = Field::in_second(44);
const CET_DYNAMIC_APIS: Field = Field::in_second(48);
const CORE_SHARING: Field = Field::in_second(52);
const FSCTL_SYSTEM_CALLS: Field = Field::in_second(56);

/// Every field, in the order of the setters.
const FIELDS: [Field; 31] = [
    DEP,
    DEP_ATL_THUNK,
    SEHOP,
    RELOCATE_IMAGES,
    HEAP_TERMINATE,
    BOTTOM_UP_ASLR,
    HIGH_ENTROPY_ASLR,
    STRICT_HANDLE_CHECKS,
    WIN32K_SYSTEM_CALLS,
    EXTENSION_POINTS,
    DYNAMIC_CODE,
    CONTROL_FLOW_GUARD,
    SIGNED_BINARIES,
    FONT_DISABLE,
    REMOTE_IMAGES,
    LOW_LABEL_IMAGES,
    PREFER_SYSTEM32_IMAGES,
    LOADER_INTEGRITY,
    STRICT_CONTROL_FLOW_GUARD,
    MODULE_TAMPERING,
    INDIRECT_BRANCH_PREDICTION,
    DYNAMIC_CODE_DOWNGRADE,
    SPECULATIVE_STORE_BYPASS,
    CET_SHADOW_STACKS,
    CET_CONTEXT_IP_VALIDATION,
    NON_CET_BINARIES,
    EXTENDED_CONTROL_FLOW_GUARD,
    POINTER_AUTHENTICATION,
    CET_DYNAMIC_APIS,
    CORE_SHARING,
    FSCTL_SYSTEM_CALLS,
];

/// Returns true if every field fits its word and no two fields of one word share a bit.
const fn disjoint(fields: &[Field]) -> bool {
    let mut first = 0_u64;
    let mut second = 0_u64;
    let mut rest = fields;
    while let [field, tail @ ..] = rest {
        if field.shift >= u64::BITS || field.bits() >> field.shift != field.mask {
            return false;
        }
        let bits = field.bits();
        match field.word {
            Word::First => {
                if first & bits != 0 {
                    return false;
                }
                first |= bits;
            }
            Word::Second => {
                if second & bits != 0 {
                    return false;
                }
                second |= bits;
            }
        }
        rest = tail;
    }
    true
}

/// Fails the build unless every field fits its word and no two fields of one word share a bit.
///
/// `MitigationPolicy::with` names it, so every compiler evaluates it; Rust 1.75 counts no use inside `const _`.
const LAYOUT: () = assert!(
    disjoint(&FIELDS),
    "mitigation fields fit their words and share no bit"
);

/// The one-bit value of a legacy flag.
const fn flag(enable: bool) -> u64 {
    if enable {
        ONE_BIT
    } else {
        0
    }
}

/// A complete SDK 10.0.22621 process-creation mitigation policy.
///
/// Each setter replaces one field.
/// Reserved values and raw policy words cannot be represented.
///
/// # Runtime support
///
/// The fields mirror Windows SDK 10.0.22621; not every field works on every supported installation.
/// Support varies by policy, Windows release, architecture, hardware, and child executable.
/// Requested policies are not weakened; an unsupported one fails process creation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct MitigationPolicy {
    words: [u64; 2],
}

impl MitigationPolicy {
    /// Creates a policy that defers every field.
    #[must_use]
    pub const fn new() -> Self {
        Self { words: [0, 0] }
    }

    /// Returns the encoded words for diagnostics.
    #[must_use]
    pub const fn words(self) -> [u64; 2] {
        self.words
    }

    /// Replaces `field` in its word with `value`.
    const fn with(self, field: Field, value: u64) -> Self {
        let () = LAYOUT;
        let [first, second] = self.words;
        let words = match field.word {
            Word::First => [field.replace(first, value), second],
            Word::Second => [first, field.replace(second, value)],
        };
        Self { words }
    }

    /// Enables or removes the legacy DEP-enable bit.
    #[must_use]
    pub const fn dep(self, enable: bool) -> Self {
        self.with(DEP, flag(enable))
    }

    /// Enables or removes the legacy DEP ATL-thunk-emulation bit.
    #[must_use]
    pub const fn dep_atl_thunk(self, enable: bool) -> Self {
        self.with(DEP_ATL_THUNK, flag(enable))
    }

    /// Enables or removes the legacy SEHOP bit.
    #[must_use]
    pub const fn sehop(self, enable: bool) -> Self {
        self.with(SEHOP, flag(enable))
    }

    /// Sets mandatory image relocation.
    #[must_use]
    pub const fn relocate_images(self, value: RelocateImages) -> Self {
        self.with(RELOCATE_IMAGES, value.bits())
    }

    /// Sets heap termination on corruption.
    #[must_use]
    pub const fn heap_terminate(self, value: Mitigation) -> Self {
        self.with(HEAP_TERMINATE, value.bits())
    }

    /// Sets bottom-up ASLR.
    #[must_use]
    pub const fn bottom_up_aslr(self, value: Mitigation) -> Self {
        self.with(BOTTOM_UP_ASLR, value.bits())
    }

    /// Sets high-entropy ASLR.
    #[must_use]
    pub const fn high_entropy_aslr(self, value: Mitigation) -> Self {
        self.with(HIGH_ENTROPY_ASLR, value.bits())
    }

    /// Sets strict invalid-handle checking.
    #[must_use]
    pub const fn strict_handle_checks(self, value: Mitigation) -> Self {
        self.with(STRICT_HANDLE_CHECKS, value.bits())
    }

    /// Sets the Win32k system-call-disable mitigation.
    #[must_use]
    pub const fn disable_win32k_system_calls(self, value: Mitigation) -> Self {
        self.with(WIN32K_SYSTEM_CALLS, value.bits())
    }

    /// Sets extension-point disabling.
    #[must_use]
    pub const fn disable_extension_points(self, value: Mitigation) -> Self {
        self.with(EXTENSION_POINTS, value.bits())
    }

    /// Sets dynamic-code policy.
    #[must_use]
    pub const fn dynamic_code(self, value: DynamicCode) -> Self {
        self.with(DYNAMIC_CODE, value.bits())
    }

    /// Sets Control Flow Guard policy.
    #[must_use]
    pub const fn control_flow_guard(self, value: ControlFlowGuard) -> Self {
        self.with(CONTROL_FLOW_GUARD, value.bits())
    }

    /// Sets signed-binary loading policy.
    #[must_use]
    pub const fn signed_binaries(self, value: SignedBinaries) -> Self {
        self.with(SIGNED_BINARIES, value.bits())
    }

    /// Sets non-system-font policy.
    #[must_use]
    pub const fn font_disable(self, value: FontDisable) -> Self {
        self.with(FONT_DISABLE, value.bits())
    }

    /// Sets remote-image blocking.
    #[must_use]
    pub const fn block_remote_images(self, value: Mitigation) -> Self {
        self.with(REMOTE_IMAGES, value.bits())
    }

    /// Sets low-integrity-label image blocking.
    #[must_use]
    pub const fn block_low_label_images(self, value: Mitigation) -> Self {
        self.with(LOW_LABEL_IMAGES, value.bits())
    }

    /// Sets System32 image preference.
    #[must_use]
    pub const fn prefer_system32_images(self, value: Mitigation) -> Self {
        self.with(PREFER_SYSTEM32_IMAGES, value.bits())
    }

    /// Sets loader integrity continuity.
    #[must_use]
    pub const fn loader_integrity(self, value: LoaderIntegrity) -> Self {
        self.with(LOADER_INTEGRITY, value.bits())
    }

    /// Sets strict Control Flow Guard.
    #[must_use]
    pub const fn strict_control_flow_guard(self, value: Mitigation) -> Self {
        self.with(STRICT_CONTROL_FLOW_GUARD, value.bits())
    }

    /// Sets module-tampering protection.
    #[must_use]
    pub const fn module_tampering(self, value: ModuleTampering) -> Self {
        self.with(MODULE_TAMPERING, value.bits())
    }

    /// Sets restricted indirect branch prediction.
    #[must_use]
    pub const fn restrict_indirect_branch_prediction(self, value: Mitigation) -> Self {
        self.with(INDIRECT_BRANCH_PREDICTION, value.bits())
    }

    /// Sets permission for a broker to downgrade dynamic-code policy.
    #[must_use]
    pub const fn allow_downgrade_dynamic_code(self, value: Mitigation) -> Self {
        self.with(DYNAMIC_CODE_DOWNGRADE, value.bits())
    }

    /// Sets speculative-store-bypass disabling.
    #[must_use]
    pub const fn disable_speculative_store_bypass(self, value: Mitigation) -> Self {
        self.with(SPECULATIVE_STORE_BYPASS, value.bits())
    }

    /// Sets CET user shadow stacks.
    #[must_use]
    pub const fn cet_user_shadow_stacks(self, value: CetShadowStacks) -> Self {
        self.with(CET_SHADOW_STACKS, value.bits())
    }

    /// Sets CET set-context instruction-pointer validation.
    #[must_use]
    pub const fn user_cet_context_ip_validation(self, value: UserCetContextIpValidation) -> Self {
        self.with(CET_CONTEXT_IP_VALIDATION, value.bits())
    }

    /// Sets blocking of binaries without CET metadata.
    #[must_use]
    pub const fn block_non_cet_binaries(self, value: BlockNonCetBinaries) -> Self {
        self.with(NON_CET_BINARIES, value.bits())
    }

    /// Sets extended Control Flow Guard.
    #[must_use]
    pub const fn extended_control_flow_guard(self, value: Mitigation) -> Self {
        self.with(EXTENDED_CONTROL_FLOW_GUARD, value.bits())
    }

    /// Sets ARM64 user-mode instruction-pointer authentication.
    #[must_use]
    pub const fn pointer_authentication(self, value: Mitigation) -> Self {
        self.with(POINTER_AUTHENTICATION, value.bits())
    }

    /// Sets CET dynamic APIs to out-of-process-only mode.
    #[must_use]
    pub const fn cet_dynamic_apis_out_of_process(self, value: Mitigation) -> Self {
        self.with(CET_DYNAMIC_APIS, value.bits())
    }

    /// Sets restricted CPU-core sharing.
    #[must_use]
    pub const fn restrict_core_sharing(self, value: Mitigation) -> Self {
        self.with(CORE_SHARING, value.bits())
    }

    /// Sets FSCTL system-call disabling.
    #[must_use]
    pub const fn disable_fsctl_system_calls(self, value: Mitigation) -> Self {
        self.with(FSCTL_SYSTEM_CALLS, value.bits())
    }
}

#[cfg(kani)]
mod proofs {
    use super::*;

    /// Applies the setter `selector` picks to `policy` with an arbitrary value.
    ///
    /// Returns the result, the field the setter writes, and the value the caller passed, read from its declared discriminant.
    fn set(policy: MitigationPolicy, selector: usize) -> (MitigationPolicy, Field, u64) {
        macro_rules! legacy {
            ($setter:ident, $field:ident) => {{
                let enable: bool = kani::any();
                (policy.$setter(enable), $field, u64::from(enable))
            }};
        }
        macro_rules! two_bit {
            ($setter:ident, $kind:ty, $field:ident) => {{
                let value: $kind = kani::any();
                assert_eq!(
                    value.bits(),
                    value as u64,
                    "the encoding is the declared discriminant"
                );
                (policy.$setter(value), $field, value as u64)
            }};
        }
        match selector {
            0 => legacy!(dep, DEP),
            1 => legacy!(dep_atl_thunk, DEP_ATL_THUNK),
            2 => legacy!(sehop, SEHOP),
            3 => two_bit!(relocate_images, RelocateImages, RELOCATE_IMAGES),
            4 => two_bit!(heap_terminate, Mitigation, HEAP_TERMINATE),
            5 => two_bit!(bottom_up_aslr, Mitigation, BOTTOM_UP_ASLR),
            6 => two_bit!(high_entropy_aslr, Mitigation, HIGH_ENTROPY_ASLR),
            7 => two_bit!(strict_handle_checks, Mitigation, STRICT_HANDLE_CHECKS),
            8 => two_bit!(disable_win32k_system_calls, Mitigation, WIN32K_SYSTEM_CALLS),
            9 => two_bit!(disable_extension_points, Mitigation, EXTENSION_POINTS),
            10 => two_bit!(dynamic_code, DynamicCode, DYNAMIC_CODE),
            11 => two_bit!(control_flow_guard, ControlFlowGuard, CONTROL_FLOW_GUARD),
            12 => two_bit!(signed_binaries, SignedBinaries, SIGNED_BINARIES),
            13 => two_bit!(font_disable, FontDisable, FONT_DISABLE),
            14 => two_bit!(block_remote_images, Mitigation, REMOTE_IMAGES),
            15 => two_bit!(block_low_label_images, Mitigation, LOW_LABEL_IMAGES),
            16 => two_bit!(prefer_system32_images, Mitigation, PREFER_SYSTEM32_IMAGES),
            17 => two_bit!(loader_integrity, LoaderIntegrity, LOADER_INTEGRITY),
            18 => two_bit!(
                strict_control_flow_guard,
                Mitigation,
                STRICT_CONTROL_FLOW_GUARD
            ),
            19 => two_bit!(module_tampering, ModuleTampering, MODULE_TAMPERING),
            20 => two_bit!(
                restrict_indirect_branch_prediction,
                Mitigation,
                INDIRECT_BRANCH_PREDICTION
            ),
            21 => two_bit!(
                allow_downgrade_dynamic_code,
                Mitigation,
                DYNAMIC_CODE_DOWNGRADE
            ),
            22 => two_bit!(
                disable_speculative_store_bypass,
                Mitigation,
                SPECULATIVE_STORE_BYPASS
            ),
            23 => two_bit!(cet_user_shadow_stacks, CetShadowStacks, CET_SHADOW_STACKS),
            24 => two_bit!(
                user_cet_context_ip_validation,
                UserCetContextIpValidation,
                CET_CONTEXT_IP_VALIDATION
            ),
            25 => two_bit!(
                block_non_cet_binaries,
                BlockNonCetBinaries,
                NON_CET_BINARIES
            ),
            26 => two_bit!(
                extended_control_flow_guard,
                Mitigation,
                EXTENDED_CONTROL_FLOW_GUARD
            ),
            27 => two_bit!(pointer_authentication, Mitigation, POINTER_AUTHENTICATION),
            28 => two_bit!(
                cet_dynamic_apis_out_of_process,
                Mitigation,
                CET_DYNAMIC_APIS
            ),
            29 => two_bit!(restrict_core_sharing, Mitigation, CORE_SHARING),
            _ => two_bit!(disable_fsctl_system_calls, Mitigation, FSCTL_SYSTEM_CALLS),
        }
    }

    fn read(policy: MitigationPolicy, field: Field) -> (u64, u64) {
        let [first, second] = policy.words();
        match field.word {
            Word::First => (first, second),
            Word::Second => (second, first),
        }
    }

    #[kani::proof]
    fn every_setter_writes_exactly_its_field() {
        let before = MitigationPolicy { words: kani::any() };
        let selector: usize = kani::any();
        kani::assume(selector < FIELDS.len());
        let (after, field, value) = set(before, selector);
        assert_eq!(field, FIELDS[selector]);
        let (written, kept) = read(after, field);
        let (original, untouched) = read(before, field);
        assert_eq!(kept, untouched);
        assert_eq!(written & !field.bits(), original & !field.bits());
        assert_eq!((written >> field.shift) & field.mask, value);
        assert_eq!(value & !field.mask, 0);
        kani::cover!(selector == 0 && value == 1);
        kani::cover!(selector == FIELDS.len() - 1 && value == ALWAYS_OFF);
        kani::cover!(field.mask == TWO_BITS && value == FOURTH_STATE && original == u64::MAX);
    }

    #[kani::proof]
    fn a_new_policy_defers_every_field() {
        let selector: usize = kani::any();
        kani::assume(selector < FIELDS.len());
        let field = FIELDS[selector];
        let (word, other) = read(MitigationPolicy::new(), field);
        assert_eq!((word >> field.shift) & field.mask, DEFER);
        assert_eq!(other, 0);
        assert_eq!(MitigationPolicy::new(), MitigationPolicy::default());
    }

    #[kani::proof]
    fn negative_control_write_without_clearing() {
        let word: u64 = kani::any();
        let value: u64 = kani::any();
        kani::assume(value <= TWO_BITS);
        let written = word | (value << RELOCATE_IMAGES.shift);
        assert!(
            (written >> RELOCATE_IMAGES.shift) & TWO_BITS == value,
            "NEGATIVE_MITIGATION_CONTROL"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setters_replace_only_their_field() {
        let first = MitigationPolicy::new()
            .dynamic_code(DynamicCode::ProhibitWithOptOut)
            .font_disable(FontDisable::Audit)
            .dynamic_code(DynamicCode::Allow);
        assert_eq!(first.words(), [(2_u64 << 36) | (3_u64 << 48), 0]);

        let second = MitigationPolicy::new()
            .cet_user_shadow_stacks(CetShadowStacks::Strict)
            .block_non_cet_binaries(BlockNonCetBinaries::NonEhContinuation);
        assert_eq!(second.words(), [0, (3_u64 << 28) | (3_u64 << 36)]);
    }

    #[test]
    fn legacy_bits_do_not_touch_two_bit_fields() {
        let policy = MitigationPolicy::new()
            .relocate_images(RelocateImages::RequireRelocations)
            .dep(true)
            .dep_atl_thunk(true)
            .sehop(true);
        assert_eq!(policy.words()[0], 7 | (3_u64 << 8));

        let cleared = policy.dep(false).dep_atl_thunk(false).sehop(false);
        assert_eq!(cleared.words()[0], 3_u64 << 8);
        assert_eq!(MitigationPolicy::new().dep(true).dep(true).words()[0], 1);
    }

    /// One setter applied to a policy.
    type Setter = fn(MitigationPolicy) -> MitigationPolicy;

    /// Each SDK 10.0.22621 field's setter, with the word, shift, and value `winnt.h` gives the state it sets.
    ///
    /// The test calls each setter, rather than a constant holding the policies, so coverage counts them.
    const SDK_22621: [(Setter, usize, u32, u64); 31] = [
        (|policy| policy.dep(true), 0, 0, 1),
        (|policy| policy.dep_atl_thunk(true), 0, 1, 1),
        (|policy| policy.sehop(true), 0, 2, 1),
        (
            |policy| policy.relocate_images(RelocateImages::RequireRelocations),
            0,
            8,
            3,
        ),
        (
            |policy| policy.heap_terminate(Mitigation::AlwaysOn),
            0,
            12,
            1,
        ),
        (
            |policy| policy.bottom_up_aslr(Mitigation::AlwaysOn),
            0,
            16,
            1,
        ),
        (
            |policy| policy.high_entropy_aslr(Mitigation::AlwaysOn),
            0,
            20,
            1,
        ),
        (
            |policy| policy.strict_handle_checks(Mitigation::AlwaysOn),
            0,
            24,
            1,
        ),
        (
            |policy| policy.disable_win32k_system_calls(Mitigation::AlwaysOn),
            0,
            28,
            1,
        ),
        (
            |policy| policy.disable_extension_points(Mitigation::AlwaysOn),
            0,
            32,
            1,
        ),
        (
            |policy| policy.dynamic_code(DynamicCode::ProhibitWithOptOut),
            0,
            36,
            3,
        ),
        (
            |policy| policy.control_flow_guard(ControlFlowGuard::ExportSuppression),
            0,
            40,
            3,
        ),
        (
            |policy| policy.signed_binaries(SignedBinaries::MicrosoftAndStore),
            0,
            44,
            3,
        ),
        (|policy| policy.font_disable(FontDisable::Audit), 0, 48, 3),
        (
            |policy| policy.block_remote_images(Mitigation::AlwaysOn),
            0,
            52,
            1,
        ),
        (
            |policy| policy.block_low_label_images(Mitigation::AlwaysOn),
            0,
            56,
            1,
        ),
        (
            |policy| policy.prefer_system32_images(Mitigation::AlwaysOn),
            0,
            60,
            1,
        ),
        (
            |policy| policy.loader_integrity(LoaderIntegrity::Audit),
            1,
            4,
            3,
        ),
        (
            |policy| policy.strict_control_flow_guard(Mitigation::AlwaysOn),
            1,
            8,
            1,
        ),
        (
            |policy| policy.module_tampering(ModuleTampering::NoInherit),
            1,
            12,
            3,
        ),
        (
            |policy| policy.restrict_indirect_branch_prediction(Mitigation::AlwaysOn),
            1,
            16,
            1,
        ),
        (
            |policy| policy.allow_downgrade_dynamic_code(Mitigation::AlwaysOn),
            1,
            20,
            1,
        ),
        (
            |policy| policy.disable_speculative_store_bypass(Mitigation::AlwaysOn),
            1,
            24,
            1,
        ),
        (
            |policy| policy.cet_user_shadow_stacks(CetShadowStacks::Strict),
            1,
            28,
            3,
        ),
        (
            |policy| policy.user_cet_context_ip_validation(UserCetContextIpValidation::Relaxed),
            1,
            32,
            3,
        ),
        (
            |policy| policy.block_non_cet_binaries(BlockNonCetBinaries::NonEhContinuation),
            1,
            36,
            3,
        ),
        (
            |policy| policy.extended_control_flow_guard(Mitigation::AlwaysOn),
            1,
            40,
            1,
        ),
        (
            |policy| policy.pointer_authentication(Mitigation::AlwaysOn),
            1,
            44,
            1,
        ),
        (
            |policy| policy.cet_dynamic_apis_out_of_process(Mitigation::AlwaysOn),
            1,
            48,
            1,
        ),
        (
            |policy| policy.restrict_core_sharing(Mitigation::AlwaysOn),
            1,
            52,
            1,
        ),
        (
            |policy| policy.disable_fsctl_system_calls(Mitigation::AlwaysOn),
            1,
            56,
            1,
        ),
    ];

    #[test]
    fn every_sdk_22621_field_has_the_expected_encoding() {
        for (set, word, shift, value) in SDK_22621 {
            let mut expected = [0_u64; 2];
            expected[word] = value << shift;
            assert_eq!(set(MitigationPolicy::new()).words(), expected);
        }
    }
}
