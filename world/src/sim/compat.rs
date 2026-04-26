use super::FileOpts;
use crate::recipe::{
    CompatAuditV1, CompatEntryKindV1, CompatFailureDetailV1, CompatFailureKindV1,
    CompatFailureSubjectV1,
};
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CompatMode {
    #[default]
    Record,
    Enforce,
}

impl CompatMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Record => "record",
            Self::Enforce => "enforce",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct RawCompatFailure {
    pub kind: CompatFailureKindV1,
    pub subject: CompatFailureSubjectV1,
    pub detail: CompatFailureDetailV1,
}

impl RawCompatFailure {
    pub const fn new(kind: CompatFailureKindV1) -> Self {
        Self {
            kind,
            subject: CompatFailureSubjectV1::None,
            detail: CompatFailureDetailV1 {
                legacy_world_version: false,
                world_size_mismatch: false,
                world_scale_mismatch: false,
            },
        }
    }

    pub const fn structured(
        kind: CompatFailureKindV1,
        subject: CompatFailureSubjectV1,
        detail: CompatFailureDetailV1,
    ) -> Self {
        Self {
            kind,
            subject,
            detail,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RawLoadOutcome<T> {
    GenerateRequested,
    Loaded(T),
    Failed(RawCompatFailure),
    Rejected(RawCompatFailure),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct CompatResolved<T> {
    pub parsed_world_file: Option<T>,
    pub compat_audit: CompatAuditV1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct CompatResolveError {
    pub audit: CompatAuditV1,
}

impl fmt::Display for CompatResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "compat load rejected: entry={}, decision={}, failure={}, resolution={}, subject={}, \
             legacy_world_version={}, world_size_mismatch={}, world_scale_mismatch={}",
            self.audit.entry.as_str(),
            self.audit.decision.as_str(),
            self.audit.failure_kind.as_str(),
            self.audit.resolution.as_str(),
            self.audit.failure_subject.as_str(),
            self.audit.failure_detail.legacy_world_version,
            self.audit.failure_detail.world_size_mismatch,
            self.audit.failure_detail.world_scale_mismatch
        )
    }
}

impl std::error::Error for CompatResolveError {}

pub(super) fn entry_kind(file_opts: &FileOpts) -> CompatEntryKindV1 {
    match file_opts {
        FileOpts::Generate(_) => CompatEntryKindV1::Generate,
        FileOpts::Save(_, _) => CompatEntryKindV1::Save,
        FileOpts::LoadOrGenerate { .. } => CompatEntryKindV1::LoadOrGenerate,
        FileOpts::LoadLegacy(_) => CompatEntryKindV1::LoadLegacy,
        FileOpts::Load(_) => CompatEntryKindV1::Load,
        FileOpts::LoadAsset(_) => CompatEntryKindV1::LoadAsset,
    }
}

pub(super) fn resolve<T>(
    mode: CompatMode,
    entry: CompatEntryKindV1,
    outcome: RawLoadOutcome<T>,
) -> Result<CompatResolved<T>, CompatResolveError> {
    let compat_audit = match outcome {
        RawLoadOutcome::GenerateRequested => CompatAuditV1::generate_requested(entry),
        RawLoadOutcome::Loaded(_) => CompatAuditV1::loaded_existing(entry),
        RawLoadOutcome::Failed(failure) => CompatAuditV1::fallback_generate_with_detail(
            entry,
            failure.kind,
            failure.subject,
            failure.detail,
        ),
        RawLoadOutcome::Rejected(failure) => {
            CompatAuditV1::reject(entry, failure.kind, failure.subject, failure.detail)
        },
    };

    if compat_audit.is_rejected() {
        return Err(CompatResolveError {
            audit: compat_audit,
        });
    }

    if matches!(mode, CompatMode::Enforce) && compat_audit.is_strict_load_contract_gap() {
        return Err(CompatResolveError {
            audit: compat_audit,
        });
    }

    Ok(CompatResolved {
        parsed_world_file: match outcome {
            RawLoadOutcome::Loaded(world_file) => Some(world_file),
            RawLoadOutcome::GenerateRequested
            | RawLoadOutcome::Failed(_)
            | RawLoadOutcome::Rejected(_) => None,
        },
        compat_audit,
    })
}

#[cfg(test)]
mod tests {
    use super::{CompatMode, RawCompatFailure, RawLoadOutcome, entry_kind, resolve};
    use crate::{
        recipe::{
            CompatDecisionV1, CompatEntryKindV1, CompatFailureDetailV1, CompatFailureKindV1,
            CompatFailureSubjectV1,
        },
        sim::{FileOpts, GenOpts},
    };
    use std::path::PathBuf;

    #[test]
    fn entry_kind_tracks_file_opts_variant() {
        assert_eq!(
            entry_kind(&FileOpts::LoadAsset("world.map.test".to_owned())),
            CompatEntryKindV1::LoadAsset
        );
        assert_eq!(
            entry_kind(&FileOpts::Save(
                PathBuf::from("map.bin"),
                GenOpts::default()
            )),
            CompatEntryKindV1::Save
        );
    }

    #[test]
    fn record_mode_keeps_strict_load_fallback_observable() {
        let resolved = resolve(
            CompatMode::Record,
            CompatEntryKindV1::Load,
            RawLoadOutcome::<u8>::Failed(RawCompatFailure::new(CompatFailureKindV1::ParseError)),
        )
        .expect("record mode should preserve current fallback behavior");

        assert_eq!(resolved.parsed_world_file, None);
        assert_eq!(
            resolved.compat_audit.decision,
            CompatDecisionV1::FallbackGenerate
        );
        assert!(resolved.compat_audit.is_strict_load_contract_gap());
    }

    #[test]
    fn enforce_mode_rejects_strict_load_fallback() {
        let err = resolve(
            CompatMode::Enforce,
            CompatEntryKindV1::LoadAsset,
            RawLoadOutcome::<u8>::Failed(RawCompatFailure::new(CompatFailureKindV1::MissingInput)),
        )
        .expect_err("enforce mode should reject strict load fallback");

        assert_eq!(err.audit.decision, CompatDecisionV1::FallbackGenerate);
        assert!(err.audit.is_strict_load_contract_gap());
    }

    #[test]
    fn enforce_mode_keeps_load_or_generate_recovery() {
        let resolved = resolve(
            CompatMode::Enforce,
            CompatEntryKindV1::LoadOrGenerate,
            RawLoadOutcome::<u8>::Failed(RawCompatFailure::new(CompatFailureKindV1::MissingInput)),
        )
        .expect("load_or_generate should remain recoverable under enforce");

        assert_eq!(
            resolved.compat_audit.decision,
            CompatDecisionV1::FallbackGenerate
        );
        assert!(!resolved.compat_audit.is_strict_load_contract_gap());
    }

    #[test]
    fn record_mode_rejects_structured_load_or_generate_mismatch() {
        let err = resolve(
            CompatMode::Record,
            CompatEntryKindV1::LoadOrGenerate,
            RawLoadOutcome::<u8>::Rejected(RawCompatFailure::structured(
                CompatFailureKindV1::OptionMismatch,
                CompatFailureSubjectV1::Options,
                CompatFailureDetailV1::option_mismatch(true, false),
            )),
        )
        .expect_err("structured mismatch should reject without entering fallback generation");

        assert!(err.audit.is_rejected());
        assert_eq!(err.audit.failure_subject, CompatFailureSubjectV1::Options);
        assert!(err.audit.failure_detail.world_size_mismatch);
        assert!(!err.audit.failure_detail.world_scale_mismatch);
    }
}
