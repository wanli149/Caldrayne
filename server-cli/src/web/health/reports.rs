use common::official_entry::BundledOfficialEntryPosture;
use serde::Serialize;

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct HealthCheck {
    pub(in crate::web) name: String,
    pub(in crate::web) ok: bool,
    pub(in crate::web) required: bool,
    pub(in crate::web) detail: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct LiveReport {
    pub(in crate::web) status: &'static str,
    pub(in crate::web) environment: &'static str,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct ReadyReport {
    pub(in crate::web) status: &'static str,
    pub(in crate::web) environment: &'static str,
    pub(in crate::web) checks: Vec<HealthCheck>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct HealthEndpointContract {
    pub(in crate::web) path: &'static str,
    pub(in crate::web) signal: &'static str,
    pub(in crate::web) success_status: u16,
    pub(in crate::web) failure_status: Option<u16>,
    pub(in crate::web) semantics: &'static str,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct HealthContract {
    pub(in crate::web) surface: &'static str,
    pub(in crate::web) environment: &'static str,
    pub(in crate::web) consumption: &'static str,
    pub(in crate::web) cache_policy: &'static str,
    pub(in crate::web) endpoints: Vec<HealthEndpointContract>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct GovernanceReport {
    pub(in crate::web) status: &'static str,
    pub(in crate::web) environment: &'static str,
    pub(in crate::web) findings: Vec<crate::settings::RuntimeGovernanceFinding>,
    pub(in crate::web) requires_operator_review: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct AuthoritativeHandshakeReport {
    pub(in crate::web) surface: &'static str,
    pub(in crate::web) authority_scope: &'static str,
    pub(in crate::web) environment_truth: &'static str,
    pub(in crate::web) compatibility_generation: u16,
    pub(in crate::web) minimum_supported_generation: u16,
    pub(in crate::web) build_identity_fields: Vec<&'static str>,
    pub(in crate::web) auth_signal: &'static str,
    pub(in crate::web) auth_mode: &'static str,
    pub(in crate::web) auth_provider: Option<String>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct QueryCompatibilityHintReport {
    pub(in crate::web) surface: &'static str,
    pub(in crate::web) authority_scope: &'static str,
    pub(in crate::web) environment_hint: &'static str,
    pub(in crate::web) compatibility_generation: u16,
    pub(in crate::web) minimum_supported_generation: u16,
    pub(in crate::web) auth_required: bool,
    pub(in crate::web) auth_hint_scope: &'static str,
    pub(in crate::web) protocol_version: u16,
    pub(in crate::web) version_selection_policy: &'static str,
    pub(in crate::web) supports_multi_version_negotiation: bool,
    pub(in crate::web) published_protocol_fields: Vec<&'static str>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct QueryProtocolRolloutContract {
    pub(in crate::web) protocol_version: u16,
    pub(in crate::web) version_selection_policy: &'static str,
    pub(in crate::web) supports_multi_version_negotiation: bool,
    pub(in crate::web) requires_lockstep_rollout: bool,
    pub(in crate::web) current_stage_policy: &'static str,
    pub(in crate::web) policy_change_requirement: &'static str,
    pub(in crate::web) authoritative_client_path: &'static str,
    pub(in crate::web) known_in_repo_consumers: Vec<&'static str>,
    pub(in crate::web) mixed_version_policy: &'static str,
    pub(in crate::web) safe_transition_options: Vec<&'static str>,
    pub(in crate::web) upgrade_order: Vec<&'static str>,
    pub(in crate::web) rollback_order: Vec<&'static str>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct CutoverMaterialChecklistItem {
    pub(in crate::web) id: &'static str,
    pub(in crate::web) owner: &'static str,
    pub(in crate::web) evidence_source: &'static str,
    pub(in crate::web) required_for_cutover: bool,
    pub(in crate::web) completion_criteria: &'static str,
    pub(in crate::web) current_repo_baseline: &'static str,
    pub(in crate::web) current_stage_status: &'static str,
    pub(in crate::web) current_stage_detail: String,
    pub(in crate::web) operator_next_step: &'static str,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct PublicEntryTransitionContract {
    pub(in crate::web) transition_scope: &'static str,
    pub(in crate::web) record_scope: &'static str,
    pub(in crate::web) atomic_bundle_fields: Vec<&'static str>,
    pub(in crate::web) atomic_runtime_gate_fields: Vec<&'static str>,
    pub(in crate::web) atomic_rollback_restore_fields: Vec<&'static str>,
    pub(in crate::web) forbidden_partial_transitions: Vec<&'static str>,
    pub(in crate::web) approval_gate: &'static str,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct PublicEntryLifecycleTransition {
    pub(in crate::web) from_state: &'static str,
    pub(in crate::web) to_state: &'static str,
    pub(in crate::web) approval_decision: Option<&'static str>,
    pub(in crate::web) required_fields: Vec<&'static str>,
    pub(in crate::web) archive_required: bool,
    pub(in crate::web) notes: Vec<&'static str>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct PublicEntryLifecycleTransitionContract {
    pub(in crate::web) lifecycle_scope: &'static str,
    pub(in crate::web) initial_state: &'static str,
    pub(in crate::web) evidence_ready_state: &'static str,
    pub(in crate::web) terminal_states_requiring_archive_receipt: Vec<&'static str>,
    pub(in crate::web) unsupported_paths: Vec<&'static str>,
    pub(in crate::web) transitions: Vec<PublicEntryLifecycleTransition>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct ExternalRecordAuthorityPairingCheck {
    pub(in crate::web) id: &'static str,
    pub(in crate::web) review_fields: Vec<&'static str>,
    pub(in crate::web) evidence_sources: Vec<&'static str>,
    pub(in crate::web) required_match: &'static str,
    pub(in crate::web) release_blocking_on_mismatch: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct ExternalExecutionDependencyReport {
    pub(in crate::web) id: &'static str,
    pub(in crate::web) owner: &'static str,
    pub(in crate::web) blocks_development_stage_closure: bool,
    pub(in crate::web) blocks_real_cutover: bool,
    pub(in crate::web) current_stage_status: &'static str,
    pub(in crate::web) detail: String,
    pub(in crate::web) operator_next_step: &'static str,
    pub(in crate::web) supporting_endpoints: Vec<&'static str>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct PublicEntryHandoffReport {
    pub(in crate::web) signal: &'static str,
    pub(in crate::web) status: &'static str,
    pub(in crate::web) applies_to_non_local_public_rollout: bool,
    pub(in crate::web) requires_operator_review: bool,
    pub(in crate::web) release_blocked: bool,
    pub(in crate::web) development_stage_closure_available_without_real_materials: bool,
    pub(in crate::web) development_stage_closure_status: &'static str,
    pub(in crate::web) development_stage_closure_scope: &'static str,
    pub(in crate::web) real_cutover_still_requires_external_materials: bool,
    pub(in crate::web) real_cutover_execution_status: &'static str,
    pub(in crate::web) real_cutover_dependency_boundary: &'static str,
    pub(in crate::web) remaining_external_execution_dependencies:
        Vec<ExternalExecutionDependencyReport>,
    pub(in crate::web) authority_scope: &'static str,
    pub(in crate::web) authoritative_public_target_path: &'static str,
    pub(in crate::web) authoritative_public_auth_path: &'static str,
    pub(in crate::web) expected_handshake_auth_mode: &'static str,
    pub(in crate::web) authoritative_handshake_auth_provider: Option<String>,
    pub(in crate::web) query_auth_requirement_hint: bool,
    pub(in crate::web) machine_verification_available_in_this_process: bool,
    pub(in crate::web) machine_verification_scope: &'static str,
    pub(in crate::web) machine_verification_limitations: &'static str,
    pub(in crate::web) repo_bundled_official_entry_snapshot: RepoBundledOfficialEntrySnapshotReport,
    pub(in crate::web) required_external_review_fields: Vec<&'static str>,
    pub(in crate::web) required_external_review_field_contracts: Vec<ExternalRecordFieldContract>,
    pub(in crate::web) required_cutover_preconditions: Vec<&'static str>,
    pub(in crate::web) required_cutover_material_checklist: Vec<CutoverMaterialChecklistItem>,
    pub(in crate::web) public_entry_transition_contract: Option<PublicEntryTransitionContract>,
    pub(in crate::web) public_entry_lifecycle_transition_contract:
        Option<PublicEntryLifecycleTransitionContract>,
    pub(in crate::web) section_instance_validation_contract:
        Option<ExternalSectionInstanceValidationContract>,
    pub(in crate::web) required_authority_pairing_checks: Vec<ExternalRecordAuthorityPairingCheck>,
    pub(in crate::web) supporting_health_endpoints: Vec<&'static str>,
    pub(in crate::web) semantics: &'static str,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct RepoBundledOfficialEntrySnapshotReport {
    pub(in crate::web) status: &'static str,
    pub(in crate::web) evidence_scope: &'static str,
    pub(in crate::web) load_source: &'static str,
    pub(in crate::web) authoritative_for_release_cutover: bool,
    pub(in crate::web) required_external_match_fields: Vec<&'static str>,
    pub(in crate::web) baseline: Option<BundledOfficialEntryPosture>,
    pub(in crate::web) load_error: Option<String>,
    pub(in crate::web) semantics: &'static str,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct CompatibilityContractReport {
    pub(in crate::web) status: &'static str,
    pub(in crate::web) environment: &'static str,
    pub(in crate::web) authoritative_handshake: AuthoritativeHandshakeReport,
    pub(in crate::web) query_hint: QueryCompatibilityHintReport,
    pub(in crate::web) query_protocol_rollout: QueryProtocolRolloutContract,
    pub(in crate::web) public_entry_handoff: PublicEntryHandoffReport,
    pub(in crate::web) environment_matches: bool,
    pub(in crate::web) compatibility_matches: bool,
    pub(in crate::web) auth_requirement_matches_runtime_config: bool,
    pub(in crate::web) shared_truth_builder: &'static str,
    pub(in crate::web) operator_consumption: &'static str,
    pub(in crate::web) mismatch_effect: &'static str,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct SurfaceEntryReport {
    pub(in crate::web) surface: &'static str,
    pub(in crate::web) bind_address: Option<String>,
    pub(in crate::web) reachability: &'static str,
    pub(in crate::web) auth_scheme: &'static str,
    pub(in crate::web) credential_bootstrap: &'static str,
    pub(in crate::web) review_status: &'static str,
    pub(in crate::web) remote_exposure_policy: &'static str,
    pub(in crate::web) purpose: &'static str,
    pub(in crate::web) consumption: &'static str,
    pub(in crate::web) authority_scope: Option<&'static str>,
    pub(in crate::web) published_protocol_fields: Vec<&'static str>,
    pub(in crate::web) auth_required: Option<bool>,
    pub(in crate::web) detail: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct SurfaceInventoryReport {
    pub(in crate::web) status: &'static str,
    pub(in crate::web) environment: &'static str,
    pub(in crate::web) entries: Vec<SurfaceEntryReport>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct ManagementAuthEntryReport {
    pub(in crate::web) surface: &'static str,
    pub(in crate::web) bind_address: Option<String>,
    pub(in crate::web) reachability: &'static str,
    pub(in crate::web) review_status: &'static str,
    pub(in crate::web) remote_exposure_policy: &'static str,
    pub(in crate::web) capability: &'static str,
    pub(in crate::web) auth_scheme: &'static str,
    pub(in crate::web) credential_bootstrap: &'static str,
    pub(in crate::web) credential_transport: &'static str,
    pub(in crate::web) secret_config_id: Option<&'static str>,
    pub(in crate::web) proxy_forwarding_forbidden: bool,
    pub(in crate::web) detail: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct ManagementAuthReport {
    pub(in crate::web) status: &'static str,
    pub(in crate::web) environment: &'static str,
    pub(in crate::web) requires_operator_review: bool,
    pub(in crate::web) review_surfaces: Vec<&'static str>,
    pub(in crate::web) entries: Vec<ManagementAuthEntryReport>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct TransportSecurityEntryReport {
    pub(in crate::web) surface: &'static str,
    pub(in crate::web) bind_address: String,
    pub(in crate::web) transport: &'static str,
    pub(in crate::web) encryption: &'static str,
    pub(in crate::web) cert_file_path: String,
    pub(in crate::web) key_file_path: String,
    pub(in crate::web) rollout_policy: &'static str,
    pub(in crate::web) validation_policy: &'static str,
    pub(in crate::web) material_state: &'static str,
    pub(in crate::web) detail: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct TransportSecurityReport {
    pub(in crate::web) status: &'static str,
    pub(in crate::web) environment: &'static str,
    pub(in crate::web) entries: Vec<TransportSecurityEntryReport>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct RuntimeListenerEntryReport {
    pub(in crate::web) surface: &'static str,
    pub(in crate::web) bind_address: String,
    pub(in crate::web) state: &'static str,
    pub(in crate::web) detail: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct RuntimeListenerReport {
    pub(in crate::web) status: &'static str,
    pub(in crate::web) environment: &'static str,
    pub(in crate::web) entries: Vec<RuntimeListenerEntryReport>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct RuntimeObservabilityEntryReport {
    pub(in crate::web) surface: &'static str,
    pub(in crate::web) state: &'static str,
    pub(in crate::web) detail: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct RuntimeObservabilityReport {
    pub(in crate::web) status: &'static str,
    pub(in crate::web) environment: &'static str,
    pub(in crate::web) requires_operator_review: bool,
    pub(in crate::web) entries: Vec<RuntimeObservabilityEntryReport>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct PreflightComponentReport {
    pub(in crate::web) signal: &'static str,
    pub(in crate::web) endpoint: &'static str,
    pub(in crate::web) status: &'static str,
    pub(in crate::web) blocking: bool,
    pub(in crate::web) requires_operator_review: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct PreflightReviewItem {
    pub(in crate::web) kind: &'static str,
    pub(in crate::web) blocking: bool,
    pub(in crate::web) detail: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct PreflightFollowUpEndpoint {
    pub(in crate::web) signal: &'static str,
    pub(in crate::web) endpoint: &'static str,
    pub(in crate::web) blocking: bool,
    pub(in crate::web) owner: &'static str,
    pub(in crate::web) reason: &'static str,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct PreflightSupportingEndpoint {
    pub(in crate::web) signal: &'static str,
    pub(in crate::web) endpoint: &'static str,
    pub(in crate::web) owner: &'static str,
    pub(in crate::web) purpose: &'static str,
    pub(in crate::web) related_findings: Vec<&'static str>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct ExternalRecordFieldContract {
    pub(in crate::web) name: &'static str,
    pub(in crate::web) value_kind: &'static str,
    pub(in crate::web) evidence_source: &'static str,
    pub(in crate::web) semantics: &'static str,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct ExternalRecordLifecycleContract {
    pub(in crate::web) authoritative_record_owner: &'static str,
    pub(in crate::web) authoritative_record_authority: &'static str,
    pub(in crate::web) canonical_record_location: &'static str,
    pub(in crate::web) instance_scope: &'static str,
    pub(in crate::web) write_mode: &'static str,
    pub(in crate::web) record_granularity: &'static str,
    pub(in crate::web) minimum_complete_record_fields: Vec<&'static str>,
    pub(in crate::web) same_record_must_link_rollout_and_rollback: bool,
    pub(in crate::web) in_process_maintenance: &'static str,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct ExternalRecordArchiveHandoffContract {
    pub(in crate::web) authoritative_archive_owner: &'static str,
    pub(in crate::web) record_completion_signal: &'static str,
    pub(in crate::web) handoff_ready_states: Vec<&'static str>,
    pub(in crate::web) terminal_states_requiring_archive_receipt: Vec<&'static str>,
    pub(in crate::web) required_archive_receipt_fields: Vec<&'static str>,
    pub(in crate::web) required_archive_correlation_dimensions: Vec<&'static str>,
    pub(in crate::web) source_record_state_must_match_section_result_status: bool,
    pub(in crate::web) terminal_section_not_complete_without_archive_receipt: bool,
    pub(in crate::web) external_record_not_sufficient_without_archive: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct ExternalRecordRetentionContract {
    pub(in crate::web) authoritative_retention_owner: &'static str,
    pub(in crate::web) authoritative_storage_scope: &'static str,
    pub(in crate::web) retention_policy: &'static str,
    pub(in crate::web) minimum_retention_window: &'static str,
    pub(in crate::web) immutability_expectation: &'static str,
    pub(in crate::web) replication_expectation: &'static str,
    pub(in crate::web) post_archive_verification_required: bool,
    pub(in crate::web) required_post_archive_checks: Vec<&'static str>,
    pub(in crate::web) required_post_archive_writeback_fields: Vec<&'static str>,
    pub(in crate::web) post_archive_writeback_target: &'static str,
    pub(in crate::web) local_process_role: &'static str,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct ExternalRecordTemplateField {
    pub(in crate::web) name: &'static str,
    pub(in crate::web) placeholder: &'static str,
    pub(in crate::web) completion_rule: &'static str,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct ExternalRecordExampleField {
    pub(in crate::web) name: &'static str,
    pub(in crate::web) value: &'static str,
    pub(in crate::web) rationale: &'static str,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct ExternalRecordSectionTemplateContract {
    pub(in crate::web) record_kind: &'static str,
    pub(in crate::web) section_signal: &'static str,
    pub(in crate::web) lifecycle_state_field: &'static str,
    pub(in crate::web) initial_state: &'static str,
    pub(in crate::web) required_fields: Vec<ExternalRecordTemplateField>,
    pub(in crate::web) exception_fields: Vec<ExternalRecordTemplateField>,
    pub(in crate::web) archive_receipt_fields_when_terminal: Vec<ExternalRecordTemplateField>,
    pub(in crate::web) post_archive_follow_up_fields: Vec<ExternalRecordTemplateField>,
    pub(in crate::web) notes: Vec<&'static str>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct ExternalRecordSectionExampleContract {
    pub(in crate::web) record_kind: &'static str,
    pub(in crate::web) section_signal: &'static str,
    pub(in crate::web) illustrative_only: bool,
    pub(in crate::web) section_state: &'static str,
    pub(in crate::web) example_fields: Vec<ExternalRecordExampleField>,
    pub(in crate::web) notes: Vec<&'static str>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct ExternalRecordWorkflowStep {
    pub(in crate::web) sequence: u8,
    pub(in crate::web) action: &'static str,
    pub(in crate::web) owner: &'static str,
    pub(in crate::web) evidence_source: &'static str,
    pub(in crate::web) record_effect: &'static str,
    pub(in crate::web) completion_record_fields: Vec<&'static str>,
    pub(in crate::web) blocking_until_complete: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct ExternalRecordTerminalMutationContract {
    pub(in crate::web) immutable_after_states: Vec<&'static str>,
    pub(in crate::web) allowed_append_only_updates: Vec<&'static str>,
    pub(in crate::web) allowed_follow_up_actions: Vec<&'static str>,
    pub(in crate::web) forbidden_mutations: Vec<&'static str>,
    pub(in crate::web) superseding_change_rule: &'static str,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct ExternalRecordExecutionBoundaryContract {
    pub(in crate::web) authoritative_live_record_system: &'static str,
    pub(in crate::web) authoritative_live_record_scope: &'static str,
    pub(in crate::web) terminal_snapshot_materialization_owner: &'static str,
    pub(in crate::web) terminal_snapshot_source: &'static str,
    pub(in crate::web) minimum_terminal_snapshot_record_fields: Vec<&'static str>,
    pub(in crate::web) conditional_terminal_snapshot_record_fields: Vec<&'static str>,
    pub(in crate::web) authoritative_archive_system: &'static str,
    pub(in crate::web) archive_receipt_writeback_target: &'static str,
    pub(in crate::web) required_system_separation: Vec<&'static str>,
    pub(in crate::web) forbidden_shortcuts: Vec<&'static str>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct ExternalSectionInstanceValidationStage {
    pub(in crate::web) stage: &'static str,
    pub(in crate::web) accepted_result_statuses: Vec<&'static str>,
    pub(in crate::web) required_workflow_steps: Vec<u8>,
    pub(in crate::web) required_fields: Vec<&'static str>,
    pub(in crate::web) conditional_required_fields: Vec<ExternalSectionConditionalFieldRequirement>,
    pub(in crate::web) required_additional_checks: Vec<&'static str>,
    pub(in crate::web) completion_rule: &'static str,
    pub(in crate::web) blocking_until_satisfied: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct ExternalSectionConditionalFieldRequirement {
    pub(in crate::web) when_result_statuses: Vec<&'static str>,
    pub(in crate::web) required_fields: Vec<&'static str>,
    pub(in crate::web) completion_rule: &'static str,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct ExternalSectionSnapshotFieldContract {
    pub(in crate::web) name: &'static str,
    pub(in crate::web) value_kind: &'static str,
    pub(in crate::web) semantics: &'static str,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct ExternalSectionSnapshotInputContract {
    pub(in crate::web) snapshot_kind: &'static str,
    pub(in crate::web) object_scope: &'static str,
    pub(in crate::web) required_top_level_fields: Vec<ExternalSectionSnapshotFieldContract>,
    pub(in crate::web) optional_top_level_fields: Vec<ExternalSectionSnapshotFieldContract>,
    pub(in crate::web) field_values_key: &'static str,
    pub(in crate::web) always_present_field_values: Vec<ExternalSectionSnapshotFieldContract>,
    pub(in crate::web) stage_scoped_field_values: Vec<ExternalSectionSnapshotFieldContract>,
    pub(in crate::web) prior_result_statuses_key: &'static str,
    pub(in crate::web) prior_result_statuses_required_for_states: Vec<&'static str>,
    pub(in crate::web) notes: Vec<&'static str>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct ExternalSectionSnapshotTemplateContract {
    pub(in crate::web) snapshot_kind: &'static str,
    pub(in crate::web) top_level_fields: Vec<ExternalRecordTemplateField>,
    pub(in crate::web) field_value_entries: Vec<ExternalRecordTemplateField>,
    pub(in crate::web) notes: Vec<&'static str>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct ExternalSectionSnapshotExampleContract {
    pub(in crate::web) snapshot_kind: &'static str,
    pub(in crate::web) illustrative_only: bool,
    pub(in crate::web) top_level_fields: Vec<ExternalRecordExampleField>,
    pub(in crate::web) field_value_entries: Vec<ExternalRecordExampleField>,
    pub(in crate::web) notes: Vec<&'static str>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct ExternalSectionValidationResultFieldContract {
    pub(in crate::web) name: &'static str,
    pub(in crate::web) value_kind: &'static str,
    pub(in crate::web) semantics: &'static str,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct ExternalSectionValidationResultContract {
    pub(in crate::web) result_kind: &'static str,
    pub(in crate::web) object_scope: &'static str,
    pub(in crate::web) required_fields: Vec<ExternalSectionValidationResultFieldContract>,
    pub(in crate::web) optional_fields: Vec<ExternalSectionValidationResultFieldContract>,
    pub(in crate::web) stage_status_field: &'static str,
    pub(in crate::web) notes: Vec<&'static str>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct ExternalSectionValidationResultExampleContract {
    pub(in crate::web) result_kind: &'static str,
    pub(in crate::web) illustrative_only: bool,
    pub(in crate::web) fields: Vec<ExternalRecordExampleField>,
    pub(in crate::web) notes: Vec<&'static str>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct ExternalSectionInstanceValidationContract {
    pub(in crate::web) record_kind: &'static str,
    pub(in crate::web) section_signal: &'static str,
    pub(in crate::web) lifecycle_state_field: &'static str,
    pub(in crate::web) validation_scope: &'static str,
    pub(in crate::web) snapshot_input_contract: ExternalSectionSnapshotInputContract,
    pub(in crate::web) snapshot_template_contract: ExternalSectionSnapshotTemplateContract,
    pub(in crate::web) minimum_snapshot_example: ExternalSectionSnapshotExampleContract,
    pub(in crate::web) validation_result_contract: ExternalSectionValidationResultContract,
    pub(in crate::web) minimum_validation_result_example:
        ExternalSectionValidationResultExampleContract,
    pub(in crate::web) draft_requirements: ExternalSectionInstanceValidationStage,
    pub(in crate::web) evidence_linked_requirements: ExternalSectionInstanceValidationStage,
    pub(in crate::web) terminal_requirements: ExternalSectionInstanceValidationStage,
    pub(in crate::web) archive_receipt_requirements: ExternalSectionInstanceValidationStage,
    pub(in crate::web) post_archive_verification_requirements:
        ExternalSectionInstanceValidationStage,
    pub(in crate::web) required_authority_pairing_check_ids: Vec<&'static str>,
    pub(in crate::web) required_archive_correlation_dimensions: Vec<&'static str>,
    pub(in crate::web) source_record_state_field: &'static str,
    pub(in crate::web) blocking_interpretation: &'static str,
    pub(in crate::web) forbidden_shortcuts: Vec<&'static str>,
    pub(in crate::web) forbidden_post_terminal_mutations: Vec<&'static str>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct PreflightReviewDecisionContract {
    pub(in crate::web) signal: &'static str,
    pub(in crate::web) review_owner: &'static str,
    pub(in crate::web) decision_scope: &'static str,
    pub(in crate::web) required_decision_fields: Vec<&'static str>,
    pub(in crate::web) required_decision_field_contracts: Vec<ExternalRecordFieldContract>,
    pub(in crate::web) exception_record_fields: Vec<&'static str>,
    pub(in crate::web) exception_record_field_contracts: Vec<ExternalRecordFieldContract>,
    pub(in crate::web) record_lifecycle_contract: ExternalRecordLifecycleContract,
    pub(in crate::web) archive_handoff_contract: ExternalRecordArchiveHandoffContract,
    pub(in crate::web) retention_contract: ExternalRecordRetentionContract,
    pub(in crate::web) terminal_mutation_contract: ExternalRecordTerminalMutationContract,
    pub(in crate::web) public_entry_transition_contract: Option<PublicEntryTransitionContract>,
    pub(in crate::web) public_entry_lifecycle_transition_contract:
        Option<PublicEntryLifecycleTransitionContract>,
    pub(in crate::web) section_instance_validation_contract:
        Option<ExternalSectionInstanceValidationContract>,
    pub(in crate::web) validator_integration_readiness_summary:
        Option<SectionValidatorIntegrationReadinessSummary>,
    pub(in crate::web) authority_pairing_checks: Vec<ExternalRecordAuthorityPairingCheck>,
    pub(in crate::web) execution_boundary_contract: ExternalRecordExecutionBoundaryContract,
    pub(in crate::web) result_status_model: Vec<ResultStatusContract>,
    pub(in crate::web) section_record_template: ExternalRecordSectionTemplateContract,
    pub(in crate::web) minimum_section_example: ExternalRecordSectionExampleContract,
    pub(in crate::web) section_execution_workflow: Vec<ExternalRecordWorkflowStep>,
    pub(in crate::web) accepted_exception_follow_up: Vec<&'static str>,
    pub(in crate::web) external_record_owner: &'static str,
    pub(in crate::web) external_record_authority: &'static str,
    pub(in crate::web) decision_reference_kind: &'static str,
    pub(in crate::web) exception_reference_kind: &'static str,
    pub(in crate::web) local_contract_role: &'static str,
    pub(in crate::web) supporting_endpoints: Vec<PreflightSupportingEndpoint>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct PreflightReport {
    pub(in crate::web) status: &'static str,
    pub(in crate::web) environment: &'static str,
    pub(in crate::web) release_blocked: bool,
    pub(in crate::web) requires_operator_review: bool,
    pub(in crate::web) development_stage_closure_status: &'static str,
    pub(in crate::web) real_cutover_execution_status: &'static str,
    pub(in crate::web) remaining_external_execution_dependencies:
        Vec<ExternalExecutionDependencyReport>,
    pub(in crate::web) repo_bundled_official_entry_snapshot: RepoBundledOfficialEntrySnapshotReport,
    pub(in crate::web) components: Vec<PreflightComponentReport>,
    pub(in crate::web) blocking_signals: Vec<&'static str>,
    pub(in crate::web) review_signals: Vec<&'static str>,
    pub(in crate::web) follow_up_endpoints: Vec<PreflightFollowUpEndpoint>,
    pub(in crate::web) review_decision_contracts: Vec<PreflightReviewDecisionContract>,
    pub(in crate::web) operator_review_items: Vec<PreflightReviewItem>,
    pub(in crate::web) required_signoff_fields: Vec<&'static str>,
    pub(in crate::web) post_review_actions: Vec<&'static str>,
    pub(in crate::web) release_gate: &'static str,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct SectionValidatorIntegrationReadinessSummary {
    pub(in crate::web) status: &'static str,
    pub(in crate::web) input_snapshot_kind: &'static str,
    pub(in crate::web) field_values_key: &'static str,
    pub(in crate::web) lifecycle_state_field: &'static str,
    pub(in crate::web) output_result_kind: &'static str,
    pub(in crate::web) output_stage_status_field: &'static str,
    pub(in crate::web) blocking_interpretation: &'static str,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct BackupCheck {
    pub(in crate::web) kind: &'static str,
    pub(in crate::web) path: String,
    pub(in crate::web) ok: bool,
    pub(in crate::web) required: bool,
    pub(in crate::web) backup_expectation: &'static str,
    pub(in crate::web) detail: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct BackupReport {
    pub(in crate::web) status: &'static str,
    pub(in crate::web) environment: &'static str,
    pub(in crate::web) checks: Vec<BackupCheck>,
    pub(in crate::web) evidence_sink_checks: Vec<HealthCheck>,
    pub(in crate::web) quiesce_requirement: &'static str,
    pub(in crate::web) restore_verification: Vec<&'static str>,
    pub(in crate::web) responsibility_boundary: ResponsibilityBoundary,
    pub(in crate::web) evidence_sink: EvidenceSinkContract,
    pub(in crate::web) evidence_write_contract: EvidenceWriteContract,
    pub(in crate::web) archive_handoff_contract: EvidenceArchiveHandoffContract,
    pub(in crate::web) evidence_requirements: Vec<EvidenceFieldContract>,
    pub(in crate::web) result_status_model: Vec<ResultStatusContract>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct MetricsContract {
    pub(in crate::web) surface: &'static str,
    pub(in crate::web) environment: &'static str,
    pub(in crate::web) consumption: &'static str,
    pub(in crate::web) content_type: &'static str,
    pub(in crate::web) cache_policy: &'static str,
    pub(in crate::web) scrape_mode: &'static str,
    pub(in crate::web) readiness_signal: bool,
    pub(in crate::web) interpretation_boundary: &'static str,
    pub(in crate::web) signal_families: Vec<MetricsSignalFamilyContract>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct MetricsSignalFamilyContract {
    pub(in crate::web) family: &'static str,
    pub(in crate::web) purpose: &'static str,
    pub(in crate::web) example_metrics: Vec<&'static str>,
    pub(in crate::web) rollout_use: &'static str,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct RecoveryEntryContract {
    pub(in crate::web) kind: &'static str,
    pub(in crate::web) path: String,
    pub(in crate::web) data_domain: &'static str,
    pub(in crate::web) write_owner: &'static str,
    pub(in crate::web) consistency_requirement: &'static str,
    pub(in crate::web) migration_strategy: &'static str,
    pub(in crate::web) recovery_class: &'static str,
    pub(in crate::web) backup_expectation: &'static str,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct RecoveryContract {
    pub(in crate::web) surface: &'static str,
    pub(in crate::web) environment: &'static str,
    pub(in crate::web) cache_policy: &'static str,
    pub(in crate::web) state_inventory: Vec<RecoveryEntryContract>,
    pub(in crate::web) recovery_staging: RecoveryStagingContract,
    pub(in crate::web) audit_retention: AuditRetentionContract,
    pub(in crate::web) minimum_runbook: Vec<&'static str>,
    pub(in crate::web) minimum_recovery_drill: Vec<&'static str>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct RecoveryStagingContract {
    pub(in crate::web) root_dir: String,
    pub(in crate::web) isolated_from_live_state: bool,
    pub(in crate::web) expected_identity_file: String,
    pub(in crate::web) expected_database_file: String,
    pub(in crate::web) expected_config_dir: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct AuditRetentionContract {
    pub(in crate::web) active_log_path: String,
    pub(in crate::web) archive_pattern: String,
    pub(in crate::web) format: &'static str,
    pub(in crate::web) rotation_trigger: &'static str,
    pub(in crate::web) max_active_file_mebibytes: u64,
    pub(in crate::web) max_archive_files: usize,
    pub(in crate::web) maintenance_point: &'static str,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct RecoveryDrillCheck {
    pub(in crate::web) name: &'static str,
    pub(in crate::web) ok: bool,
    pub(in crate::web) required: bool,
    pub(in crate::web) detail: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct RecoveryDrillReport {
    pub(in crate::web) status: &'static str,
    pub(in crate::web) environment: &'static str,
    pub(in crate::web) live_data_dir: String,
    pub(in crate::web) staging_data_dir: String,
    pub(in crate::web) checks: Vec<RecoveryDrillCheck>,
    pub(in crate::web) evidence_sink_checks: Vec<HealthCheck>,
    pub(in crate::web) cutover_preconditions: Vec<&'static str>,
    pub(in crate::web) responsibility_boundary: ResponsibilityBoundary,
    pub(in crate::web) evidence_sink: EvidenceSinkContract,
    pub(in crate::web) evidence_write_contract: EvidenceWriteContract,
    pub(in crate::web) archive_handoff_contract: EvidenceArchiveHandoffContract,
    pub(in crate::web) execution_contract: RecoveryDrillExecutionContract,
    pub(in crate::web) evidence_requirements: Vec<EvidenceFieldContract>,
    pub(in crate::web) result_status_model: Vec<ResultStatusContract>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct OperationalBaselineObjective {
    pub(in crate::web) category: &'static str,
    pub(in crate::web) name: &'static str,
    pub(in crate::web) target: &'static str,
    pub(in crate::web) measurement: &'static str,
    pub(in crate::web) owner: &'static str,
    pub(in crate::web) supporting_endpoints: Vec<&'static str>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct OperationalBaselineReport {
    pub(in crate::web) status: &'static str,
    pub(in crate::web) environment: &'static str,
    pub(in crate::web) target_tick_rate: u64,
    pub(in crate::web) target_tick_interval_millis: u64,
    pub(in crate::web) failure_drill_cadence: &'static str,
    pub(in crate::web) runbook_baseline: Vec<&'static str>,
    pub(in crate::web) observability_channels: Vec<ObservabilityChannelContract>,
    pub(in crate::web) objectives: Vec<OperationalBaselineObjective>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct ObservabilityChannelContract {
    pub(in crate::web) channel: &'static str,
    pub(in crate::web) purpose: &'static str,
    pub(in crate::web) sink: String,
    pub(in crate::web) retention_policy: &'static str,
    pub(in crate::web) retention_owner: &'static str,
    pub(in crate::web) authoritative_for_ops_audit: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct ResponsibilityBoundary {
    pub(in crate::web) this_process: Vec<&'static str>,
    pub(in crate::web) external_orchestrator: Vec<&'static str>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct EvidenceFieldContract {
    pub(in crate::web) name: &'static str,
    pub(in crate::web) owner: &'static str,
    pub(in crate::web) semantics: &'static str,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct EvidenceSinkContract {
    pub(in crate::web) path: String,
    pub(in crate::web) format: &'static str,
    pub(in crate::web) owner: &'static str,
    pub(in crate::web) semantics: &'static str,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct EvidenceWriteContract {
    pub(in crate::web) format: &'static str,
    pub(in crate::web) write_mode: &'static str,
    pub(in crate::web) record_granularity: &'static str,
    pub(in crate::web) minimum_common_fields: Vec<&'static str>,
    pub(in crate::web) in_process_maintenance: &'static str,
    pub(in crate::web) local_sink_retention_role: &'static str,
    pub(in crate::web) external_archive_required: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct EvidenceArchiveHandoffContract {
    pub(in crate::web) authoritative_archive_owner: &'static str,
    pub(in crate::web) local_record_completion_signal: &'static str,
    pub(in crate::web) handoff_ready_states: Vec<&'static str>,
    pub(in crate::web) terminal_states_requiring_archive_receipt: Vec<&'static str>,
    pub(in crate::web) required_archive_receipt_fields: Vec<&'static str>,
    pub(in crate::web) local_record_not_sufficient_without_archive: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct RecoveryDrillExecutionContract {
    pub(in crate::web) execution_owner: &'static str,
    pub(in crate::web) cadence_expectation: &'static str,
    pub(in crate::web) release_gate: &'static str,
    pub(in crate::web) required_signoff_fields: Vec<&'static str>,
    pub(in crate::web) required_post_drill_actions: Vec<&'static str>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::web) struct ResultStatusContract {
    pub(in crate::web) state: &'static str,
    pub(in crate::web) semantics: &'static str,
}
