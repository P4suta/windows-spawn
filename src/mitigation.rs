use crate::core_logic::{replace_one_bit_field as set_legacy, replace_two_bit_field as replace};

/// Data Execution Prevention policy.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u64)]
pub enum DepPolicy {
    /// Defer to the executable and operating system.
    #[default]
    Defer = 0,
    /// Enable Data Execution Prevention.
    Enable = 1,
}

/// DEP ATL thunk-emulation policy.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u64)]
pub enum AtlThunkPolicy {
    /// Defer to the executable and operating system.
    #[default]
    Defer = 0,
    /// Disable ATL thunk emulation.
    Disable = 1,
}

/// Structured Exception Handler Overwrite Protection policy.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u64)]
pub enum SehopPolicy {
    /// Defer to the executable and operating system.
    #[default]
    Defer = 0,
    /// Enable SEHOP.
    Enable = 1,
}

/// The ordinary two-bit mitigation states used by the Windows SDK.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u64)]
pub enum Mitigation {
    /// Let the child executable and operating system choose.
    #[default]
    Defer = 0,
    /// Force the mitigation on.
    AlwaysOn = 1,
    /// Force the mitigation off.
    AlwaysOff = 2,
}

/// Mandatory-ASLR modes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u64)]
pub enum RelocateImages {
    /// Defer to the child.
    #[default]
    Defer = 0,
    /// Relocate images even when they are not dynamic-base compatible.
    AlwaysOn = 1,
    /// Do not force relocation.
    AlwaysOff = 2,
    /// Relocate images and reject images without relocation data.
    RequireRelocations = 3,
}

/// Dynamic-code policy modes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u64)]
pub enum DynamicCode {
    /// Defer to the child.
    #[default]
    Defer = 0,
    /// Prohibit dynamic code.
    Prohibit = 1,
    /// Do not prohibit dynamic code.
    Allow = 2,
    /// Prohibit dynamic code while allowing the child to opt out.
    ProhibitWithOptOut = 3,
}

/// Control Flow Guard modes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u64)]
pub enum ControlFlowGuard {
    /// Defer to the child.
    #[default]
    Defer = 0,
    /// Enable Control Flow Guard.
    AlwaysOn = 1,
    /// Disable Control Flow Guard.
    AlwaysOff = 2,
    /// Enable Control Flow Guard with export suppression.
    ExportSuppression = 3,
}

/// Microsoft-signed binary policy modes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u64)]
pub enum SignedBinaries {
    /// Defer to the child.
    #[default]
    Defer = 0,
    /// Permit only Microsoft-signed binaries.
    MicrosoftOnly = 1,
    /// Do not restrict binary signatures.
    AlwaysOff = 2,
    /// Permit Microsoft Store binaries in addition to Microsoft binaries.
    MicrosoftAndStore = 3,
}

/// Non-system-font policy modes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u64)]
pub enum FontDisable {
    /// Defer to the child.
    #[default]
    Defer = 0,
    /// Block non-system fonts.
    Block = 1,
    /// Allow non-system fonts.
    Allow = 2,
    /// Audit non-system font loads.
    Audit = 3,
}

/// Loader integrity continuity modes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u64)]
pub enum LoaderIntegrity {
    /// Defer to the child.
    #[default]
    Defer = 0,
    /// Enforce loader integrity continuity.
    AlwaysOn = 1,
    /// Disable enforcement.
    AlwaysOff = 2,
    /// Audit violations.
    Audit = 3,
}

/// Module-tampering protection modes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u64)]
pub enum ModuleTampering {
    /// Defer to the child.
    #[default]
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
/// Runtime support depends on the Windows release, processor architecture,
/// hardware capabilities, and child executable. Representability here does
/// not imply that the current host accepts the policy.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u64)]
pub enum CetShadowStacks {
    /// Defer to the child.
    #[default]
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
/// Runtime support depends on the Windows release, processor architecture,
/// hardware capabilities, and child executable.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u64)]
pub enum UserCetContextIpValidation {
    /// Defer to the child.
    #[default]
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
/// Runtime support depends on the Windows release, processor architecture,
/// and executable metadata.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u64)]
pub enum BlockNonCetBinaries {
    /// Defer to the child.
    #[default]
    Defer = 0,
    /// Block binaries without CET metadata.
    AlwaysOn = 1,
    /// Disable blocking.
    AlwaysOff = 2,
    /// Block binaries without EH continuation metadata.
    NonEhContinuation = 3,
}

macro_rules! mitigation_bits {
    ($type:ty, $($variant:path => $bits:literal),+ $(,)?) => {
        impl $type {
            const fn bits(self) -> u64 {
                match self {
                    $($variant => $bits,)+
                }
            }
        }
    };
}

mitigation_bits!(DepPolicy, DepPolicy::Defer => 0, DepPolicy::Enable => 1);
mitigation_bits!(AtlThunkPolicy, AtlThunkPolicy::Defer => 0, AtlThunkPolicy::Disable => 1);
mitigation_bits!(SehopPolicy, SehopPolicy::Defer => 0, SehopPolicy::Enable => 1);
mitigation_bits!(
    Mitigation,
    Mitigation::Defer => 0,
    Mitigation::AlwaysOn => 1,
    Mitigation::AlwaysOff => 2,
);
mitigation_bits!(
    RelocateImages,
    RelocateImages::Defer => 0,
    RelocateImages::AlwaysOn => 1,
    RelocateImages::AlwaysOff => 2,
    RelocateImages::RequireRelocations => 3,
);
mitigation_bits!(
    DynamicCode,
    DynamicCode::Defer => 0,
    DynamicCode::Prohibit => 1,
    DynamicCode::Allow => 2,
    DynamicCode::ProhibitWithOptOut => 3,
);
mitigation_bits!(
    ControlFlowGuard,
    ControlFlowGuard::Defer => 0,
    ControlFlowGuard::AlwaysOn => 1,
    ControlFlowGuard::AlwaysOff => 2,
    ControlFlowGuard::ExportSuppression => 3,
);
mitigation_bits!(
    SignedBinaries,
    SignedBinaries::Defer => 0,
    SignedBinaries::MicrosoftOnly => 1,
    SignedBinaries::AlwaysOff => 2,
    SignedBinaries::MicrosoftAndStore => 3,
);
mitigation_bits!(
    FontDisable,
    FontDisable::Defer => 0,
    FontDisable::Block => 1,
    FontDisable::Allow => 2,
    FontDisable::Audit => 3,
);
mitigation_bits!(
    LoaderIntegrity,
    LoaderIntegrity::Defer => 0,
    LoaderIntegrity::AlwaysOn => 1,
    LoaderIntegrity::AlwaysOff => 2,
    LoaderIntegrity::Audit => 3,
);
mitigation_bits!(
    ModuleTampering,
    ModuleTampering::Defer => 0,
    ModuleTampering::AlwaysOn => 1,
    ModuleTampering::AlwaysOff => 2,
    ModuleTampering::NoInherit => 3,
);
mitigation_bits!(
    CetShadowStacks,
    CetShadowStacks::Defer => 0,
    CetShadowStacks::AlwaysOn => 1,
    CetShadowStacks::AlwaysOff => 2,
    CetShadowStacks::Strict => 3,
);
mitigation_bits!(
    UserCetContextIpValidation,
    UserCetContextIpValidation::Defer => 0,
    UserCetContextIpValidation::AlwaysOn => 1,
    UserCetContextIpValidation::AlwaysOff => 2,
    UserCetContextIpValidation::Relaxed => 3,
);
mitigation_bits!(
    BlockNonCetBinaries,
    BlockNonCetBinaries::Defer => 0,
    BlockNonCetBinaries::AlwaysOn => 1,
    BlockNonCetBinaries::AlwaysOff => 2,
    BlockNonCetBinaries::NonEhContinuation => 3,
);

/// A complete SDK 10.0.22621 process-creation mitigation policy.
///
/// Setters replace one field. Reserved values and combined raw policy words
/// cannot be represented.
///
/// # Runtime support
///
/// This type mirrors the policy fields in Windows SDK 10.0.22621; it is not a
/// claim that every field works on every supported Windows installation.
/// Availability varies by individual policy, Windows release, processor
/// architecture, hardware, and child executable. windows-spawn does not weaken
/// a requested policy. Unsupported policies return the process-creation error.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct MitigationPolicy {
    words: MitigationWords,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
struct MitigationWords {
    first: u64,
    second: u64,
}

impl MitigationPolicy {
    /// Creates a policy which defers every field.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            words: MitigationWords {
                first: 0,
                second: 0,
            },
        }
    }

    /// Returns the encoded words for diagnostics.
    #[must_use]
    pub const fn words(self) -> [u64; 2] {
        [self.words.first, self.words.second]
    }

    /// Sets the legacy DEP policy.
    #[must_use]
    pub const fn dep(mut self, policy: DepPolicy) -> Self {
        self.words.first = set_legacy(self.words.first, 0, policy.bits());
        self
    }

    /// Sets the legacy DEP ATL-thunk-emulation policy.
    #[must_use]
    pub const fn dep_atl_thunk(mut self, policy: AtlThunkPolicy) -> Self {
        self.words.first = set_legacy(self.words.first, 1, policy.bits());
        self
    }

    /// Sets the legacy SEHOP policy.
    #[must_use]
    pub const fn sehop(mut self, policy: SehopPolicy) -> Self {
        self.words.first = set_legacy(self.words.first, 2, policy.bits());
        self
    }

    /// Sets mandatory image relocation.
    #[must_use]
    pub const fn relocate_images(mut self, value: RelocateImages) -> Self {
        self.words.first = replace(self.words.first, 8, value.bits());
        self
    }

    /// Sets heap termination on corruption.
    #[must_use]
    pub const fn heap_terminate(mut self, value: Mitigation) -> Self {
        self.words.first = replace(self.words.first, 12, value.bits());
        self
    }

    /// Sets bottom-up ASLR.
    #[must_use]
    pub const fn bottom_up_aslr(mut self, value: Mitigation) -> Self {
        self.words.first = replace(self.words.first, 16, value.bits());
        self
    }

    /// Sets high-entropy ASLR.
    #[must_use]
    pub const fn high_entropy_aslr(mut self, value: Mitigation) -> Self {
        self.words.first = replace(self.words.first, 20, value.bits());
        self
    }

    /// Sets strict invalid-handle checking.
    #[must_use]
    pub const fn strict_handle_checks(mut self, value: Mitigation) -> Self {
        self.words.first = replace(self.words.first, 24, value.bits());
        self
    }

    /// Sets the Win32k system-call-disable mitigation.
    #[must_use]
    pub const fn disable_win32k_system_calls(mut self, value: Mitigation) -> Self {
        self.words.first = replace(self.words.first, 28, value.bits());
        self
    }

    /// Sets extension-point disabling.
    #[must_use]
    pub const fn disable_extension_points(mut self, value: Mitigation) -> Self {
        self.words.first = replace(self.words.first, 32, value.bits());
        self
    }

    /// Sets dynamic-code policy.
    #[must_use]
    pub const fn dynamic_code(mut self, value: DynamicCode) -> Self {
        self.words.first = replace(self.words.first, 36, value.bits());
        self
    }

    /// Sets Control Flow Guard policy.
    #[must_use]
    pub const fn control_flow_guard(mut self, value: ControlFlowGuard) -> Self {
        self.words.first = replace(self.words.first, 40, value.bits());
        self
    }

    /// Sets signed-binary loading policy.
    #[must_use]
    pub const fn signed_binaries(mut self, value: SignedBinaries) -> Self {
        self.words.first = replace(self.words.first, 44, value.bits());
        self
    }

    /// Sets non-system-font policy.
    #[must_use]
    pub const fn font_disable(mut self, value: FontDisable) -> Self {
        self.words.first = replace(self.words.first, 48, value.bits());
        self
    }

    /// Sets remote-image blocking.
    #[must_use]
    pub const fn block_remote_images(mut self, value: Mitigation) -> Self {
        self.words.first = replace(self.words.first, 52, value.bits());
        self
    }

    /// Sets low-integrity-label image blocking.
    #[must_use]
    pub const fn block_low_label_images(mut self, value: Mitigation) -> Self {
        self.words.first = replace(self.words.first, 56, value.bits());
        self
    }

    /// Sets System32 image preference.
    #[must_use]
    pub const fn prefer_system32_images(mut self, value: Mitigation) -> Self {
        self.words.first = replace(self.words.first, 60, value.bits());
        self
    }

    /// Sets loader integrity continuity.
    #[must_use]
    pub const fn loader_integrity(mut self, value: LoaderIntegrity) -> Self {
        self.words.second = replace(self.words.second, 4, value.bits());
        self
    }

    /// Sets strict Control Flow Guard.
    #[must_use]
    pub const fn strict_control_flow_guard(mut self, value: Mitigation) -> Self {
        self.words.second = replace(self.words.second, 8, value.bits());
        self
    }

    /// Sets module-tampering protection.
    #[must_use]
    pub const fn module_tampering(mut self, value: ModuleTampering) -> Self {
        self.words.second = replace(self.words.second, 12, value.bits());
        self
    }

    /// Sets restricted indirect branch prediction.
    #[must_use]
    pub const fn restrict_indirect_branch_prediction(mut self, value: Mitigation) -> Self {
        self.words.second = replace(self.words.second, 16, value.bits());
        self
    }

    /// Sets permission for a broker to downgrade dynamic-code policy.
    #[must_use]
    pub const fn allow_downgrade_dynamic_code(mut self, value: Mitigation) -> Self {
        self.words.second = replace(self.words.second, 20, value.bits());
        self
    }

    /// Sets speculative-store-bypass disabling.
    #[must_use]
    pub const fn disable_speculative_store_bypass(mut self, value: Mitigation) -> Self {
        self.words.second = replace(self.words.second, 24, value.bits());
        self
    }

    /// Sets CET user shadow stacks.
    #[must_use]
    pub const fn cet_user_shadow_stacks(mut self, value: CetShadowStacks) -> Self {
        self.words.second = replace(self.words.second, 28, value.bits());
        self
    }

    /// Sets CET set-context instruction-pointer validation.
    #[must_use]
    pub const fn user_cet_context_ip_validation(
        mut self,
        value: UserCetContextIpValidation,
    ) -> Self {
        self.words.second = replace(self.words.second, 32, value.bits());
        self
    }

    /// Sets blocking of binaries without CET metadata.
    #[must_use]
    pub const fn block_non_cet_binaries(mut self, value: BlockNonCetBinaries) -> Self {
        self.words.second = replace(self.words.second, 36, value.bits());
        self
    }

    /// Sets extended Control Flow Guard.
    #[must_use]
    pub const fn extended_control_flow_guard(mut self, value: Mitigation) -> Self {
        self.words.second = replace(self.words.second, 40, value.bits());
        self
    }

    /// Sets ARM64 user-mode instruction-pointer authentication.
    #[must_use]
    pub const fn pointer_authentication(mut self, value: Mitigation) -> Self {
        self.words.second = replace(self.words.second, 44, value.bits());
        self
    }

    /// Sets CET dynamic APIs to out-of-process-only mode.
    #[must_use]
    pub const fn cet_dynamic_apis_out_of_process(mut self, value: Mitigation) -> Self {
        self.words.second = replace(self.words.second, 48, value.bits());
        self
    }

    /// Sets restricted CPU-core sharing.
    #[must_use]
    pub const fn restrict_core_sharing(mut self, value: Mitigation) -> Self {
        self.words.second = replace(self.words.second, 52, value.bits());
        self
    }

    /// Sets FSCTL system-call disabling.
    #[must_use]
    pub const fn disable_fsctl_system_calls(mut self, value: Mitigation) -> Self {
        self.words.second = replace(self.words.second, 56, value.bits());
        self
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    macro_rules! field {
        ($policy:expr, $word:expr, $shift:expr, $value:expr) => {{
            let mut expected = [0_u64; 2];
            expected[$word] = ($value as u64) << $shift;
            assert_eq!($policy.words(), expected);
        }};
    }

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
            .dep(DepPolicy::Enable)
            .dep_atl_thunk(AtlThunkPolicy::Disable)
            .sehop(SehopPolicy::Enable);
        assert_eq!(policy.words()[0], 7 | (3_u64 << 8));

        let cleared = policy
            .dep(DepPolicy::Defer)
            .dep_atl_thunk(AtlThunkPolicy::Defer)
            .sehop(SehopPolicy::Defer);
        assert_eq!(cleared.words()[0], 3_u64 << 8);
        assert_eq!(
            MitigationPolicy::new()
                .dep(DepPolicy::Enable)
                .dep(DepPolicy::Enable)
                .words()[0],
            1
        );
    }

    #[test]
    fn first_sdk_22621_word_has_the_expected_encoding() {
        field!(MitigationPolicy::new().dep(DepPolicy::Enable), 0, 0, 1);
        field!(
            MitigationPolicy::new().dep_atl_thunk(AtlThunkPolicy::Disable),
            0,
            1,
            1
        );
        field!(MitigationPolicy::new().sehop(SehopPolicy::Enable), 0, 2, 1);
        field!(
            MitigationPolicy::new().relocate_images(RelocateImages::RequireRelocations),
            0,
            8,
            3
        );
        field!(
            MitigationPolicy::new().heap_terminate(Mitigation::AlwaysOn),
            0,
            12,
            1
        );
        field!(
            MitigationPolicy::new().bottom_up_aslr(Mitigation::AlwaysOn),
            0,
            16,
            1
        );
        field!(
            MitigationPolicy::new().high_entropy_aslr(Mitigation::AlwaysOn),
            0,
            20,
            1
        );
        field!(
            MitigationPolicy::new().strict_handle_checks(Mitigation::AlwaysOn),
            0,
            24,
            1
        );
        field!(
            MitigationPolicy::new().disable_win32k_system_calls(Mitigation::AlwaysOn),
            0,
            28,
            1
        );
        field!(
            MitigationPolicy::new().disable_extension_points(Mitigation::AlwaysOn),
            0,
            32,
            1
        );
        field!(
            MitigationPolicy::new().dynamic_code(DynamicCode::ProhibitWithOptOut),
            0,
            36,
            3
        );
        field!(
            MitigationPolicy::new().control_flow_guard(ControlFlowGuard::ExportSuppression),
            0,
            40,
            3
        );
        field!(
            MitigationPolicy::new().signed_binaries(SignedBinaries::MicrosoftAndStore),
            0,
            44,
            3
        );
        field!(
            MitigationPolicy::new().font_disable(FontDisable::Audit),
            0,
            48,
            3
        );
        field!(
            MitigationPolicy::new().block_remote_images(Mitigation::AlwaysOn),
            0,
            52,
            1
        );
        field!(
            MitigationPolicy::new().block_low_label_images(Mitigation::AlwaysOn),
            0,
            56,
            1
        );
        field!(
            MitigationPolicy::new().prefer_system32_images(Mitigation::AlwaysOn),
            0,
            60,
            1
        );
    }

    #[test]
    fn second_sdk_22621_word_has_the_expected_encoding() {
        field!(
            MitigationPolicy::new().loader_integrity(LoaderIntegrity::Audit),
            1,
            4,
            3
        );
        field!(
            MitigationPolicy::new().strict_control_flow_guard(Mitigation::AlwaysOn),
            1,
            8,
            1
        );
        field!(
            MitigationPolicy::new().module_tampering(ModuleTampering::NoInherit),
            1,
            12,
            3
        );
        field!(
            MitigationPolicy::new().restrict_indirect_branch_prediction(Mitigation::AlwaysOn),
            1,
            16,
            1
        );
        field!(
            MitigationPolicy::new().allow_downgrade_dynamic_code(Mitigation::AlwaysOn),
            1,
            20,
            1
        );
        field!(
            MitigationPolicy::new().disable_speculative_store_bypass(Mitigation::AlwaysOn),
            1,
            24,
            1
        );
        field!(
            MitigationPolicy::new().cet_user_shadow_stacks(CetShadowStacks::Strict),
            1,
            28,
            3
        );
        field!(
            MitigationPolicy::new()
                .user_cet_context_ip_validation(UserCetContextIpValidation::Relaxed),
            1,
            32,
            3
        );
        field!(
            MitigationPolicy::new().block_non_cet_binaries(BlockNonCetBinaries::NonEhContinuation),
            1,
            36,
            3
        );
        field!(
            MitigationPolicy::new().extended_control_flow_guard(Mitigation::AlwaysOn),
            1,
            40,
            1
        );
        field!(
            MitigationPolicy::new().pointer_authentication(Mitigation::AlwaysOn),
            1,
            44,
            1
        );
        field!(
            MitigationPolicy::new().cet_dynamic_apis_out_of_process(Mitigation::AlwaysOn),
            1,
            48,
            1
        );
        field!(
            MitigationPolicy::new().restrict_core_sharing(Mitigation::AlwaysOn),
            1,
            52,
            1
        );
        field!(
            MitigationPolicy::new().disable_fsctl_system_calls(Mitigation::AlwaysOn),
            1,
            56,
            1
        );
    }
}
