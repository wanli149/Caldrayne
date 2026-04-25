use super::reports::*;

pub(super) fn external_record_field_contract(
    name: &'static str,
    value_kind: &'static str,
    evidence_source: &'static str,
    semantics: &'static str,
) -> ExternalRecordFieldContract {
    ExternalRecordFieldContract {
        name,
        value_kind,
        evidence_source,
        semantics,
    }
}

pub(super) fn external_record_field_names(
    contracts: &[ExternalRecordFieldContract],
) -> Vec<&'static str> {
    contracts.iter().map(|contract| contract.name).collect()
}

pub(super) fn template_field(
    name: &'static str,
    placeholder: &'static str,
    completion_rule: &'static str,
) -> ExternalRecordTemplateField {
    ExternalRecordTemplateField {
        name,
        placeholder,
        completion_rule,
    }
}

pub(super) fn example_field(
    name: &'static str,
    value: &'static str,
    rationale: &'static str,
) -> ExternalRecordExampleField {
    ExternalRecordExampleField {
        name,
        value,
        rationale,
    }
}

pub(super) fn workflow_step(
    sequence: u8,
    action: &'static str,
    owner: &'static str,
    evidence_source: &'static str,
    record_effect: &'static str,
    completion_record_fields: Vec<&'static str>,
    blocking_until_complete: bool,
) -> ExternalRecordWorkflowStep {
    ExternalRecordWorkflowStep {
        sequence,
        action,
        owner,
        evidence_source,
        record_effect,
        completion_record_fields,
        blocking_until_complete,
    }
}

pub(super) fn authority_pairing_check(
    id: &'static str,
    review_fields: Vec<&'static str>,
    evidence_sources: Vec<&'static str>,
    required_match: &'static str,
    release_blocking_on_mismatch: bool,
) -> ExternalRecordAuthorityPairingCheck {
    ExternalRecordAuthorityPairingCheck {
        id,
        review_fields,
        evidence_sources,
        required_match,
        release_blocking_on_mismatch,
    }
}

pub(super) fn external_section_snapshot_field_contract(
    name: &'static str,
    value_kind: &'static str,
    semantics: &'static str,
) -> ExternalSectionSnapshotFieldContract {
    ExternalSectionSnapshotFieldContract {
        name,
        value_kind,
        semantics,
    }
}

fn snapshot_field_contract_from_record_field(
    contract: &ExternalRecordFieldContract,
) -> ExternalSectionSnapshotFieldContract {
    external_section_snapshot_field_contract(contract.name, contract.value_kind, contract.semantics)
}

fn review_record_runtime_field_value_kind(name: &'static str) -> &'static str {
    match name {
        "archive_reference" => "string",
        "archived_at_utc" => "utc-timestamp",
        "archived_by" => "string",
        "source_record_state" => "enum-string",
        "post_archive_verified_by" => "string",
        "post_archive_verified_at_utc" => "utc-timestamp",
        "post_archive_verification_result" => "enum-string",
        "post_archive_verification_reference" => "string",
        _ => "string",
    }
}

fn review_record_runtime_field_semantics(signal: &'static str, name: &'static str) -> &'static str {
    match name {
        "archive_reference" | "archived_at_utc" | "archived_by" => {
            review_record_field_completion_rule(signal, name)
        },
        "source_record_state" => {
            "record which terminal result_status the archive handoff captured from the same \
             section so archive receipt validation can verify that the live section and archived \
             terminal snapshot stayed aligned"
        },
        "post_archive_verified_by"
        | "post_archive_verified_at_utc"
        | "post_archive_verification_result"
        | "post_archive_verification_reference" => {
            review_record_field_completion_rule(signal, name)
        },
        _ => "record the field value captured from the authoritative external section snapshot",
    }
}

fn push_unique_snapshot_field(
    fields: &mut Vec<ExternalSectionSnapshotFieldContract>,
    field: ExternalSectionSnapshotFieldContract,
) {
    if !fields.iter().any(|existing| existing.name == field.name) {
        fields.push(field);
    }
}

fn collect_stage_scoped_snapshot_fields(
    signal: &'static str,
    required_decision_field_contracts: &[ExternalRecordFieldContract],
    exception_record_field_contracts: &[ExternalRecordFieldContract],
    archive_handoff_contract: &ExternalRecordArchiveHandoffContract,
    post_archive_writeback_fields: &[&'static str],
) -> Vec<ExternalSectionSnapshotFieldContract> {
    let mut fields = Vec::new();
    let always_present_names = [
        "release_reference",
        "reviewed_by",
        "decision_recorded_at_utc",
        "result_status",
    ];

    for contract in required_decision_field_contracts {
        if !always_present_names.contains(&contract.name) {
            push_unique_snapshot_field(
                &mut fields,
                snapshot_field_contract_from_record_field(contract),
            );
        }
    }
    for contract in exception_record_field_contracts {
        push_unique_snapshot_field(
            &mut fields,
            snapshot_field_contract_from_record_field(contract),
        );
    }
    for field in &archive_handoff_contract.required_archive_receipt_fields {
        push_unique_snapshot_field(
            &mut fields,
            external_section_snapshot_field_contract(
                field,
                review_record_runtime_field_value_kind(field),
                review_record_runtime_field_semantics(signal, field),
            ),
        );
    }
    push_unique_snapshot_field(
        &mut fields,
        external_section_snapshot_field_contract(
            "source_record_state",
            review_record_runtime_field_value_kind("source_record_state"),
            review_record_runtime_field_semantics(signal, "source_record_state"),
        ),
    );
    for field in post_archive_writeback_fields {
        push_unique_snapshot_field(
            &mut fields,
            external_section_snapshot_field_contract(
                field,
                review_record_runtime_field_value_kind(field),
                review_record_runtime_field_semantics(signal, field),
            ),
        );
    }

    fields
}

fn collect_snapshot_field_value_contracts(
    signal: &'static str,
    required_decision_field_contracts: &[ExternalRecordFieldContract],
    exception_record_field_contracts: &[ExternalRecordFieldContract],
    archive_handoff_contract: &ExternalRecordArchiveHandoffContract,
    post_archive_writeback_fields: &[&'static str],
) -> Vec<ExternalSectionSnapshotFieldContract> {
    let mut fields = Vec::new();
    for field in external_section_snapshot_input_contract(
        signal,
        required_decision_field_contracts,
        exception_record_field_contracts,
        archive_handoff_contract,
        post_archive_writeback_fields,
        Vec::new(),
    )
    .always_present_field_values
    {
        push_unique_snapshot_field(&mut fields, field);
    }
    for field in collect_stage_scoped_snapshot_fields(
        signal,
        required_decision_field_contracts,
        exception_record_field_contracts,
        archive_handoff_contract,
        post_archive_writeback_fields,
    ) {
        push_unique_snapshot_field(&mut fields, field);
    }

    fields
}

fn external_section_snapshot_input_contract(
    signal: &'static str,
    required_decision_field_contracts: &[ExternalRecordFieldContract],
    exception_record_field_contracts: &[ExternalRecordFieldContract],
    archive_handoff_contract: &ExternalRecordArchiveHandoffContract,
    post_archive_writeback_fields: &[&'static str],
    notes: Vec<&'static str>,
) -> ExternalSectionSnapshotInputContract {
    let always_present_names = [
        "release_reference",
        "reviewed_by",
        "decision_recorded_at_utc",
        "result_status",
    ];
    let always_present_field_values = always_present_names
        .iter()
        .filter_map(|name| {
            required_decision_field_contracts
                .iter()
                .find(|contract| contract.name == *name)
                .map(snapshot_field_contract_from_record_field)
        })
        .collect::<Vec<_>>();

    ExternalSectionSnapshotInputContract {
        snapshot_kind: "external-release-review-section-snapshot-v1",
        object_scope: "one extracted authoritative section snapshot used as validator input for \
                       the external release-review-record",
        required_top_level_fields: vec![
            external_section_snapshot_field_contract(
                "record_kind",
                "const-string",
                "must stay release-review-record so the validator does not accidentally consume a \
                 different tracker object kind",
            ),
            external_section_snapshot_field_contract(
                "section_signal",
                "const-string",
                "must stay aligned with the concrete section contract being validated",
            ),
            external_section_snapshot_field_contract(
                "field_values",
                "string-keyed-object",
                "captures the current field snapshot from the authoritative external review \
                 section; stage-specific required keys are evaluated by the validation contract",
            ),
        ],
        optional_top_level_fields: vec![external_section_snapshot_field_contract(
            "prior_result_statuses",
            "ordered-enum-string-array",
            "records prior result_status values from the same section when lifecycle history is \
             needed to prove that rolled-back only occurred after a prior approved terminal state",
        )],
        field_values_key: "field_values",
        always_present_field_values,
        stage_scoped_field_values: collect_stage_scoped_snapshot_fields(
            signal,
            required_decision_field_contracts,
            exception_record_field_contracts,
            archive_handoff_contract,
            post_archive_writeback_fields,
        ),
        prior_result_statuses_key: "prior_result_statuses",
        prior_result_statuses_required_for_states: vec!["rolled-back"],
        notes,
    }
}

fn snapshot_field_value_placeholder(signal: &'static str, name: &'static str) -> &'static str {
    match name {
        "archive_reference" => "<archive-object-or-ticket-reference>",
        "archived_at_utc" => "<utc-timestamp>",
        "archived_by" => "<operator-or-automation-id>",
        "source_record_state" => "<terminal-result-status>",
        "post_archive_verified_by" => "<release-operator-id>",
        "post_archive_verified_at_utc" => "<utc-timestamp>",
        "post_archive_verification_result" => "<verified|needs-follow-up>",
        "post_archive_verification_reference" => "<archive-review-note-or-ticket>",
        _ => review_record_field_placeholder(signal, name),
    }
}

fn snapshot_field_value_completion_rule(signal: &'static str, name: &'static str) -> &'static str {
    match name {
        "archive_reference" | "archived_at_utc" | "archived_by" => {
            review_record_runtime_field_semantics(signal, name)
        },
        "source_record_state" => {
            "record the terminal result_status that the archive receipt captured from the same \
             section"
        },
        "post_archive_verified_by"
        | "post_archive_verified_at_utc"
        | "post_archive_verification_result"
        | "post_archive_verification_reference" => {
            review_record_runtime_field_semantics(signal, name)
        },
        _ => review_record_field_completion_rule(signal, name),
    }
}

fn snapshot_template_field_value_entries(
    signal: &'static str,
    required_decision_field_contracts: &[ExternalRecordFieldContract],
    exception_record_field_contracts: &[ExternalRecordFieldContract],
    archive_handoff_contract: &ExternalRecordArchiveHandoffContract,
    post_archive_writeback_fields: &[&'static str],
) -> Vec<ExternalRecordTemplateField> {
    collect_snapshot_field_value_contracts(
        signal,
        required_decision_field_contracts,
        exception_record_field_contracts,
        archive_handoff_contract,
        post_archive_writeback_fields,
    )
    .into_iter()
    .map(|field| {
        template_field(
            field.name,
            snapshot_field_value_placeholder(signal, field.name),
            snapshot_field_value_completion_rule(signal, field.name),
        )
    })
    .collect()
}

fn snapshot_template_contract(
    signal: &'static str,
    required_decision_field_contracts: &[ExternalRecordFieldContract],
    exception_record_field_contracts: &[ExternalRecordFieldContract],
    archive_handoff_contract: &ExternalRecordArchiveHandoffContract,
    post_archive_writeback_fields: &[&'static str],
    notes: Vec<&'static str>,
) -> ExternalSectionSnapshotTemplateContract {
    ExternalSectionSnapshotTemplateContract {
        snapshot_kind: "external-release-review-section-snapshot-v1",
        top_level_fields: vec![
            template_field(
                "record_kind",
                "release-review-record",
                "must stay exactly release-review-record",
            ),
            template_field(
                "section_signal",
                signal,
                "must stay exactly aligned with the section contract being validated",
            ),
            template_field(
                "field_values",
                "<object keyed by published section field names>",
                "embed the current external section field snapshot under this object key",
            ),
            template_field(
                "prior_result_statuses",
                "<[prior-result-status,...]-when-required>",
                "include only when the current result_status requires lifecycle history proof, \
                 such as rolled-back",
            ),
        ],
        field_value_entries: snapshot_template_field_value_entries(
            signal,
            required_decision_field_contracts,
            exception_record_field_contracts,
            archive_handoff_contract,
            post_archive_writeback_fields,
        ),
        notes,
    }
}

fn snapshot_example_terminal_result_status(signal: &'static str) -> &'static str {
    match signal {
        "public-entry-handoff" => "rolled-back",
        _ => "approved",
    }
}

fn snapshot_example_prior_result_statuses(signal: &'static str) -> Option<&'static str> {
    match signal {
        "public-entry-handoff" => Some("[\"cutover-approved\"]"),
        _ => None,
    }
}

fn snapshot_field_value_example(
    signal: &'static str,
    name: &'static str,
) -> (&'static str, &'static str) {
    match name {
        "result_status" => (
            snapshot_example_terminal_result_status(signal),
            "illustrative current section lifecycle state in the snapshot input",
        ),
        "archive_reference" => (
            "archive://release-review/2026-05-01/public-entry-handoff-terminal",
            "illustrative archive receipt reference",
        ),
        "archived_at_utc" => (
            "2026-05-01T08:55:00Z",
            "illustrative archive handoff timestamp",
        ),
        "archived_by" => (
            "ops-release-automation",
            "illustrative archive handoff actor",
        ),
        "source_record_state" => (
            snapshot_example_terminal_result_status(signal),
            "illustrative source terminal result_status captured during archive handoff",
        ),
        "post_archive_verified_by" => (
            "ops-release-owner",
            "illustrative post-archive verification owner",
        ),
        "post_archive_verified_at_utc" => (
            "2026-05-01T09:10:00Z",
            "illustrative post-archive verification timestamp",
        ),
        "post_archive_verification_result" => {
            ("verified", "illustrative post-archive verification outcome")
        },
        "post_archive_verification_reference" => (
            "note://archive-review/release-2026-05-01",
            "illustrative archive verification note reference",
        ),
        _ => (
            review_record_example_value(signal, name),
            review_record_example_rationale(signal, name),
        ),
    }
}

fn snapshot_example_field_value_entries(
    signal: &'static str,
    required_decision_field_contracts: &[ExternalRecordFieldContract],
    exception_record_field_contracts: &[ExternalRecordFieldContract],
    archive_handoff_contract: &ExternalRecordArchiveHandoffContract,
    post_archive_writeback_fields: &[&'static str],
) -> Vec<ExternalRecordExampleField> {
    collect_snapshot_field_value_contracts(
        signal,
        required_decision_field_contracts,
        exception_record_field_contracts,
        archive_handoff_contract,
        post_archive_writeback_fields,
    )
    .into_iter()
    .map(|field| {
        let (value, rationale) = snapshot_field_value_example(signal, field.name);
        example_field(field.name, value, rationale)
    })
    .collect()
}

fn snapshot_example_contract(
    signal: &'static str,
    required_decision_field_contracts: &[ExternalRecordFieldContract],
    exception_record_field_contracts: &[ExternalRecordFieldContract],
    archive_handoff_contract: &ExternalRecordArchiveHandoffContract,
    post_archive_writeback_fields: &[&'static str],
    notes: Vec<&'static str>,
) -> ExternalSectionSnapshotExampleContract {
    let mut top_level_fields = vec![
        example_field(
            "record_kind",
            "release-review-record",
            "illustrative fixed snapshot kind target",
        ),
        example_field(
            "section_signal",
            signal,
            "illustrative fixed section identity for this snapshot contract",
        ),
        example_field(
            "field_values",
            "{...see field_value_entries...}",
            "the nested field snapshot is expanded below as field_value_entries for readability",
        ),
    ];
    if let Some(prior_statuses) = snapshot_example_prior_result_statuses(signal) {
        top_level_fields.push(example_field(
            "prior_result_statuses",
            prior_statuses,
            "illustrative lifecycle history proof for rolled-back validation input",
        ));
    }

    ExternalSectionSnapshotExampleContract {
        snapshot_kind: "external-release-review-section-snapshot-v1",
        illustrative_only: true,
        top_level_fields,
        field_value_entries: snapshot_example_field_value_entries(
            signal,
            required_decision_field_contracts,
            exception_record_field_contracts,
            archive_handoff_contract,
            post_archive_writeback_fields,
        ),
        notes,
    }
}

fn validation_result_field_contract(
    name: &'static str,
    value_kind: &'static str,
    semantics: &'static str,
) -> ExternalSectionValidationResultFieldContract {
    ExternalSectionValidationResultFieldContract {
        name,
        value_kind,
        semantics,
    }
}

fn validation_result_contract(
    signal: &'static str,
    notes: Vec<&'static str>,
) -> ExternalSectionValidationResultContract {
    ExternalSectionValidationResultContract {
        result_kind: "external-section-validation-result-v1",
        object_scope: "one validator output object describing whether a concrete external review \
                       section snapshot satisfies the published section instance validation \
                       contract",
        required_fields: vec![
            validation_result_field_contract(
                "result_kind",
                "const-string",
                "must stay external-section-validation-result-v1",
            ),
            validation_result_field_contract(
                "section_signal",
                "const-string",
                "identifies which release-review section contract was evaluated",
            ),
            validation_result_field_contract(
                "evaluated_result_status",
                "enum-string",
                "echoes the current result_status carried by the evaluated section snapshot",
            ),
            validation_result_field_contract(
                "stage_status",
                "enum-string",
                "reports whether the evaluated snapshot is valid, incomplete, or invalid for the \
                 claimed stage/result_status",
            ),
            validation_result_field_contract(
                "highest_satisfied_stage",
                "enum-string",
                "reports the highest validation stage that the snapshot currently satisfies",
            ),
            validation_result_field_contract(
                "blocking",
                "boolean",
                "true when the reported outcome means release approval or audit closure must stop \
                 at the current stage",
            ),
            validation_result_field_contract(
                "summary",
                "string",
                "short machine-consumable summary of the validation outcome",
            ),
        ],
        optional_fields: vec![
            validation_result_field_contract(
                "missing_required_fields",
                "string-array",
                "lists field names missing from the evaluated snapshot for the claimed stage",
            ),
            validation_result_field_contract(
                "failed_additional_checks",
                "string-array",
                "lists stage-level additional checks that the evaluated snapshot did not satisfy",
            ),
            validation_result_field_contract(
                "failed_authority_pairing_check_ids",
                "string-array",
                "lists authority pairing checks that failed for the evaluated snapshot",
            ),
            validation_result_field_contract(
                "forbidden_shortcut_hits",
                "string-array",
                "lists shortcut rules violated by the evaluated snapshot or evaluation flow",
            ),
            validation_result_field_contract(
                "forbidden_post_terminal_mutation_hits",
                "string-array",
                "lists forbidden post-terminal mutations detected on the evaluated snapshot",
            ),
            validation_result_field_contract(
                "next_blocking_stage",
                "enum-string",
                "identifies the next stage that still blocks release approval or audit closure",
            ),
            validation_result_field_contract(
                "evaluated_stage",
                "enum-string",
                "optionally records the stage the validator attempted to prove directly",
            ),
        ],
        stage_status_field: "stage_status",
        notes: {
            let mut merged = vec![
                "stage_status should be interpreted against the published validation stages for \
                 this section contract",
                "highest_satisfied_stage may be lower than evaluated_stage when the snapshot does \
                 not yet satisfy the requested stage",
            ];
            if signal == "public-entry-handoff" {
                merged.push(
                    "failed_authority_pairing_check_ids is especially relevant for \
                     public-entry-handoff because bundle/runtime authority mismatches are \
                     release-blocking",
                );
            }
            merged.extend(notes);
            merged
        },
    }
}

fn validation_result_example(
    signal: &'static str,
    notes: Vec<&'static str>,
) -> ExternalSectionValidationResultExampleContract {
    let highest_satisfied_stage = match signal {
        "public-entry-handoff" => "post-archive-verified",
        _ => "post-archive-verified",
    };
    let evaluated_result_status = match signal {
        "public-entry-handoff" => "rolled-back",
        _ => "approved",
    };

    ExternalSectionValidationResultExampleContract {
        result_kind: "external-section-validation-result-v1",
        illustrative_only: true,
        fields: vec![
            example_field(
                "result_kind",
                "external-section-validation-result-v1",
                "illustrative fixed validator result kind",
            ),
            example_field(
                "section_signal",
                signal,
                "illustrative section identity under validation",
            ),
            example_field(
                "evaluated_result_status",
                evaluated_result_status,
                "illustrative current section result_status copied from the evaluated snapshot",
            ),
            example_field(
                "evaluated_stage",
                highest_satisfied_stage,
                "illustrative requested validation stage",
            ),
            example_field(
                "stage_status",
                "valid",
                "illustrative validator outcome for a fully satisfied snapshot",
            ),
            example_field(
                "highest_satisfied_stage",
                highest_satisfied_stage,
                "illustrative highest stage the sample snapshot satisfies",
            ),
            example_field(
                "blocking",
                "false",
                "illustrative non-blocking result because the sample snapshot is valid",
            ),
            example_field(
                "summary",
                "snapshot satisfies all published requirements for the evaluated stage",
                "illustrative summary text only",
            ),
            example_field(
                "missing_required_fields",
                "[]",
                "illustrative empty list because the sample snapshot is complete",
            ),
            example_field(
                "failed_additional_checks",
                "[]",
                "illustrative empty list because the sample snapshot satisfies stage checks",
            ),
            example_field(
                "failed_authority_pairing_check_ids",
                if signal == "public-entry-handoff" {
                    "[]"
                } else {
                    "[]"
                },
                "illustrative empty list because the sample snapshot does not violate pairing \
                 checks",
            ),
            example_field(
                "forbidden_shortcut_hits",
                "[]",
                "illustrative empty list because the sample snapshot did not rely on forbidden \
                 shortcuts",
            ),
            example_field(
                "forbidden_post_terminal_mutation_hits",
                "[]",
                "illustrative empty list because the sample snapshot did not rewrite terminal \
                 fields incorrectly",
            ),
        ],
        notes,
    }
}

pub(super) const CUTOVER_MATERIAL_STATUS_REPO_BASELINE_BLOCKED: &str = "repo-baseline-blocked";
pub(super) const CUTOVER_MATERIAL_STATUS_OPERATOR_CAPTURE_REQUIRED: &str =
    "operator-capture-required";
pub(super) const CUTOVER_MATERIAL_STATUS_EXTERNAL_MATERIAL_REQUIRED: &str =
    "external-material-required";

pub(super) fn cutover_material_checklist_item(
    id: &'static str,
    owner: &'static str,
    evidence_source: &'static str,
    required_for_cutover: bool,
    completion_criteria: &'static str,
    current_repo_baseline: &'static str,
    current_stage_status: &'static str,
    current_stage_detail: impl Into<String>,
    operator_next_step: &'static str,
) -> CutoverMaterialChecklistItem {
    CutoverMaterialChecklistItem {
        id,
        owner,
        evidence_source,
        required_for_cutover,
        completion_criteria,
        current_repo_baseline,
        current_stage_status,
        current_stage_detail: current_stage_detail.into(),
        operator_next_step,
    }
}

pub(super) fn cutover_gap_reason_summary(gap_reasons: &[&'static str]) -> String {
    if gap_reasons.is_empty() {
        "none".to_owned()
    } else {
        gap_reasons.join(", ")
    }
}

pub(super) fn public_entry_cutover_material_checklist(
    environment: &'static str,
    authoritative_auth_mode: common_net::msg::ServerAuthMode,
    authoritative_auth_provider: Option<&str>,
    repo_bundled_official_entry_snapshot: &RepoBundledOfficialEntrySnapshotReport,
) -> Vec<CutoverMaterialChecklistItem> {
    let repo_bundled_baseline = repo_bundled_official_entry_snapshot.baseline.as_ref();
    let authoritative_auth_provider_detail = authoritative_auth_provider.unwrap_or("none");

    let bundled_artifact_review_detail = match repo_bundled_baseline {
        Some(baseline) => format!(
            "repo/local bundled baseline is visible (target_kind={}, auth_mode={}, \
             non_local_cutover_ready={}, gap_reasons=[{}]); it remains advisory only and cannot \
             replace shipped Public client artifact review",
            baseline.target_kind.as_str(),
            baseline.auth_mode.as_str(),
            baseline.non_local_cutover_ready,
            cutover_gap_reason_summary(&baseline.non_local_cutover_gap_reasons)
        ),
        None => format!(
            "repo/local bundled official_entry baseline is unavailable in-process (status={}); \
             shipped Public client artifact review is the only authoritative bundle-side source \
             exposed here",
            repo_bundled_official_entry_snapshot.status
        ),
    };

    let (external_auth_authority_status, external_auth_authority_detail, external_auth_next_step) =
        match repo_bundled_baseline {
            Some(baseline) => {
                match (baseline.auth_server.as_deref(), authoritative_auth_provider) {
                    (Some(bundled_auth_server), Some(provider))
                        if bundled_auth_server == provider =>
                    {
                        (
                            CUTOVER_MATERIAL_STATUS_OPERATOR_CAPTURE_REQUIRED,
                            format!(
                                "repo/local bundled auth pin already matches the authoritative \
                                 handshake auth_provider {}; operator still must capture the \
                                 shipped bundle auth pin and exact-match evidence for the rollout \
                                 unit",
                                provider
                            ),
                            "copy the shipped bundle auth pin and authoritative handshake \
                             auth_provider into the same public-entry-handoff review section",
                        )
                    },
                    (Some(bundled_auth_server), Some(provider)) => (
                        CUTOVER_MATERIAL_STATUS_REPO_BASELINE_BLOCKED,
                        format!(
                            "repo/local bundled auth pin {} does not match the authoritative \
                             handshake auth_provider {}; exact-match Public auth review cannot \
                             pass until they converge",
                            bundled_auth_server, provider
                        ),
                        "update bundled official_entry.auth_server so the shipped Public bundle \
                         pins the same auth authority as the target realm handshake",
                    ),
                    (Some(_), None) => (
                        CUTOVER_MATERIAL_STATUS_REPO_BASELINE_BLOCKED,
                        format!(
                            "repo/local bundled auth pin is set, but the authoritative handshake \
                             auth mode is {} with no external auth_provider; non-local Public \
                             auth handoff is not yet in a supported exact-match posture",
                            authoritative_auth_mode.as_str()
                        ),
                        "bring the target realm to a supported external-auth handshake posture \
                         before Public cutover review",
                    ),
                    (None, Some(provider)) => (
                        CUTOVER_MATERIAL_STATUS_REPO_BASELINE_BLOCKED,
                        format!(
                            "repo/local bundled auth pin is unset while the authoritative \
                             handshake auth_provider is {}; exact-match Public auth review cannot \
                             pass",
                            provider
                        ),
                        "set bundled official_entry.auth_server to the target realm auth \
                         authority before release review",
                    ),
                    (None, None) => (
                        CUTOVER_MATERIAL_STATUS_REPO_BASELINE_BLOCKED,
                        format!(
                            "repo/local bundled auth pin is unset and the authoritative handshake \
                             auth mode is {}; non-local Public auth handoff remains unsupported",
                            authoritative_auth_mode.as_str()
                        ),
                        "move the target realm and shipped bundle to a supported external-auth \
                         posture before non-local Public review",
                    ),
                }
            },
            None => (
                CUTOVER_MATERIAL_STATUS_EXTERNAL_MATERIAL_REQUIRED,
                format!(
                    "repo/local bundled official_entry baseline is unavailable in-process \
                     (status={}); external shipped bundle review must carry the auth pin evidence \
                     and exact-match validation against authoritative auth_provider {}",
                    repo_bundled_official_entry_snapshot.status, authoritative_auth_provider_detail
                ),
                "capture the shipped bundle auth pin and authoritative handshake auth_provider in \
                 the external release review record",
            ),
        };

    let (non_local_target_status, non_local_target_detail, non_local_target_next_step) =
        match repo_bundled_baseline {
            Some(baseline) if baseline.non_local_cutover_ready => (
                CUTOVER_MATERIAL_STATUS_OPERATOR_CAPTURE_REQUIRED,
                format!(
                    "repo/local bundled posture already classifies the target as {} and reports \
                     no remaining non-local cutover gaps; external shipped bundle review still \
                     must confirm the same posture for the rollout unit",
                    baseline.target_kind.as_str()
                ),
                "record the shipped bundle target posture and confirm that \
                 non_local_cutover_gap_reasons is empty for the cutover candidate",
            ),
            Some(baseline) => (
                CUTOVER_MATERIAL_STATUS_REPO_BASELINE_BLOCKED,
                format!(
                    "repo/local bundled posture still reports target_kind={} and \
                     non_local_cutover_gap_reasons=[{}]",
                    baseline.target_kind.as_str(),
                    cutover_gap_reason_summary(&baseline.non_local_cutover_gap_reasons)
                ),
                "replace bundled official_entry.server_address/auth posture until the bundle is a \
                 non-local candidate with no remaining cutover gap reasons",
            ),
            None => (
                CUTOVER_MATERIAL_STATUS_EXTERNAL_MATERIAL_REQUIRED,
                format!(
                    "repo/local bundled official_entry baseline is unavailable in-process \
                     (status={}); external shipped bundle review must prove the bundle targets a \
                     non-local candidate and carries no remaining cutover gaps",
                    repo_bundled_official_entry_snapshot.status
                ),
                "capture the shipped bundle target posture and gap reasons in the external \
                 release review record",
            ),
        };

    vec![
        cutover_material_checklist_item(
            "bundled-public-entry-artifact-reviewed",
            "release-operator",
            "external-release-tracker + bundled-client-artifact-review + \
             client-exported-entry-contract",
            true,
            "record the shipped Public client artifact reference plus the bundled official_entry \
             artifact identity, server_address, auth_server, use_srv, use_quic, validate_tls, \
             bundled target kind, and non-local gap reasons for the exact Public client artifact \
             being considered",
            "current bundled client artifact still points at a private/LAN target and keeps \
             auth_server unset",
            CUTOVER_MATERIAL_STATUS_EXTERNAL_MATERIAL_REQUIRED,
            bundled_artifact_review_detail,
            "capture the shipped Public client artifact review and the client-exported bundled \
             entry posture in the same public-entry-handoff section",
        ),
        cutover_material_checklist_item(
            "authoritative-runtime-target-confirmed",
            "release-operator",
            "/health/compatibility",
            true,
            "record target_runtime_environment, authoritative_compatibility_generation, \
             expected_handshake_auth_mode, authoritative_handshake_auth_provider, and \
             query_auth_required_hint from the authoritative server-side contract before \
             approving cutover",
            "health contract already exports the authoritative target environment and \
             compatibility/auth posture, but rollout-specific values still need operator capture",
            CUTOVER_MATERIAL_STATUS_OPERATOR_CAPTURE_REQUIRED,
            format!(
                "authoritative runtime currently reports environment={}, auth_mode={}, \
                 auth_provider={}; operator still must copy these rollout-specific values from \
                 /health/compatibility into the same release_reference",
                environment,
                authoritative_auth_mode.as_str(),
                authoritative_auth_provider_detail
            ),
            "copy /health/compatibility target environment, compatibility, and auth posture into \
             the same external review section",
        ),
        cutover_material_checklist_item(
            "external-auth-authority-pinned",
            "release-operator",
            "bundled-client-artifact-review + realm handshake auth_provider",
            true,
            "bundled official_entry.auth_server must be non-empty for non-local Public rollout \
             and must exactly match the authoritative handshake auth provider for the target realm",
            "current repo baseline cannot pass this check because bundled auth_server is None",
            external_auth_authority_status,
            external_auth_authority_detail,
            external_auth_next_step,
        ),
        cutover_material_checklist_item(
            "non-local-target-material-ready",
            "release-operator",
            "client-exported-entry-contract",
            true,
            "bundled_target_is_non_local_candidate and non_local_cutover_ready must both be true, \
             with no remaining non_local_cutover_gap_reasons",
            "current repo baseline still reports \
             bundled_public_target_is_private_or_unique_local_ip and \
             bundled_public_auth_pin_missing",
            non_local_target_status,
            non_local_target_detail,
            non_local_target_next_step,
        ),
        cutover_material_checklist_item(
            "readiness-evidence-linked",
            "release-operator",
            "/health/ready",
            true,
            "ready_report_status must be recorded from a ready instance in the same rollout unit \
             before Public traffic is reopened",
            "the ready contract exists, but rollout-specific status still has to be linked per \
             release",
            CUTOVER_MATERIAL_STATUS_OPERATOR_CAPTURE_REQUIRED,
            "the ready contract is available in-process, but the observed ready_report_status \
             still has to be captured for the exact rollout unit that will receive Public traffic"
                .to_owned(),
            "record ready_report_status from /health/ready on the target rollout instance before \
             reopening Public traffic",
        ),
        cutover_material_checklist_item(
            "backup-and-recovery-evidence-linked",
            "release-operator",
            "/health/backup + /health/recovery/drill",
            true,
            "same release-review-record must link current backup_evidence_reference and a \
             rollout-acceptable recovery_drill_reference before approval",
            "backup and recovery drill contracts exist, but the current repo does not yet \
             auto-fill external review records",
            CUTOVER_MATERIAL_STATUS_OPERATOR_CAPTURE_REQUIRED,
            "backup and recovery drill contracts are available in-process, but the same \
             release-review-record still has to link the rollout-specific evidence references \
             before approval"
                .to_owned(),
            "link current backup evidence and a rollout-acceptable recovery drill reference in \
             the same external review section",
        ),
        cutover_material_checklist_item(
            "rollback-path-recorded",
            "release-operator",
            "external-release-tracker",
            true,
            "same release-review-record must include rollback_reference, \
             rollback_public_client_artifact_reference, and \
             rollback_bundled_official_entry_artifact_identity before cutover so the rollback \
             path plus the restored Public client artifact and entry material are already fixed \
             if traffic must be reverted",
            "rollback_reference is contractually required but still depends on external operator \
             recording; rollback bundled official_entry material is not auto-filled in this repo",
            CUTOVER_MATERIAL_STATUS_EXTERNAL_MATERIAL_REQUIRED,
            "rollback path capture still depends on external operator recording because this \
             process does not auto-fill rollback_reference, rollback Public client artifact, or \
             rollback bundled official_entry material into the authoritative review record"
                .to_owned(),
            "fix rollback_reference, rollback_public_client_artifact_reference, and \
             rollback_bundled_official_entry_artifact_identity in the same external review \
             section before cutover approval",
        ),
    ]
}

pub(super) fn public_entry_authority_pairing_checks() -> Vec<ExternalRecordAuthorityPairingCheck> {
    vec![
        authority_pairing_check(
            "bundled-artifact-vs-release-unit",
            vec![
                "bundled_public_client_artifact_reference",
                "bundled_official_entry_artifact_identity",
                "release_reference",
            ],
            vec!["external-release-tracker", "bundled-client-artifact-review"],
            "the shipped Public client artifact reference and the bundled official_entry artifact \
             identity recorded in the review must both belong to the same release_reference; do \
             not approve if the rollout record points at a different client bundle or a different \
             bundled entry payload",
            true,
        ),
        authority_pairing_check(
            "bundled-target-transport-vs-runtime-contract",
            vec![
                "bundled_official_entry_server_address",
                "bundled_official_entry_use_srv",
                "bundled_official_entry_use_quic",
                "bundled_official_entry_validate_tls",
                "target_runtime_environment",
                "authoritative_compatibility_generation",
            ],
            vec!["bundled-client-artifact-review", "/health/compatibility"],
            "the bundled Public target address and transport material must describe the same \
             rollout unit whose runtime environment and compatibility generation are being \
             approved; do not mix one client bundle's address or transport flags with another \
             rollout unit's runtime contract",
            true,
        ),
        authority_pairing_check(
            "bundled-auth-pin-vs-handshake-authority",
            vec![
                "bundled_official_entry_auth_server",
                "expected_handshake_auth_mode",
                "authoritative_handshake_auth_provider",
                "query_auth_required_hint",
            ],
            vec!["bundled-client-artifact-review", "/health/compatibility"],
            "the bundled Public auth pin must remain non-empty for non-local rollout and must be \
             compatible with the authoritative handshake auth mode; exact-match the authoritative \
             handshake auth provider and do not fall back to no-auth semantics",
            true,
        ),
        authority_pairing_check(
            "non-local-posture-vs-release-gate",
            vec![
                "bundled_target_kind",
                "bundled_target_is_non_local_candidate",
                "non_local_cutover_ready",
                "non_local_cutover_gap_reasons",
                "ready_report_status",
            ],
            vec!["client-exported-entry-contract", "/health/ready"],
            "do not approve Public cutover if the client-exported bundled target posture still \
             reports unresolved non-local gap reasons or if readiness evidence is not captured \
             for the same rollout unit",
            true,
        ),
        authority_pairing_check(
            "rollback-entry-material-vs-rollback-path",
            vec![
                "rollback_reference",
                "rollback_public_client_artifact_reference",
                "rollback_bundled_official_entry_artifact_identity",
                "release_reference",
            ],
            vec!["external-release-tracker"],
            "the same release review record must identify the rollback path, rollback Public \
             client artifact reference, and rollback bundled official_entry artifact identity \
             that will be restored if this rollout unit is reverted",
            true,
        ),
    ]
}

pub(super) fn public_entry_transition_contract() -> PublicEntryTransitionContract {
    PublicEntryTransitionContract {
        transition_scope: "non-local Public official_entry cutover transition unit",
        record_scope: "same public-entry-handoff section keyed by release_reference",
        atomic_bundle_fields: vec![
            "bundled_public_client_artifact_reference",
            "bundled_official_entry_artifact_identity",
            "bundled_official_entry_server_address",
            "bundled_official_entry_auth_server",
            "bundled_official_entry_use_srv",
            "bundled_official_entry_use_quic",
            "bundled_official_entry_validate_tls",
            "bundled_target_kind",
            "bundled_target_is_non_local_candidate",
            "non_local_cutover_ready",
            "non_local_cutover_gap_reasons",
        ],
        atomic_runtime_gate_fields: vec![
            "target_runtime_environment",
            "authoritative_compatibility_generation",
            "expected_handshake_auth_mode",
            "authoritative_handshake_auth_provider",
            "query_auth_required_hint",
            "ready_report_status",
            "backup_evidence_reference",
            "recovery_drill_reference",
        ],
        atomic_rollback_restore_fields: vec![
            "rollback_reference",
            "rollback_public_client_artifact_reference",
            "rollback_bundled_official_entry_artifact_identity",
        ],
        forbidden_partial_transitions: vec![
            "do not publish a new official_entry.server_address without the matching auth pin and \
             transport flags from the same shipped bundle review",
            "do not move official_entry.auth_server without capturing the matching \
             authoritative_handshake_auth_provider and expected_handshake_auth_mode for the same \
             rollout unit",
            "do not reopen Public traffic before ready_report_status, backup_evidence_reference, \
             and recovery_drill_reference are linked for the same release_reference",
            "do not approve the bundle-side transition until rollback_reference, \
             rollback_public_client_artifact_reference, and \
             rollback_bundled_official_entry_artifact_identity are fixed for the same rollout unit",
        ],
        approval_gate: "reopen Public traffic only after bundle-side tuple, runtime gate, and \
                        rollback restore unit are all recorded on the same release_reference",
    }
}

pub(super) fn public_entry_lifecycle_transition_contract() -> PublicEntryLifecycleTransitionContract
{
    let evidence_linked_required_fields = vec![
        "bundled_public_client_artifact_reference",
        "bundled_official_entry_artifact_identity",
        "bundled_official_entry_server_address",
        "bundled_official_entry_auth_server",
        "bundled_official_entry_use_srv",
        "bundled_official_entry_use_quic",
        "bundled_official_entry_validate_tls",
        "bundled_target_kind",
        "bundled_target_is_non_local_candidate",
        "non_local_cutover_ready",
        "non_local_cutover_gap_reasons",
        "target_runtime_environment",
        "authoritative_compatibility_generation",
        "expected_handshake_auth_mode",
        "authoritative_handshake_auth_provider",
        "query_auth_required_hint",
        "ready_report_status",
        "backup_evidence_reference",
        "recovery_drill_reference",
        "result_status",
    ];
    let terminal_decision_required_fields = vec![
        "approval_decision",
        "rollback_reference",
        "rollback_public_client_artifact_reference",
        "rollback_bundled_official_entry_artifact_identity",
        "result_status",
    ];

    PublicEntryLifecycleTransitionContract {
        lifecycle_scope: "public-entry-handoff approval-to-terminal lifecycle on the same \
                          release-review-record section",
        initial_state: "draft",
        evidence_ready_state: "evidence-linked",
        terminal_states_requiring_archive_receipt: vec![
            "cutover-approved",
            "cutover-rejected",
            "rolled-back",
        ],
        unsupported_paths: vec![
            "exception-accepted is currently unsupported for public-entry-handoff; do not treat \
             exception fields as a valid terminal/archive path until a dedicated lifecycle is \
             formalized",
            "rolled-back is invalid before the same section was previously cutover-approved",
        ],
        transitions: vec![
            PublicEntryLifecycleTransition {
                from_state: "draft",
                to_state: "evidence-linked",
                approval_decision: None,
                required_fields: evidence_linked_required_fields,
                archive_required: false,
                notes: vec![
                    "bundle-side material, runtime truth, and readiness/backup/drill evidence \
                     must all be linked before the section may leave draft",
                    "do not advance to evidence-linked if authority pairing checks or transition \
                     contract requirements still fail for the same release_reference",
                ],
            },
            PublicEntryLifecycleTransition {
                from_state: "evidence-linked",
                to_state: "cutover-approved",
                approval_decision: Some("approved"),
                required_fields: terminal_decision_required_fields.clone(),
                archive_required: true,
                notes: vec![
                    "approved is the only valid approval_decision for a cutover-approved terminal \
                     state",
                    "rollback restore fields must already be fixed on the same section before \
                     Public traffic is reopened",
                ],
            },
            PublicEntryLifecycleTransition {
                from_state: "evidence-linked",
                to_state: "cutover-rejected",
                approval_decision: Some("rejected"),
                required_fields: terminal_decision_required_fields.clone(),
                archive_required: true,
                notes: vec![
                    "rejected is the only valid approval_decision for a cutover-rejected terminal \
                     state",
                    "the same section still keeps the rollback unit fixed so the rejected rollout \
                     remains audit-complete and reversible",
                ],
            },
            PublicEntryLifecycleTransition {
                from_state: "cutover-approved",
                to_state: "rolled-back",
                approval_decision: Some("approved"),
                required_fields: terminal_decision_required_fields,
                archive_required: true,
                notes: vec![
                    "rolled-back is only valid after the same section was previously \
                     cutover-approved",
                    "rolled-back reuses the original approval_decision=approved and records the \
                     later reversion through result_status on the same section",
                ],
            },
        ],
    }
}

pub(super) fn archive_receipt_template_fields(
    archive_handoff_contract: &ExternalRecordArchiveHandoffContract,
) -> Vec<ExternalRecordTemplateField> {
    archive_handoff_contract
        .required_archive_receipt_fields
        .iter()
        .map(|field| {
            let (placeholder, completion_rule) = match *field {
                "archive_reference" => (
                    "<archive-object-or-ticket-reference>",
                    "record the immutable archive location or archive tracker id after the \
                     section reaches a terminal state",
                ),
                "archived_at_utc" => (
                    "<utc-timestamp>",
                    "record when the terminal review section was handed off to archive",
                ),
                "archived_by" => (
                    "<operator-or-automation-id>",
                    "record who completed the archive handoff",
                ),
                "source_record_state" => (
                    "<terminal-result-status>",
                    "record which terminal result_status was archived",
                ),
                _ => (
                    "<fill-required-value>",
                    "record the required archive receipt value from the external archive handoff \
                     step",
                ),
            };
            template_field(field, placeholder, completion_rule)
        })
        .collect()
}

pub(super) fn release_review_post_archive_writeback_field_names() -> Vec<&'static str> {
    vec![
        "post_archive_verified_by",
        "post_archive_verified_at_utc",
        "post_archive_verification_result",
        "post_archive_verification_reference",
    ]
}

pub(super) fn review_record_field_placeholder(
    signal: &'static str,
    name: &'static str,
) -> &'static str {
    match (signal, name) {
        (_, "reviewed_by") => "<release-operator-id>",
        ("public-entry-handoff", "approval_decision") => "<approved|rejected>",
        (_, "approval_decision") => "<approved|rejected|exception-accepted>",
        (_, "decision_recorded_at_utc") => "<utc-timestamp>",
        (_, "result_status") => "<state-from-result_status_model>",
        (_, "release_reference") => "<release-reference>",
        ("public-entry-handoff", "bundled_public_client_artifact_reference") => {
            "<public-client-release-artifact-ref>"
        },
        ("public-entry-handoff", "bundled_official_entry_artifact_identity") => {
            "<official-entry-content-sha256-v1:...>"
        },
        ("public-entry-handoff", "bundled_official_entry_server_address") => {
            "<public-realm-host-or-socket>"
        },
        ("public-entry-handoff", "bundled_official_entry_auth_server") => {
            "<https://auth.realm.example-or-null>"
        },
        ("public-entry-handoff", "bundled_official_entry_use_srv") => "<true|false>",
        ("public-entry-handoff", "bundled_official_entry_use_quic") => "<true|false>",
        ("public-entry-handoff", "bundled_official_entry_validate_tls") => "<true|false>",
        ("public-entry-handoff", "bundled_target_kind") => {
            "<missing|localhost-or-loopback|private-or-unique-local-ip|reserved-non-public-ip|named-host-candidate|public-ip-candidate>"
        },
        ("public-entry-handoff", "bundled_target_is_non_local_candidate") => "<true|false>",
        ("public-entry-handoff", "non_local_cutover_ready") => "<true|false>",
        ("public-entry-handoff", "non_local_cutover_gap_reasons") => {
            "<[]-or-gap-reason-list>"
        },
        ("public-entry-handoff", "target_runtime_environment") => "<test|production>",
        ("public-entry-handoff", "authoritative_compatibility_generation") => {
            "<u16-generation>"
        },
        ("public-entry-handoff", "expected_handshake_auth_mode") => {
            "<no-external-auth|external-provider>"
        },
        ("public-entry-handoff", "authoritative_handshake_auth_provider") => {
            "<https://auth.realm.example-or-null>"
        },
        ("public-entry-handoff", "query_auth_required_hint") => "<true|false>",
        ("public-entry-handoff", "ready_report_status") => "<ready-status>",
        ("public-entry-handoff", "backup_evidence_reference") => {
            "<backup-evidence-reference>"
        },
        ("public-entry-handoff", "recovery_drill_reference") => {
            "<recovery-drill-reference>"
        },
        ("public-entry-handoff", "rollback_public_client_artifact_reference") => {
            "<rollback-public-client-release-artifact-ref>"
        },
        ("public-entry-handoff", "rollback_bundled_official_entry_artifact_identity") => {
            "<rollback-official-entry-content-sha256-v1:...>"
        },
        ("public-entry-handoff", "rollback_reference") => "<rollback-runbook-or-release-ref>",
        ("public-entry-handoff", "bundled_auth_pin_review_reference") => {
            "<review-note-reference>"
        },
        (_, "post_archive_verified_by") => "<release-operator-id>",
        (_, "post_archive_verified_at_utc") => "<utc-timestamp>",
        (_, "post_archive_verification_result") => "<verified|needs-follow-up>",
        (_, "post_archive_verification_reference") => "<archive-review-note-or-ticket>",
        ("governance-audit", "exception_reason") => "<operator-rationale>",
        ("governance-audit", "governance_note_reference") => "<governance-note-reference>",
        ("governance-audit", "rollback_reference") => "<rollback-runbook-or-release-ref>",
        ("management-auth", "affected_surfaces") => "<[surface-id,...]>",
        ("management-auth", "compensating_controls") => "<control-summary>",
        ("management-auth", "rollback_reference") => "<rollback-runbook-or-release-ref>",
        _ => "<fill-required-value>",
    }
}

pub(super) fn review_record_field_completion_rule(
    signal: &'static str,
    name: &'static str,
) -> &'static str {
    match (signal, name) {
        (_, "reviewed_by") => "record the human owner who made the review decision",
        ("public-entry-handoff", "approval_decision") => {
            "record approved or rejected only; public-entry-handoff does not currently support \
             exception-accepted as a valid lifecycle transition"
        },
        (_, "approval_decision") => {
            "record the operator decision that matches the section result_status"
        },
        (_, "decision_recorded_at_utc") => {
            "record when the decision was entered into the external release tracker"
        },
        (_, "result_status") => {
            "must be one of the states published in result_status_model for this review section"
        },
        (_, "release_reference") => {
            "must match the rollout unit key used as the canonical external record id"
        },
        ("public-entry-handoff", "bundled_public_client_artifact_reference") => {
            "record the exact shipped Public client release artifact reference whose bundled \
             official_entry is under review"
        },
        ("public-entry-handoff", "bundled_official_entry_artifact_identity") => {
            "copy the exact artifact identity derived from the shipped bundled official_entry"
        },
        ("public-entry-handoff", "bundled_official_entry_server_address") => {
            "copy the exact bundled Public realm target under review"
        },
        ("public-entry-handoff", "bundled_official_entry_auth_server") => {
            "copy the exact bundled Public auth pin, using null only if the bundle truly has no \
             auth authority configured"
        },
        ("public-entry-handoff", "bundled_official_entry_use_srv")
        | ("public-entry-handoff", "bundled_official_entry_use_quic")
        | ("public-entry-handoff", "bundled_official_entry_validate_tls") => {
            "copy the bundled transport policy from the shipped Public artifact"
        },
        ("public-entry-handoff", "bundled_target_kind")
        | ("public-entry-handoff", "bundled_target_is_non_local_candidate")
        | ("public-entry-handoff", "non_local_cutover_ready")
        | ("public-entry-handoff", "non_local_cutover_gap_reasons") => {
            "copy the client-exported interpretation of the bundled target posture"
        },
        ("public-entry-handoff", "target_runtime_environment")
        | ("public-entry-handoff", "authoritative_compatibility_generation")
        | ("public-entry-handoff", "expected_handshake_auth_mode")
        | ("public-entry-handoff", "authoritative_handshake_auth_provider")
        | ("public-entry-handoff", "query_auth_required_hint") => {
            "copy the authoritative rollout contract values from /health/compatibility"
        },
        ("public-entry-handoff", "ready_report_status") => {
            "copy the observed ready status from /health/ready for the rollout unit"
        },
        ("public-entry-handoff", "backup_evidence_reference") => {
            "link the backup evidence record supporting this rollout"
        },
        ("public-entry-handoff", "recovery_drill_reference") => {
            "link a recovery drill record that reached a rollout-acceptable state"
        },
        ("public-entry-handoff", "rollback_public_client_artifact_reference") => {
            "record the exact Public client release artifact reference that rollback will restore \
             if this rollout unit is reverted"
        },
        ("public-entry-handoff", "rollback_bundled_official_entry_artifact_identity") => {
            "record the exact bundled official_entry artifact identity that rollback will restore \
             if this rollout unit is reverted"
        },
        ("public-entry-handoff", "rollback_reference") => {
            "record the rollback path before Public traffic is reopened"
        },
        ("public-entry-handoff", "bundled_auth_pin_review_reference") => {
            "link the review note that justifies any auth pin exception"
        },
        (_, "post_archive_verified_by") => {
            "record who completed post-archive verification for the terminal review section"
        },
        (_, "post_archive_verified_at_utc") => {
            "record when archive retrieval and retention verification finished"
        },
        (_, "post_archive_verification_result") => {
            "record whether post-archive verification completed cleanly or still needs follow-up"
        },
        (_, "post_archive_verification_reference") => {
            "link the archive retrieval note, ticket, or evidence reference that proves \
             post-archive verification"
        },
        ("governance-audit", "exception_reason") => {
            "record why the governance exception is being accepted"
        },
        ("governance-audit", "governance_note_reference") => {
            "link the ticket or note that backs the governance decision"
        },
        ("governance-audit", "rollback_reference") => {
            "record how to revert if the governance exception causes rollback"
        },
        ("management-auth", "affected_surfaces") => {
            "list the management or observability surfaces covered by the exception"
        },
        ("management-auth", "compensating_controls") => {
            "record the controls that make the accepted exposure reviewable"
        },
        ("management-auth", "rollback_reference") => {
            "record how to revert if the management auth exception must be rolled back"
        },
        _ => "record the value from the evidence source defined in the field contract",
    }
}

pub(super) fn review_record_template_fields(
    signal: &'static str,
    contracts: &[ExternalRecordFieldContract],
) -> Vec<ExternalRecordTemplateField> {
    contracts
        .iter()
        .map(|contract| {
            template_field(
                contract.name,
                review_record_field_placeholder(signal, contract.name),
                review_record_field_completion_rule(signal, contract.name),
            )
        })
        .collect()
}

pub(super) fn release_review_section_template_contract(
    signal: &'static str,
    initial_state: &'static str,
    required_decision_field_contracts: &[ExternalRecordFieldContract],
    exception_record_field_contracts: &[ExternalRecordFieldContract],
    archive_handoff_contract: &ExternalRecordArchiveHandoffContract,
    post_archive_writeback_fields: &[&'static str],
    notes: Vec<&'static str>,
) -> ExternalRecordSectionTemplateContract {
    ExternalRecordSectionTemplateContract {
        record_kind: "release-review-record",
        section_signal: signal,
        lifecycle_state_field: "result_status",
        initial_state,
        required_fields: review_record_template_fields(signal, required_decision_field_contracts),
        exception_fields: review_record_template_fields(signal, exception_record_field_contracts),
        archive_receipt_fields_when_terminal: archive_receipt_template_fields(
            archive_handoff_contract,
        ),
        post_archive_follow_up_fields: post_archive_writeback_fields
            .iter()
            .map(|field| {
                template_field(
                    field,
                    review_record_field_placeholder(signal, field),
                    review_record_field_completion_rule(signal, field),
                )
            })
            .collect(),
        notes,
    }
}

pub(super) fn review_record_example_value(
    signal: &'static str,
    name: &'static str,
) -> &'static str {
    match (signal, name) {
        (_, "reviewed_by") => "ops-release-owner",
        (_, "approval_decision") => "approved",
        (_, "decision_recorded_at_utc") => "2026-05-01T08:15:00Z",
        (_, "result_status") => "cutover-approved",
        (_, "release_reference") => "release-2026-05-01-public-cutover-01",
        ("public-entry-handoff", "bundled_public_client_artifact_reference") => {
            "artifact://public-client/release-2026-05-01-build-01"
        },
        ("public-entry-handoff", "bundled_official_entry_artifact_identity") => {
            "official-entry-content-sha256-v1:examplebundledeadbeef"
        },
        ("public-entry-handoff", "bundled_official_entry_server_address") => {
            "prod.realm.example:14004"
        },
        ("public-entry-handoff", "bundled_official_entry_auth_server") => {
            "https://auth.realm.example"
        },
        ("public-entry-handoff", "bundled_official_entry_use_srv") => "true",
        ("public-entry-handoff", "bundled_official_entry_use_quic") => "true",
        ("public-entry-handoff", "bundled_official_entry_validate_tls") => "true",
        ("public-entry-handoff", "bundled_target_kind") => "named-host-candidate",
        ("public-entry-handoff", "bundled_target_is_non_local_candidate") => "true",
        ("public-entry-handoff", "non_local_cutover_ready") => "true",
        ("public-entry-handoff", "non_local_cutover_gap_reasons") => "[]",
        ("public-entry-handoff", "target_runtime_environment") => "production",
        ("public-entry-handoff", "authoritative_compatibility_generation") => "104",
        ("public-entry-handoff", "expected_handshake_auth_mode") => "external-provider",
        ("public-entry-handoff", "authoritative_handshake_auth_provider") => {
            "https://auth.realm.example"
        },
        ("public-entry-handoff", "query_auth_required_hint") => "true",
        ("public-entry-handoff", "ready_report_status") => "ready",
        ("public-entry-handoff", "backup_evidence_reference") => {
            "backup-evidence:release-2026-05-01-public-cutover-01"
        },
        ("public-entry-handoff", "recovery_drill_reference") => {
            "recovery-drill:2026-04-30-prod-restore"
        },
        ("public-entry-handoff", "rollback_public_client_artifact_reference") => {
            "artifact://public-client/release-2026-04-18-build-03"
        },
        ("public-entry-handoff", "rollback_bundled_official_entry_artifact_identity") => {
            "official-entry-content-sha256-v1:previousbundlecafebabe"
        },
        ("public-entry-handoff", "rollback_reference") => "runbook://public-cutover/rollback-01",
        ("public-entry-handoff", "bundled_auth_pin_review_reference") => {
            "note://release-review/auth-pin-exception"
        },
        ("governance-audit", "exception_reason") => {
            "temporary operator-approved exposure retained during scheduled migration window"
        },
        ("governance-audit", "governance_note_reference") => "note://governance/review-2026-05-01",
        ("governance-audit", "rollback_reference") => "runbook://governance/rollback-01",
        ("management-auth", "affected_surfaces") => "[\"metrics\", \"web\"]",
        ("management-auth", "compensating_controls") => {
            "reverse proxy ACL plus rotating shared secret documented in ticket OPS-42"
        },
        ("management-auth", "rollback_reference") => "runbook://management-auth/rollback-01",
        _ => "example-value",
    }
}

pub(super) fn review_record_example_rationale(
    signal: &'static str,
    name: &'static str,
) -> &'static str {
    match (signal, name) {
        (_, "reviewed_by") => "illustrative operator identity only",
        (_, "approval_decision") => {
            "illustrative decision value; use the enum your external release tracker expects"
        },
        (_, "decision_recorded_at_utc") => "illustrative UTC timestamp",
        (_, "result_status") => {
            "illustrative terminal state; choose a state from result_status_model that matches the \
             real decision"
        },
        (_, "release_reference") => "illustrative rollout unit key",
        ("public-entry-handoff", "bundled_public_client_artifact_reference")
        | ("public-entry-handoff", "rollback_public_client_artifact_reference") => {
            "illustrative release artifact reference format; replace with your real shipped or \
             rollback Public client artifact id"
        },
        ("public-entry-handoff", "bundled_official_entry_server_address")
        | ("public-entry-handoff", "bundled_official_entry_auth_server")
        | ("public-entry-handoff", "authoritative_handshake_auth_provider") => {
            "uses RFC 2606 example hostnames instead of real production values"
        },
        ("public-entry-handoff", "bundled_official_entry_artifact_identity") => {
            "illustrative artifact identity shape only"
        },
        ("public-entry-handoff", "rollback_bundled_official_entry_artifact_identity") => {
            "illustrative rollback artifact identity shape only"
        },
        ("public-entry-handoff", "backup_evidence_reference")
        | ("public-entry-handoff", "recovery_drill_reference")
        | ("public-entry-handoff", "rollback_reference") => {
            "illustrative reference format; replace with your real evidence or runbook id"
        },
        _ => "illustrative value; replace with rollout-specific truth",
    }
}

pub(super) fn release_review_section_example_contract(
    signal: &'static str,
    section_state: &'static str,
    required_decision_field_contracts: &[ExternalRecordFieldContract],
    notes: Vec<&'static str>,
) -> ExternalRecordSectionExampleContract {
    ExternalRecordSectionExampleContract {
        record_kind: "release-review-record",
        section_signal: signal,
        illustrative_only: true,
        section_state,
        example_fields: required_decision_field_contracts
            .iter()
            .map(|contract| {
                example_field(
                    contract.name,
                    review_record_example_value(signal, contract.name),
                    review_record_example_rationale(signal, contract.name),
                )
            })
            .collect(),
        notes,
    }
}

pub(super) fn release_review_section_execution_workflow(
    signal: &'static str,
) -> Vec<ExternalRecordWorkflowStep> {
    match signal {
        "public-entry-handoff" => vec![
            workflow_step(
                1,
                "open or locate the public-entry-handoff section inside the release-review-record \
                 for the rollout unit",
                "release-operator",
                "external-release-tracker",
                "set release_reference, reviewed_by, decision_recorded_at_utc, and result_status \
                 = draft on the authoritative external record section",
                vec![
                    "release_reference",
                    "reviewed_by",
                    "decision_recorded_at_utc",
                    "result_status",
                ],
                true,
            ),
            workflow_step(
                2,
                "copy the shipped Public client artifact reference, bundled official_entry \
                 artifact identity, address, auth pin, transport flags, and client-exported \
                 target posture into the review section",
                "release-operator",
                "external-release-tracker + bundled-client-artifact-review + \
                 client-exported-entry-contract",
                "populate shipped Public client artifact and bundled entry material fields before \
                 any approval decision",
                vec![
                    "bundled_public_client_artifact_reference",
                    "bundled_official_entry_artifact_identity",
                    "bundled_official_entry_server_address",
                    "bundled_official_entry_auth_server",
                    "bundled_official_entry_use_srv",
                    "bundled_official_entry_use_quic",
                    "bundled_official_entry_validate_tls",
                    "bundled_target_kind",
                    "bundled_target_is_non_local_candidate",
                    "non_local_cutover_ready",
                    "non_local_cutover_gap_reasons",
                ],
                true,
            ),
            workflow_step(
                3,
                "copy the authoritative runtime environment, compatibility generation, auth mode, \
                 authoritative auth provider, query auth hint, ready status, backup evidence, and \
                 recovery drill reference",
                "release-operator",
                "/health/compatibility + /health/ready + /health/backup + /health/recovery/drill",
                "advance the section to evidence-linked once rollout evidence is fully attached",
                vec![
                    "target_runtime_environment",
                    "authoritative_compatibility_generation",
                    "expected_handshake_auth_mode",
                    "authoritative_handshake_auth_provider",
                    "query_auth_required_hint",
                    "ready_report_status",
                    "backup_evidence_reference",
                    "recovery_drill_reference",
                ],
                true,
            ),
            workflow_step(
                4,
                "record approval_decision, rollback_reference, \
                 rollback_public_client_artifact_reference, \
                 rollback_bundled_official_entry_artifact_identity, and terminal result_status \
                 once the operator decides whether cutover is approved or rejected; only record \
                 rolled-back after the same section was previously cutover-approved",
                "release-operator",
                "external-release-tracker",
                "write the terminal result_status on the same section and keep rollout/rollback \
                 history on the same release-review-record with the rollback Public client \
                 artifact and entry material fixed before traffic is reopened; approval_decision \
                 must stay aligned with the public-entry lifecycle transition contract",
                vec![
                    "approval_decision",
                    "rollback_reference",
                    "rollback_public_client_artifact_reference",
                    "rollback_bundled_official_entry_artifact_identity",
                    "result_status",
                ],
                true,
            ),
            workflow_step(
                5,
                "handoff the terminal section to archive and write back archive receipt fields",
                "release-operator",
                "external-release-tracker archive handoff",
                "record archive_reference, archived_at_utc, archived_by, and source_record_state \
                 on the same release-review-record once archiving completes; the archive handoff \
                 must remain correlated to the same release_reference, section_signal, terminal \
                 result_status, bundled_public_client_artifact_reference, \
                 bundled_official_entry_artifact_identity, rollback_reference, \
                 rollback_public_client_artifact_reference, and \
                 rollback_bundled_official_entry_artifact_identity",
                release_review_archive_receipt_field_names(),
                true,
            ),
            workflow_step(
                6,
                "retrieve the archived terminal section, verify archive correlation and \
                 retrievability, and append post-archive verification fields",
                "release-operator",
                "external archive retrieval + external-release-tracker",
                "append post_archive_verified_by, post_archive_verified_at_utc, \
                 post_archive_verification_result, and post_archive_verification_reference on the \
                 same release-review-record section once archive verification completes",
                release_review_post_archive_writeback_field_names(),
                false,
            ),
        ],
        "governance-audit" => vec![
            workflow_step(
                1,
                "open or locate the governance-audit section inside the release-review-record",
                "release-operator",
                "external-release-tracker",
                "set release_reference and result_status = draft for the governance section",
                vec![
                    "release_reference",
                    "reviewed_by",
                    "decision_recorded_at_utc",
                    "result_status",
                ],
                true,
            ),
            workflow_step(
                2,
                "link governance findings and the review note backing the rollout decision",
                "release-operator",
                "/health/governance + supporting endpoints + external-release-tracker",
                "populate governance review evidence before final approval; if the terminal path \
                 later becomes exception-accepted, append exception_reason and rollback_reference \
                 before writing the terminal state",
                vec!["governance_note_reference"],
                true,
            ),
            workflow_step(
                3,
                "record approval_decision and terminal result_status for approved, rejected, \
                 exception-accepted, or rolled-back governance posture",
                "release-operator",
                "external-release-tracker",
                "write the terminal result_status on the governance section without opening a \
                 second review record; exception-accepted also requires exception_reason and \
                 rollback_reference on the same section",
                vec!["approval_decision", "result_status"],
                true,
            ),
            workflow_step(
                4,
                "archive the terminal governance section and write back the archive receipt",
                "release-operator",
                "external-release-tracker archive handoff",
                "write archive receipt fields after governance section archiving completes and \
                 keep the archive handoff correlated to the same release_reference, \
                 section_signal, and terminal result_status",
                release_review_archive_receipt_field_names(),
                true,
            ),
            workflow_step(
                5,
                "retrieve the archived governance section and append post-archive verification \
                 fields",
                "release-operator",
                "external archive retrieval + external-release-tracker",
                "append post_archive_verified_by, post_archive_verified_at_utc, \
                 post_archive_verification_result, and post_archive_verification_reference after \
                 governance archive verification completes",
                release_review_post_archive_writeback_field_names(),
                false,
            ),
        ],
        "management-auth" => vec![
            workflow_step(
                1,
                "open or locate the management-auth section inside the release-review-record",
                "release-operator",
                "external-release-tracker",
                "set release_reference and result_status = draft for the management-auth section",
                vec![
                    "release_reference",
                    "reviewed_by",
                    "decision_recorded_at_utc",
                    "result_status",
                ],
                true,
            ),
            workflow_step(
                2,
                "copy the reviewed surfaces and exposure findings into the section",
                "release-operator",
                "/health/management-auth + external-release-tracker",
                "populate management-auth review evidence before approval; if the terminal path \
                 later becomes exception-accepted, append compensating_controls and \
                 rollback_reference before writing the terminal state",
                vec!["affected_surfaces"],
                true,
            ),
            workflow_step(
                3,
                "record approval_decision and terminal result_status for the final management \
                 auth posture",
                "release-operator",
                "external-release-tracker",
                "write the terminal result_status on the management-auth section without opening \
                 a second review record; exception-accepted also requires compensating_controls \
                 and rollback_reference on the same section",
                vec!["approval_decision", "result_status"],
                true,
            ),
            workflow_step(
                4,
                "archive the terminal management-auth section and write back the archive receipt",
                "release-operator",
                "external-release-tracker archive handoff",
                "write archive receipt fields after management-auth archiving completes and keep \
                 the archive handoff correlated to the same release_reference, section_signal, \
                 and terminal result_status",
                release_review_archive_receipt_field_names(),
                true,
            ),
            workflow_step(
                5,
                "retrieve the archived management-auth section and append post-archive \
                 verification fields",
                "release-operator",
                "external archive retrieval + external-release-tracker",
                "append post_archive_verified_by, post_archive_verified_at_utc, \
                 post_archive_verification_result, and post_archive_verification_reference after \
                 management-auth archive verification completes",
                release_review_post_archive_writeback_field_names(),
                false,
            ),
        ],
        _ => Vec::new(),
    }
}

pub(super) fn push_unique_static_strs(target: &mut Vec<&'static str>, items: &[&'static str]) {
    for item in items {
        if !target.iter().any(|existing| existing == item) {
            target.push(*item);
        }
    }
}

pub(super) fn workflow_completion_fields_up_to(
    workflow: &[ExternalRecordWorkflowStep],
    up_to_sequence: u8,
) -> Vec<&'static str> {
    let mut fields = Vec::new();
    for step in workflow
        .iter()
        .filter(|step| step.sequence <= up_to_sequence)
    {
        push_unique_static_strs(&mut fields, &step.completion_record_fields);
    }
    fields
}

pub(super) fn workflow_sequences_up_to(
    workflow: &[ExternalRecordWorkflowStep],
    up_to_sequence: u8,
) -> Vec<u8> {
    workflow
        .iter()
        .filter(|step| step.sequence <= up_to_sequence)
        .map(|step| step.sequence)
        .collect()
}

pub(super) fn conditional_field_requirement(
    when_result_statuses: Vec<&'static str>,
    required_fields: Vec<&'static str>,
    completion_rule: &'static str,
) -> ExternalSectionConditionalFieldRequirement {
    ExternalSectionConditionalFieldRequirement {
        when_result_statuses,
        required_fields,
        completion_rule,
    }
}

pub(super) fn section_validation_stage(
    stage: &'static str,
    accepted_result_statuses: Vec<&'static str>,
    workflow: &[ExternalRecordWorkflowStep],
    up_to_sequence: u8,
    conditional_required_fields: Vec<ExternalSectionConditionalFieldRequirement>,
    required_additional_checks: Vec<&'static str>,
    completion_rule: &'static str,
    blocking_until_satisfied: bool,
) -> ExternalSectionInstanceValidationStage {
    ExternalSectionInstanceValidationStage {
        stage,
        accepted_result_statuses,
        required_workflow_steps: workflow_sequences_up_to(workflow, up_to_sequence),
        required_fields: workflow_completion_fields_up_to(workflow, up_to_sequence),
        conditional_required_fields,
        required_additional_checks,
        completion_rule,
        blocking_until_satisfied,
    }
}

pub(super) fn public_entry_section_instance_validation_contract(
    required_decision_field_contracts: &[ExternalRecordFieldContract],
) -> ExternalSectionInstanceValidationContract {
    let workflow = release_review_section_execution_workflow("public-entry-handoff");
    let lifecycle_transition_contract = public_entry_lifecycle_transition_contract();
    let terminal_states = lifecycle_transition_contract
        .terminal_states_requiring_archive_receipt
        .clone();
    let archive_handoff_contract = release_review_record_archive_handoff_contract(
        "public-entry-handoff",
        "release-review-record reached a terminal Public cutover decision state with the bundled \
         artifact review, required evidence references, and rollback path recorded where \
         applicable",
        terminal_states.clone(),
        terminal_states.clone(),
    );
    let post_archive_writeback_fields = release_review_post_archive_writeback_field_names();
    let exception_record_field_contracts = public_entry_handoff_exception_field_contracts();
    let retention_contract = release_review_record_retention_contract(
        "public-entry-handoff",
        &post_archive_writeback_fields,
    );
    let terminal_mutation_contract = release_review_terminal_mutation_contract(
        "public-entry-handoff",
        &archive_handoff_contract,
        &post_archive_writeback_fields,
    );
    let authority_pairing_checks = public_entry_authority_pairing_checks();
    let execution_boundary_contract = release_review_record_execution_boundary_contract(
        "public-entry-handoff",
        required_decision_field_contracts,
        &exception_record_field_contracts,
        &archive_handoff_contract,
    );

    ExternalSectionInstanceValidationContract {
        record_kind: "release-review-record",
        section_signal: "public-entry-handoff",
        lifecycle_state_field: "result_status",
        validation_scope: "validate one concrete public-entry-handoff section instance inside the \
                           authoritative release-review-record from draft through post-archive \
                           verification",
        snapshot_input_contract: external_section_snapshot_input_contract(
            "public-entry-handoff",
            required_decision_field_contracts,
            &exception_record_field_contracts,
            &archive_handoff_contract,
            &post_archive_writeback_fields,
            vec![
                "field_values.result_status is the authoritative current lifecycle state for the \
                 snapshot under validation",
                "keep field_values keyed by the exact external section field names already \
                 published by required_decision_fields, exception fields, archive receipt fields, \
                 and post-archive writeback fields",
                "prior_result_statuses is only required when field_values.result_status = \
                 rolled-back because the validator must prove that the same section previously \
                 reached cutover-approved",
            ],
        ),
        snapshot_template_contract: snapshot_template_contract(
            "public-entry-handoff",
            required_decision_field_contracts,
            &exception_record_field_contracts,
            &archive_handoff_contract,
            &post_archive_writeback_fields,
            vec![
                "expand field_values with the concrete section fields currently present in the \
                 authoritative external tracker snapshot",
                "omit prior_result_statuses unless the current field_values.result_status \
                 requires lifecycle history proof",
                "use the same exact field names already published in required_decision_fields, \
                 exception fields, archive receipt fields, and post-archive writeback fields",
            ],
        ),
        minimum_snapshot_example: snapshot_example_contract(
            "public-entry-handoff",
            required_decision_field_contracts,
            &exception_record_field_contracts,
            &archive_handoff_contract,
            &post_archive_writeback_fields,
            vec![
                "illustrative only; the example uses a rolled-back path so prior_result_statuses \
                 is visible in the sample input",
                "replace every example artifact id, hostname, timestamp, and archive reference \
                 with rollout-specific truth from the real external section snapshot",
            ],
        ),
        validation_result_contract: validation_result_contract("public-entry-handoff", vec![
            "stage_status=invalid or incomplete must be treated as release-blocking until the \
             snapshot satisfies the required Public cutover stage",
        ]),
        minimum_validation_result_example: validation_result_example("public-entry-handoff", vec![
            "illustrative only; the sample shows a fully valid rolled-back Public section after \
             archive and post-archive verification",
        ]),
        draft_requirements: section_validation_stage(
            "draft-minimum",
            vec!["draft"],
            &workflow,
            1,
            Vec::new(),
            vec![
                "result_status must remain draft until shipped bundle material and runtime \
                 evidence have been linked on the same section",
                "release_reference and section_signal must identify the same rollout unit and \
                 review section in the single authoritative release-review-record",
            ],
            "the section exists and has the minimum identity, authorship, and initial lifecycle \
             fields recorded before evidence collection begins",
            true,
        ),
        evidence_linked_requirements: section_validation_stage(
            "evidence-linked",
            vec!["evidence-linked"],
            &workflow,
            3,
            Vec::new(),
            vec![
                "all shipped Public bundle review fields and /health/compatibility, \
                 /health/ready, /health/backup, and /health/recovery/drill evidence fields must \
                 coexist on the same section before result_status advances to evidence-linked",
                "repo/local bundled baseline may inform review, but it must not be treated as \
                 authoritative proof of the shipped Public artifact under review",
            ],
            "advance to evidence-linked only after the shipped bundle review and runtime evidence \
             are both attached on the same section",
            true,
        ),
        terminal_requirements: section_validation_stage(
            "terminal-decision",
            terminal_states.clone(),
            &workflow,
            4,
            Vec::new(),
            vec![
                "approval_decision must stay aligned with the public-entry lifecycle transition \
                 contract for the terminal result_status recorded on the same section",
                "rolled-back is only valid after the same section was previously cutover-approved",
                "the rollback reference, rollback Public client artifact reference, and rollback \
                 bundled official_entry artifact identity must be fixed on the same section \
                 before Public traffic is reopened or reverted",
            ],
            "a terminal Public cutover section must carry lifecycle-aligned approval data and the \
             full rollback tuple on the same section",
            true,
        ),
        archive_receipt_requirements: section_validation_stage(
            "archive-receipt-linked",
            terminal_states.clone(),
            &workflow,
            5,
            Vec::new(),
            vec![
                "source_record_state must exactly match the terminal result_status written on the \
                 same section",
                "archive handoff must stay correlated to the same release_reference, \
                 section_signal, terminal result_status, shipped bundle tuple, and rollback tuple \
                 published for this reversible rollout unit",
            ],
            "the terminal section is not complete until archive receipt fields are written back \
             on the same section",
            true,
        ),
        post_archive_verification_requirements: section_validation_stage(
            "post-archive-verified",
            terminal_states,
            &workflow,
            6,
            Vec::new(),
            retention_contract.required_post_archive_checks.clone(),
            "append post-archive verification fields on the same section after archive retrieval \
             and retention verification complete",
            false,
        ),
        required_authority_pairing_check_ids: authority_pairing_checks
            .iter()
            .map(|check| check.id)
            .collect(),
        required_archive_correlation_dimensions: archive_handoff_contract
            .required_archive_correlation_dimensions
            .clone(),
        source_record_state_field: "source_record_state",
        blocking_interpretation: "missing draft, evidence-linked, terminal, or archive-receipt \
                                  requirements means the section is not yet a valid Public \
                                  cutover record and must block release approval or closure; \
                                  missing post-archive verification keeps audit closure \
                                  incomplete even if the terminal decision was already archived",
        forbidden_shortcuts: execution_boundary_contract.forbidden_shortcuts,
        forbidden_post_terminal_mutations: terminal_mutation_contract.forbidden_mutations,
    }
}

pub(super) fn governance_section_instance_validation_contract(
    required_decision_field_contracts: &[ExternalRecordFieldContract],
) -> ExternalSectionInstanceValidationContract {
    let workflow = release_review_section_execution_workflow("governance-audit");
    let terminal_states = vec!["approved", "exception-accepted", "rejected", "rolled-back"];
    let archive_handoff_contract = release_review_record_archive_handoff_contract(
        "governance-audit",
        "release-review-record reached a terminal governance review state with findings, \
         decision, and rollback path recorded where applicable",
        terminal_states.clone(),
        terminal_states.clone(),
    );
    let post_archive_writeback_fields = release_review_post_archive_writeback_field_names();
    let exception_record_field_contracts = governance_exception_field_contracts();
    let retention_contract = release_review_record_retention_contract(
        "governance-audit",
        &post_archive_writeback_fields,
    );
    let terminal_mutation_contract = release_review_terminal_mutation_contract(
        "governance-audit",
        &archive_handoff_contract,
        &post_archive_writeback_fields,
    );
    let execution_boundary_contract = release_review_record_execution_boundary_contract(
        "governance-audit",
        required_decision_field_contracts,
        &exception_record_field_contracts,
        &archive_handoff_contract,
    );
    let exception_terminal_fields = vec!["exception_reason", "rollback_reference"];

    ExternalSectionInstanceValidationContract {
        record_kind: "release-review-record",
        section_signal: "governance-audit",
        lifecycle_state_field: "result_status",
        validation_scope: "validate one concrete governance-audit section instance inside the \
                           authoritative release-review-record from draft through post-archive \
                           verification",
        snapshot_input_contract: external_section_snapshot_input_contract(
            "governance-audit",
            required_decision_field_contracts,
            &exception_record_field_contracts,
            &archive_handoff_contract,
            &post_archive_writeback_fields,
            vec![
                "field_values.result_status is the authoritative current lifecycle state for the \
                 governance section snapshot under validation",
                "when field_values.result_status = exception-accepted, the same field_values map \
                 must also retain exception_reason and rollback_reference so the validator can \
                 enforce the exception-only terminal path",
                "prior_result_statuses is only required when field_values.result_status = \
                 rolled-back because the validator must prove that the same section previously \
                 reached a prior terminal governance state",
            ],
        ),
        snapshot_template_contract: snapshot_template_contract(
            "governance-audit",
            required_decision_field_contracts,
            &exception_record_field_contracts,
            &archive_handoff_contract,
            &post_archive_writeback_fields,
            vec![
                "expand field_values with the concrete governance section fields captured from \
                 the external tracker",
                "keep exception-only fields absent unless the current snapshot is validating the \
                 exception-accepted path",
                "omit prior_result_statuses unless the current field_values.result_status \
                 requires lifecycle history proof",
            ],
        ),
        minimum_snapshot_example: snapshot_example_contract(
            "governance-audit",
            required_decision_field_contracts,
            &exception_record_field_contracts,
            &archive_handoff_contract,
            &post_archive_writeback_fields,
            vec![
                "illustrative only; the example uses an approved governance terminal state",
                "replace every note id, timestamp, archive reference, and operator identity with \
                 rollout-specific truth from the real governance snapshot",
            ],
        ),
        validation_result_contract: validation_result_contract("governance-audit", vec![
            "stage_status=invalid or incomplete means the governance section is not yet a valid \
             authoritative review record for the rollout unit",
        ]),
        minimum_validation_result_example: validation_result_example("governance-audit", vec![
            "illustrative only; the sample shows a fully valid approved governance section after \
             archive and post-archive verification",
        ]),
        draft_requirements: section_validation_stage(
            "draft-minimum",
            vec!["draft"],
            &workflow,
            1,
            Vec::new(),
            vec![
                "result_status must remain draft until governance review notes are linked on the \
                 same section",
                "release_reference and section_signal must still identify the same rollout unit \
                 and governance review section in the authoritative external record",
            ],
            "the governance section exists and carries the minimum identity, authorship, and \
             initial lifecycle fields before review evidence is linked",
            true,
        ),
        evidence_linked_requirements: section_validation_stage(
            "findings-linked",
            vec!["findings-linked"],
            &workflow,
            2,
            Vec::new(),
            vec![
                "governance findings and the supporting governance review note must be linked on \
                 the same section before result_status advances to findings-linked",
            ],
            "advance to findings-linked only after the governance review note is recorded on the \
             same section",
            true,
        ),
        terminal_requirements: section_validation_stage(
            "terminal-decision",
            terminal_states.clone(),
            &workflow,
            3,
            vec![conditional_field_requirement(
                vec!["exception-accepted"],
                exception_terminal_fields.clone(),
                "exception-accepted governance posture also requires exception_reason and \
                 rollback_reference on the same section",
            )],
            vec![
                "approval_decision must remain aligned with the governance terminal result_status \
                 recorded on the same section",
                "rolled-back is only valid after the same rollout unit has already passed through \
                 a prior terminal governance decision state",
            ],
            "a terminal governance section must carry the terminal decision on the same section; \
             exception-accepted additionally requires exception_reason and rollback_reference",
            true,
        ),
        archive_receipt_requirements: section_validation_stage(
            "archive-receipt-linked",
            terminal_states.clone(),
            &workflow,
            4,
            vec![conditional_field_requirement(
                vec!["exception-accepted"],
                exception_terminal_fields.clone(),
                "archive-ready exception governance sections must still carry exception_reason \
                 and rollback_reference on the same section",
            )],
            vec![
                "source_record_state must exactly match the terminal result_status written on the \
                 same section",
                "archive handoff must stay correlated to the same release_reference, \
                 section_signal, and terminal result_status for the governance review section",
            ],
            "the terminal governance section is not complete until archive receipt fields are \
             written back on the same section",
            true,
        ),
        post_archive_verification_requirements: section_validation_stage(
            "post-archive-verified",
            terminal_states,
            &workflow,
            5,
            vec![conditional_field_requirement(
                vec!["exception-accepted"],
                exception_terminal_fields,
                "post-archive verification of exception governance sections still requires the \
                 preserved exception_reason and rollback_reference fields",
            )],
            retention_contract.required_post_archive_checks.clone(),
            "append post-archive verification fields on the same governance section after archive \
             retrieval and retention verification complete",
            false,
        ),
        required_authority_pairing_check_ids: Vec::new(),
        required_archive_correlation_dimensions: archive_handoff_contract
            .required_archive_correlation_dimensions
            .clone(),
        source_record_state_field: "source_record_state",
        blocking_interpretation: "missing draft, findings-linked, terminal, or archive-receipt \
                                  requirements means the governance section is not yet a valid \
                                  authoritative review record for the rollout unit; missing \
                                  post-archive verification keeps audit closure incomplete even \
                                  after archiving",
        forbidden_shortcuts: execution_boundary_contract.forbidden_shortcuts,
        forbidden_post_terminal_mutations: terminal_mutation_contract.forbidden_mutations,
    }
}

pub(super) fn management_auth_section_instance_validation_contract(
    required_decision_field_contracts: &[ExternalRecordFieldContract],
) -> ExternalSectionInstanceValidationContract {
    let workflow = release_review_section_execution_workflow("management-auth");
    let terminal_states = vec!["approved", "exception-accepted", "rejected", "rolled-back"];
    let archive_handoff_contract = release_review_record_archive_handoff_contract(
        "management-auth",
        "release-review-record reached a terminal management auth review state with exposure \
         findings, decision, and rollback path recorded where applicable",
        terminal_states.clone(),
        terminal_states.clone(),
    );
    let post_archive_writeback_fields = release_review_post_archive_writeback_field_names();
    let exception_record_field_contracts = management_auth_exception_field_contracts();
    let retention_contract =
        release_review_record_retention_contract("management-auth", &post_archive_writeback_fields);
    let terminal_mutation_contract = release_review_terminal_mutation_contract(
        "management-auth",
        &archive_handoff_contract,
        &post_archive_writeback_fields,
    );
    let execution_boundary_contract = release_review_record_execution_boundary_contract(
        "management-auth",
        required_decision_field_contracts,
        &exception_record_field_contracts,
        &archive_handoff_contract,
    );
    let exception_terminal_fields = vec!["compensating_controls", "rollback_reference"];

    ExternalSectionInstanceValidationContract {
        record_kind: "release-review-record",
        section_signal: "management-auth",
        lifecycle_state_field: "result_status",
        validation_scope: "validate one concrete management-auth section instance inside the \
                           authoritative release-review-record from draft through post-archive \
                           verification",
        snapshot_input_contract: external_section_snapshot_input_contract(
            "management-auth",
            required_decision_field_contracts,
            &exception_record_field_contracts,
            &archive_handoff_contract,
            &post_archive_writeback_fields,
            vec![
                "field_values.result_status is the authoritative current lifecycle state for the \
                 management-auth section snapshot under validation",
                "when field_values.result_status = exception-accepted, the same field_values map \
                 must also retain compensating_controls and rollback_reference so the validator \
                 can enforce the exception-only terminal path",
                "prior_result_statuses is only required when field_values.result_status = \
                 rolled-back because the validator must prove that the same section previously \
                 reached a prior terminal management-auth state",
            ],
        ),
        snapshot_template_contract: snapshot_template_contract(
            "management-auth",
            required_decision_field_contracts,
            &exception_record_field_contracts,
            &archive_handoff_contract,
            &post_archive_writeback_fields,
            vec![
                "expand field_values with the concrete management-auth section fields captured \
                 from the external tracker",
                "keep exception-only fields absent unless the current snapshot is validating the \
                 exception-accepted path",
                "omit prior_result_statuses unless the current field_values.result_status \
                 requires lifecycle history proof",
            ],
        ),
        minimum_snapshot_example: snapshot_example_contract(
            "management-auth",
            required_decision_field_contracts,
            &exception_record_field_contracts,
            &archive_handoff_contract,
            &post_archive_writeback_fields,
            vec![
                "illustrative only; the example uses an approved management-auth terminal state",
                "replace every surface list, timestamp, archive reference, and operator identity \
                 with rollout-specific truth from the real management-auth snapshot",
            ],
        ),
        validation_result_contract: validation_result_contract("management-auth", vec![
            "stage_status=invalid or incomplete means the management-auth section is not yet a \
             valid authoritative review record for the rollout unit",
        ]),
        minimum_validation_result_example: validation_result_example("management-auth", vec![
            "illustrative only; the sample shows a fully valid approved management-auth section \
             after archive and post-archive verification",
        ]),
        draft_requirements: section_validation_stage(
            "draft-minimum",
            vec!["draft"],
            &workflow,
            1,
            Vec::new(),
            vec![
                "result_status must remain draft until reviewed management or observability \
                 surfaces are linked on the same section",
                "release_reference and section_signal must still identify the same rollout unit \
                 and management-auth review section in the authoritative external record",
            ],
            "the management-auth section exists and carries the minimum identity, authorship, and \
             initial lifecycle fields before review evidence is linked",
            true,
        ),
        evidence_linked_requirements: section_validation_stage(
            "findings-linked",
            vec!["findings-linked"],
            &workflow,
            2,
            Vec::new(),
            vec![
                "affected_surfaces must identify the reviewed management or observability \
                 surfaces on the same section before result_status advances to findings-linked",
            ],
            "advance to findings-linked only after the reviewed surfaces are recorded on the same \
             section",
            true,
        ),
        terminal_requirements: section_validation_stage(
            "terminal-decision",
            terminal_states.clone(),
            &workflow,
            3,
            vec![conditional_field_requirement(
                vec!["exception-accepted"],
                exception_terminal_fields.clone(),
                "exception-accepted management-auth posture also requires compensating_controls \
                 and rollback_reference on the same section",
            )],
            vec![
                "approval_decision must remain aligned with the management-auth terminal \
                 result_status recorded on the same section",
                "rolled-back is only valid after the same rollout unit has already passed through \
                 a prior terminal management-auth decision state",
            ],
            "a terminal management-auth section must carry the terminal decision on the same \
             section; exception-accepted additionally requires compensating_controls and \
             rollback_reference",
            true,
        ),
        archive_receipt_requirements: section_validation_stage(
            "archive-receipt-linked",
            terminal_states.clone(),
            &workflow,
            4,
            vec![conditional_field_requirement(
                vec!["exception-accepted"],
                exception_terminal_fields.clone(),
                "archive-ready exception management-auth sections must still carry \
                 compensating_controls and rollback_reference on the same section",
            )],
            vec![
                "source_record_state must exactly match the terminal result_status written on the \
                 same section",
                "archive handoff must stay correlated to the same release_reference, \
                 section_signal, and terminal result_status for the management-auth review section",
            ],
            "the terminal management-auth section is not complete until archive receipt fields \
             are written back on the same section",
            true,
        ),
        post_archive_verification_requirements: section_validation_stage(
            "post-archive-verified",
            terminal_states,
            &workflow,
            5,
            vec![conditional_field_requirement(
                vec!["exception-accepted"],
                exception_terminal_fields,
                "post-archive verification of exception management-auth sections still requires \
                 the preserved compensating_controls and rollback_reference fields",
            )],
            retention_contract.required_post_archive_checks.clone(),
            "append post-archive verification fields on the same management-auth section after \
             archive retrieval and retention verification complete",
            false,
        ),
        required_authority_pairing_check_ids: Vec::new(),
        required_archive_correlation_dimensions: archive_handoff_contract
            .required_archive_correlation_dimensions
            .clone(),
        source_record_state_field: "source_record_state",
        blocking_interpretation: "missing draft, findings-linked, terminal, or archive-receipt \
                                  requirements means the management-auth section is not yet a \
                                  valid authoritative review record for the rollout unit; missing \
                                  post-archive verification keeps audit closure incomplete even \
                                  after archiving",
        forbidden_shortcuts: execution_boundary_contract.forbidden_shortcuts,
        forbidden_post_terminal_mutations: terminal_mutation_contract.forbidden_mutations,
    }
}

pub(super) fn public_entry_handoff_required_review_field_contracts()
-> Vec<ExternalRecordFieldContract> {
    vec![
        external_record_field_contract(
            "reviewed_by",
            "operator-identity",
            "release-operator-attestation",
            "human owner who reviewed the bundled Public entry handoff record",
        ),
        external_record_field_contract(
            "approval_decision",
            "approval-decision-enum",
            "release-operator-attestation",
            "operator decision for whether this bundled Public entry is approved for rollout",
        ),
        external_record_field_contract(
            "decision_recorded_at_utc",
            "utc-timestamp",
            "release-operator-attestation",
            "time when the release review decision was recorded",
        ),
        external_record_field_contract(
            "result_status",
            "result-status-enum",
            "external-release-tracker",
            "current lifecycle state for this public-entry-handoff review section; must match one \
             of the published result_status_model states",
        ),
        external_record_field_contract(
            "bundled_public_client_artifact_reference",
            "release-artifact-reference",
            "external-release-tracker + bundled-client-artifact-review",
            "exact shipped Public client artifact reference whose bundled official_entry content \
             is being reviewed for rollout",
        ),
        external_record_field_contract(
            "bundled_official_entry_artifact_identity",
            "artifact-identity-string",
            "bundled-client-artifact-review",
            "stable identity of the shipped bundled official_entry content inside the referenced \
             Public client artifact",
        ),
        external_record_field_contract(
            "bundled_official_entry_server_address",
            "host-or-socket-address",
            "bundled-client-artifact-review",
            "server address bundled into the Public client artifact",
        ),
        external_record_field_contract(
            "bundled_official_entry_auth_server",
            "url-or-null",
            "bundled-client-artifact-review",
            "exact auth authority pin bundled into the Public client artifact",
        ),
        external_record_field_contract(
            "bundled_official_entry_use_srv",
            "boolean",
            "bundled-client-artifact-review",
            "whether the bundled Public entry resolves the realm via SRV lookup",
        ),
        external_record_field_contract(
            "bundled_official_entry_use_quic",
            "boolean",
            "bundled-client-artifact-review",
            "whether the bundled Public entry expects QUIC transport",
        ),
        external_record_field_contract(
            "bundled_official_entry_validate_tls",
            "boolean",
            "bundled-client-artifact-review",
            "whether the bundled Public entry requires TLS validation",
        ),
        external_record_field_contract(
            "bundled_target_kind",
            "target-kind-enum",
            "client-exported-entry-contract",
            "client-side static interpretation of the bundled official_entry target posture",
        ),
        external_record_field_contract(
            "bundled_target_is_non_local_candidate",
            "boolean",
            "client-exported-entry-contract",
            "whether the bundled target syntactically looks like a non-local rollout candidate",
        ),
        external_record_field_contract(
            "non_local_cutover_ready",
            "boolean",
            "client-exported-entry-contract",
            "whether the bundled Public entry currently satisfies the client-side non-local \
             cutover material gate",
        ),
        external_record_field_contract(
            "non_local_cutover_gap_reasons",
            "string-list",
            "client-exported-entry-contract",
            "client-side gap reasons that still prevent real non-local Public cutover",
        ),
        external_record_field_contract(
            "target_runtime_environment",
            "runtime-environment-enum",
            "/health/compatibility",
            "authoritative server runtime environment for the target rollout",
        ),
        external_record_field_contract(
            "authoritative_compatibility_generation",
            "u16",
            "/health/compatibility",
            "authoritative handshake compatibility generation that the shipped Public entry must \
             target",
        ),
        external_record_field_contract(
            "expected_handshake_auth_mode",
            "server-auth-mode-enum",
            "/health/compatibility",
            "authoritative handshake auth mode that must match the bundled Public auth posture",
        ),
        external_record_field_contract(
            "authoritative_handshake_auth_provider",
            "url-or-null",
            "/health/compatibility",
            "authoritative realm handshake auth_provider that the bundled Public auth pin must \
             exactly match for the target rollout",
        ),
        external_record_field_contract(
            "query_auth_required_hint",
            "boolean",
            "/health/compatibility",
            "published query-server auth_required hint recorded alongside the authoritative \
             handshake contract",
        ),
        external_record_field_contract(
            "ready_report_status",
            "ready-status-enum",
            "/health/ready",
            "observed readiness status at the time of the rollout decision",
        ),
        external_record_field_contract(
            "backup_evidence_reference",
            "evidence-reference",
            "/health/backup",
            "reference to the backup evidence record supporting this cutover",
        ),
        external_record_field_contract(
            "recovery_drill_reference",
            "evidence-reference",
            "/health/recovery/drill",
            "reference to a recovery drill record that reached a rollout-acceptable state",
        ),
        external_record_field_contract(
            "rollback_public_client_artifact_reference",
            "release-artifact-reference",
            "external-release-tracker",
            "Public client artifact reference that rollback will restore if this rollout is \
             reverted",
        ),
        external_record_field_contract(
            "rollback_bundled_official_entry_artifact_identity",
            "artifact-identity-string",
            "external-release-tracker",
            "bundled official_entry artifact identity that rollback will restore if this Public \
             cutover is reverted",
        ),
        external_record_field_contract(
            "rollback_reference",
            "release-or-runbook-reference",
            "external-release-tracker",
            "rollback path to use if the bundled Public cutover is rejected or reverted",
        ),
        external_record_field_contract(
            "release_reference",
            "release-reference",
            "external-release-tracker",
            "release tracker identifier that ties the bundled Public review record to the rollout \
             event",
        ),
    ]
}

pub(super) fn public_entry_handoff_exception_field_contracts() -> Vec<ExternalRecordFieldContract> {
    vec![
        external_record_field_contract(
            "bundled_public_client_artifact_reference",
            "release-artifact-reference",
            "external-release-tracker + bundled-client-artifact-review",
            "Public client artifact reference covered by the exception record",
        ),
        external_record_field_contract(
            "bundled_official_entry_artifact_identity",
            "artifact-identity-string",
            "bundled-client-artifact-review",
            "identity of the bundled Public entry artifact covered by the exception record",
        ),
        external_record_field_contract(
            "bundled_auth_pin_review_reference",
            "review-note-reference",
            "external-release-tracker",
            "reference to the review note reserved for bundled auth pin exception handling; \
             public-entry-handoff does not yet publish a valid exception-accepted terminal path",
        ),
        external_record_field_contract(
            "recovery_drill_reference",
            "evidence-reference",
            "/health/recovery/drill",
            "recovery drill evidence linked to the exception decision",
        ),
        external_record_field_contract(
            "rollback_public_client_artifact_reference",
            "release-artifact-reference",
            "external-release-tracker",
            "rollback Public client artifact reference linked to the exception decision",
        ),
        external_record_field_contract(
            "rollback_reference",
            "release-or-runbook-reference",
            "external-release-tracker",
            "rollback path linked to the exception decision",
        ),
    ]
}

pub(super) fn governance_required_decision_field_contracts() -> Vec<ExternalRecordFieldContract> {
    vec![
        external_record_field_contract(
            "reviewed_by",
            "operator-identity",
            "release-operator-attestation",
            "human owner who accepted or rejected the governance findings",
        ),
        external_record_field_contract(
            "approval_decision",
            "approval-decision-enum",
            "release-operator-attestation",
            "operator decision for the governance review",
        ),
        external_record_field_contract(
            "decision_recorded_at_utc",
            "utc-timestamp",
            "release-operator-attestation",
            "time when the governance review decision was recorded",
        ),
        external_record_field_contract(
            "result_status",
            "result-status-enum",
            "external-release-tracker",
            "current lifecycle state for this governance review section; must match one of the \
             published result_status_model states",
        ),
        external_record_field_contract(
            "release_reference",
            "release-reference",
            "external-release-tracker",
            "release tracker identifier tied to the governance decision",
        ),
        external_record_field_contract(
            "governance_note_reference",
            "review-note-reference",
            "external-release-tracker",
            "reference to the governance note or ticket backing the governance review decision",
        ),
    ]
}

pub(super) fn governance_exception_field_contracts() -> Vec<ExternalRecordFieldContract> {
    vec![
        external_record_field_contract(
            "exception_reason",
            "freeform-text",
            "external-release-tracker",
            "operator rationale for accepting a governance exception",
        ),
        external_record_field_contract(
            "rollback_reference",
            "release-or-runbook-reference",
            "external-release-tracker",
            "rollback path linked to the governance exception",
        ),
    ]
}

pub(super) fn management_auth_required_decision_field_contracts() -> Vec<ExternalRecordFieldContract>
{
    vec![
        external_record_field_contract(
            "reviewed_by",
            "operator-identity",
            "release-operator-attestation",
            "human owner who reviewed remote management and observability auth posture",
        ),
        external_record_field_contract(
            "approval_decision",
            "approval-decision-enum",
            "release-operator-attestation",
            "operator decision for management auth exposure review",
        ),
        external_record_field_contract(
            "decision_recorded_at_utc",
            "utc-timestamp",
            "release-operator-attestation",
            "time when the management auth review decision was recorded",
        ),
        external_record_field_contract(
            "result_status",
            "result-status-enum",
            "external-release-tracker",
            "current lifecycle state for this management auth review section; must match one of \
             the published result_status_model states",
        ),
        external_record_field_contract(
            "release_reference",
            "release-reference",
            "external-release-tracker",
            "release tracker identifier tied to the management auth decision",
        ),
        external_record_field_contract(
            "affected_surfaces",
            "string-list",
            "/health/management-auth",
            "management or observability surfaces covered by the review decision",
        ),
    ]
}

pub(super) fn management_auth_exception_field_contracts() -> Vec<ExternalRecordFieldContract> {
    vec![
        external_record_field_contract(
            "compensating_controls",
            "freeform-text",
            "external-release-tracker",
            "operator-documented compensating controls for the accepted exposure",
        ),
        external_record_field_contract(
            "rollback_reference",
            "release-or-runbook-reference",
            "external-release-tracker",
            "rollback path linked to the management auth exception",
        ),
    ]
}

pub(super) fn release_review_record_lifecycle_contract(
    minimum_complete_record_field_contracts: &[ExternalRecordFieldContract],
) -> ExternalRecordLifecycleContract {
    ExternalRecordLifecycleContract {
        authoritative_record_owner: "release-operator",
        authoritative_record_authority: "external-release-tracker",
        canonical_record_location: "single authoritative release-review-record keyed by \
                                    release_reference in the external release tracker",
        instance_scope: "one release-review-record per rollout unit; keep one section per review \
                         signal such as public-entry-handoff, governance-audit, or \
                         management-auth within that same record",
        write_mode: "same external record updated across review lifecycle state transitions",
        record_granularity: "one decision-scope section per review signal within the \
                             authoritative external release-review-record",
        minimum_complete_record_fields: external_record_field_names(
            minimum_complete_record_field_contracts,
        ),
        same_record_must_link_rollout_and_rollback: true,
        in_process_maintenance: "none",
    }
}

pub(super) fn release_review_archive_correlation_dimensions(
    signal: &'static str,
) -> Vec<&'static str> {
    match signal {
        "public-entry-handoff" => vec![
            "release_reference",
            "section_signal",
            "terminal_result_status",
            "bundled_public_client_artifact_reference",
            "bundled_official_entry_artifact_identity",
            "rollback_reference",
            "rollback_public_client_artifact_reference",
            "rollback_bundled_official_entry_artifact_identity",
        ],
        _ => vec![
            "release_reference",
            "section_signal",
            "terminal_result_status",
        ],
    }
}

pub(super) fn release_review_record_archive_handoff_contract(
    signal: &'static str,
    record_completion_signal: &'static str,
    handoff_ready_states: Vec<&'static str>,
    terminal_states_requiring_archive_receipt: Vec<&'static str>,
) -> ExternalRecordArchiveHandoffContract {
    ExternalRecordArchiveHandoffContract {
        authoritative_archive_owner: "external-release-tracker",
        record_completion_signal,
        handoff_ready_states,
        terminal_states_requiring_archive_receipt,
        required_archive_receipt_fields: release_review_archive_receipt_field_names(),
        required_archive_correlation_dimensions: release_review_archive_correlation_dimensions(
            signal,
        ),
        source_record_state_must_match_section_result_status: true,
        terminal_section_not_complete_without_archive_receipt: true,
        external_record_not_sufficient_without_archive: true,
    }
}

pub(super) fn release_review_archive_receipt_field_names() -> Vec<&'static str> {
    vec![
        "archive_reference",
        "archived_at_utc",
        "archived_by",
        "source_record_state",
    ]
}

pub(super) fn release_review_record_retention_contract(
    signal: &'static str,
    post_archive_writeback_fields: &[&'static str],
) -> ExternalRecordRetentionContract {
    ExternalRecordRetentionContract {
        authoritative_retention_owner: "external-release-tracker",
        authoritative_storage_scope: "authoritative review record plus archived terminal record \
                                      material kept outside this process",
        retention_policy: match signal {
            "public-entry-handoff" => {
                "retain the authoritative Public cutover review and its archived terminal state \
                 for the full rollback, incident review, and audit window defined by external \
                 release governance"
            },
            "governance-audit" => {
                "retain governance review decisions and accepted exceptions for the full audit and \
                 rollback accountability window defined by external release governance"
            },
            "management-auth" => {
                "retain management auth exposure decisions and accepted exceptions for the full \
                 audit and rollback accountability window defined by external release governance"
            },
            _ => {
                "retain the authoritative external review record for the full rollback and audit \
                 accountability window defined by external release governance"
            },
        },
        minimum_retention_window: "at least through the complete rollback window and the \
                                   subsequent audit / incident review period required by the \
                                   operator",
        immutability_expectation: "after archive receipt is written, treat archived terminal \
                                   snapshots as immutable; append superseding review history \
                                   instead of rewriting archived history",
        replication_expectation: "the external archive must preserve at least one \
                                  operator-restorable copy independent from the live tracker UI \
                                  or transient local files",
        post_archive_verification_required: true,
        required_post_archive_checks: vec![
            "archive receipt fields are written back to the same release-review-record section",
            "post_archive_verified_by, post_archive_verified_at_utc, \
             post_archive_verification_result, and post_archive_verification_reference are \
             appended to the same release-review-record section after archive retrieval and \
             retention checks complete",
            "archive_reference resolves to retrievable archived review material",
            "source_record_state matches the terminal result_status recorded for the section",
            "release_reference still resolves to the same rollout unit across tracker and archive",
            match signal {
                "public-entry-handoff" => {
                    "archive material stays correlated to the same release_reference, \
                     section_signal, terminal result_status, \
                     bundled_public_client_artifact_reference, and \
                     bundled_official_entry_artifact_identity reviewed for cutover, together with \
                     rollback_reference, rollback_public_client_artifact_reference, and \
                     rollback_bundled_official_entry_artifact_identity for the same reversible \
                     rollout unit"
                },
                _ => {
                    "archive material stays correlated to the same release_reference, \
                     section_signal, and terminal result_status captured for the section"
                },
            },
            "after terminal capture, only append-only archive receipt or explicit superseding \
             follow-up updates are allowed; decision evidence must not be silently rewritten",
        ],
        required_post_archive_writeback_fields: post_archive_writeback_fields.to_vec(),
        post_archive_writeback_target: "append post-archive verification fields to the same \
                                        section in the authoritative external-release-tracker \
                                        after archive receipt writeback",
        local_process_role: "publish minimum retention and post-archive verification expectations \
                             only; this process does not keep the authoritative long-term review \
                             record",
    }
}

pub(super) fn release_review_terminal_mutation_contract(
    signal: &'static str,
    archive_handoff_contract: &ExternalRecordArchiveHandoffContract,
    post_archive_writeback_fields: &[&'static str],
) -> ExternalRecordTerminalMutationContract {
    let mut allowed_append_only_updates = archive_handoff_contract
        .required_archive_receipt_fields
        .clone();
    for field in post_archive_writeback_fields {
        if !allowed_append_only_updates
            .iter()
            .any(|existing| existing == field)
        {
            allowed_append_only_updates.push(*field);
        }
    }
    ExternalRecordTerminalMutationContract {
        immutable_after_states: archive_handoff_contract
            .terminal_states_requiring_archive_receipt
            .clone(),
        allowed_append_only_updates,
        allowed_follow_up_actions: match signal {
            "public-entry-handoff" => vec![
                "append archive receipt fields and post-archive verification fields after the \
                 terminal Public cutover section is archived",
                "append an explicit correction or superseding review reference if archived \
                 metadata must be clarified",
                "append an explicit rollback or superseding follow-up on the same \
                 release-review-record instead of silently rewriting the archived cutover decision",
            ],
            "governance-audit" => vec![
                "append archive receipt fields and post-archive verification fields after the \
                 terminal governance section is archived",
                "append an explicit correction or superseding governance reference if archived \
                 metadata must be clarified",
                "append a new explicit governance follow-up on the same release-review-record \
                 instead of silently rewriting archived governance posture",
            ],
            "management-auth" => vec![
                "append archive receipt fields and post-archive verification fields after the \
                 terminal management-auth section is archived",
                "append an explicit correction or superseding management-auth reference if \
                 archived metadata must be clarified",
                "append a new explicit management-auth follow-up on the same \
                 release-review-record instead of silently rewriting archived auth posture",
            ],
            _ => vec![
                "append archive receipt fields and post-archive verification fields after the \
                 terminal review section is archived",
                "append an explicit correction or superseding review reference if archived \
                 metadata must be clarified",
            ],
        },
        forbidden_mutations: match signal {
            "public-entry-handoff" => vec![
                "do not overwrite release_reference, bundled_official_entry_artifact_identity, \
                 bundled_public_client_artifact_reference, \
                 rollback_public_client_artifact_reference, bundled target posture, or terminal \
                 decision evidence after terminal capture",
                "do not change a terminal result_status in place once archive receipt is written",
                "do not rewrite archive receipt or post-archive verification evidence in place \
                 once it has been appended; record an explicit superseding follow-up instead",
                "do not replace archived Public cutover history with a new bundle review without \
                 recording an explicit superseding follow-up",
            ],
            _ => vec![
                "do not overwrite release_reference, decision evidence, or exception rationale \
                 after terminal capture",
                "do not change a terminal result_status in place once archive receipt is written",
                "do not rewrite archive receipt or post-archive verification evidence in place \
                 once it has been appended; record an explicit superseding follow-up instead",
                "do not replace archived terminal history with silent field edits; record an \
                 explicit superseding follow-up instead",
            ],
        },
        superseding_change_rule: match signal {
            "public-entry-handoff" => {
                "if a Public cutover decision later changes, preserve the archived terminal \
                 snapshot and record the new outcome as an explicit follow-up transition or \
                 superseding section history entry tied to the same release_reference"
            },
            _ => {
                "if a terminal review decision later changes, preserve the archived terminal \
                 snapshot and record the new outcome as an explicit follow-up transition or \
                 superseding section history entry tied to the same release_reference"
            },
        },
    }
}

pub(super) fn archive_correlation_dimension_record_field(dimension: &'static str) -> &'static str {
    match dimension {
        "terminal_result_status" => "result_status",
        other => other,
    }
}

pub(super) fn release_review_terminal_snapshot_record_fields(
    required_decision_field_contracts: &[ExternalRecordFieldContract],
    archive_handoff_contract: &ExternalRecordArchiveHandoffContract,
) -> Vec<&'static str> {
    let mut fields = external_record_field_names(required_decision_field_contracts);
    if !fields.iter().any(|field| *field == "section_signal") {
        fields.push("section_signal");
    }
    for dimension in &archive_handoff_contract.required_archive_correlation_dimensions {
        let field = archive_correlation_dimension_record_field(dimension);
        if !fields.iter().any(|existing| *existing == field) {
            fields.push(field);
        }
    }
    fields
}

pub(super) fn release_review_record_execution_boundary_contract(
    signal: &'static str,
    required_decision_field_contracts: &[ExternalRecordFieldContract],
    exception_record_field_contracts: &[ExternalRecordFieldContract],
    archive_handoff_contract: &ExternalRecordArchiveHandoffContract,
) -> ExternalRecordExecutionBoundaryContract {
    ExternalRecordExecutionBoundaryContract {
        authoritative_live_record_system: "external-release-tracker",
        authoritative_live_record_scope: "mutable working section inside the authoritative \
                                          release-review-record keyed by release_reference and \
                                          section_signal",
        terminal_snapshot_materialization_owner: "release-operator",
        terminal_snapshot_source: "same review section in the external-release-tracker at the \
                                   moment it reaches a terminal result_status",
        minimum_terminal_snapshot_record_fields: release_review_terminal_snapshot_record_fields(
            required_decision_field_contracts,
            archive_handoff_contract,
        ),
        conditional_terminal_snapshot_record_fields: external_record_field_names(
            exception_record_field_contracts,
        ),
        authoritative_archive_system: "external archive referenced by archive_reference and kept \
                                       outside this process",
        archive_receipt_writeback_target: "write archive receipt fields back to the same section \
                                           in the authoritative external-release-tracker",
        required_system_separation: vec![
            "treat the mutable live tracker section and the immutable archived terminal snapshot \
             as separate storage responsibilities even when the same external product links them",
            "archive_reference must resolve to retrievable archived material independent from the \
             live tracker UI state",
            "this process and local evidence sinks do not store the authoritative \
             release-review-record or its immutable archive copy",
        ],
        forbidden_shortcuts: match signal {
            "public-entry-handoff" => vec![
                "do not treat a terminal state in the live tracker alone as sufficient archive \
                 completion",
                "do not archive a Public cutover section without the shipped Public client \
                 artifact reference, bundled artifact identity, rollback path, rollback Public \
                 client artifact reference, and rollback bundled official_entry artifact identity \
                 that define the reversible rollout unit",
                "do not use a local ronl evidence file or this process output as a substitute for \
                 the authoritative external review record or archive snapshot",
            ],
            _ => vec![
                "do not treat a terminal state in the live tracker alone as sufficient archive \
                 completion",
                "do not archive a terminal review section without the decision fields that define \
                 the rollout unit and terminal outcome",
                "do not use a local ronl evidence file or this process output as a substitute for \
                 the authoritative external review record or archive snapshot",
            ],
        },
    }
}

pub(super) fn public_entry_handoff_result_status_model() -> Vec<ResultStatusContract> {
    vec![
        ResultStatusContract {
            state: "draft",
            semantics: "review record opened but bundled entry review or required health evidence \
                        is still incomplete",
        },
        ResultStatusContract {
            state: "evidence-linked",
            semantics: "bundled client artifact review plus compatibility, ready, backup, and \
                        recovery evidence references have been linked",
        },
        ResultStatusContract {
            state: "cutover-approved",
            semantics: "operators approved reopening non-local Public traffic for this bundled \
                        entry",
        },
        ResultStatusContract {
            state: "cutover-rejected",
            semantics: "operators rejected or deferred the bundled Public cutover",
        },
        ResultStatusContract {
            state: "rolled-back",
            semantics: "same release review record updated after an approved Public cutover was \
                        reverted",
        },
    ]
}

pub(super) fn governance_review_result_status_model() -> Vec<ResultStatusContract> {
    vec![
        ResultStatusContract {
            state: "draft",
            semantics: "governance review section opened but findings have not been resolved yet",
        },
        ResultStatusContract {
            state: "findings-linked",
            semantics: "governance findings and supporting review notes have been linked to the \
                        record",
        },
        ResultStatusContract {
            state: "approved",
            semantics: "governance review cleared without exceptions",
        },
        ResultStatusContract {
            state: "exception-accepted",
            semantics: "governance exception accepted with compensating notes and rollback path \
                        recorded",
        },
        ResultStatusContract {
            state: "rejected",
            semantics: "governance review rejected the rollout",
        },
        ResultStatusContract {
            state: "rolled-back",
            semantics: "same release review record updated after the rollout reverted while \
                        governance review remained part of the release decision chain",
        },
    ]
}

pub(super) fn management_auth_review_result_status_model() -> Vec<ResultStatusContract> {
    vec![
        ResultStatusContract {
            state: "draft",
            semantics: "management auth review section opened but remote exposure posture is not \
                        yet fully assessed",
        },
        ResultStatusContract {
            state: "findings-linked",
            semantics: "reviewed management or observability exposure findings have been linked \
                        to the record",
        },
        ResultStatusContract {
            state: "approved",
            semantics: "management auth posture approved without exceptions",
        },
        ResultStatusContract {
            state: "exception-accepted",
            semantics: "management auth exception accepted with compensating controls and \
                        rollback path recorded",
        },
        ResultStatusContract {
            state: "rejected",
            semantics: "management auth review rejected the rollout",
        },
        ResultStatusContract {
            state: "rolled-back",
            semantics: "same release review record updated after the rollout reverted while \
                        management auth review remained part of the release decision chain",
        },
    ]
}
