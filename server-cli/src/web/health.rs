use common::official_entry::OfficialEntry;
use hyper::StatusCode;
use prometheus::Registry;
use std::path::Path;

#[derive(Clone)]
pub struct HealthState {
    pub environment: &'static str,
    pub auth_server_configured: bool,
    pub authoritative_auth_provider: Option<String>,
    pub server_state: server::ServerStatePaths,
    pub recovery_staging_state: server::ServerStatePaths,
    pub audit_retention: crate::settings::AuditRetentionPolicy,
    pub runtime_listener_inventory: server::RuntimeListenerInventory,
    pub runtime_observability_inventory: RuntimeObservabilityInventory,
    pub surface_inventory: Vec<crate::settings::RuntimeSurface>,
    pub management_auth_inventory: Vec<crate::settings::ManagementAuthInventoryEntry>,
    pub transport_security_inventory: Vec<crate::settings::TransportSecurityInventoryEntry>,
    pub governance_findings: Vec<crate::settings::RuntimeGovernanceFinding>,
}

#[derive(Clone)]
pub(super) struct MetricsState {
    pub(super) registry: Registry,
    pub(super) contract: MetricsContract,
    pub(super) runtime_observability_inventory: RuntimeObservabilityInventory,
}

mod observability;
#[cfg(feature = "worldgen")]
pub(crate) use observability::set_world_compat_observability_status;
pub(in crate::web) use observability::{
    RuntimeObservabilityContext, RuntimeObservabilitySurface, set_runtime_observability_status,
};
pub use observability::{
    RuntimeObservabilityInventory, RuntimeObservabilityState, RuntimeObservabilityStatus,
    default_runtime_observability_inventory, snapshot_runtime_observability_inventory,
};
fn repo_bundled_official_entry_snapshot_review_items(
    snapshot: &RepoBundledOfficialEntrySnapshotReport,
) -> Vec<PreflightReviewItem> {
    match &snapshot.baseline {
        Some(baseline) if !baseline.non_local_cutover_ready => vec![PreflightReviewItem {
            kind: "repo-bundled-entry-transitional-baseline",
            blocking: false,
            detail: format!(
                "repo/local bundled official_entry baseline remains transitional (target_kind={}, \
                 gap_reasons=[{}]); use /health/compatibility only as a local comparison baseline \
                 and require the external release review to prove the shipped Public client \
                 artifact carries the intended non-local bundle instead of this repo snapshot",
                baseline.target_kind.as_str(),
                if baseline.non_local_cutover_gap_reasons.is_empty() {
                    "none".to_owned()
                } else {
                    baseline.non_local_cutover_gap_reasons.join(", ")
                }
            ),
        }],
        Some(_) => Vec::new(),
        None => vec![PreflightReviewItem {
            kind: "repo-bundled-entry-snapshot-unavailable",
            blocking: false,
            detail: format!(
                "repo/local bundled official_entry baseline could not be loaded in-process ({}); \
                 external release review must carry the full shipped Public client artifact \
                 comparison because no local bundled baseline snapshot is available here",
                snapshot
                    .load_error
                    .as_deref()
                    .unwrap_or("unknown load error")
            ),
        }],
    }
}

mod reports;
use reports::*;

mod contracts;
use contracts::*;
impl HealthState {
    fn runtime_environment(&self) -> server::settings::RuntimeEnvironment {
        match self.environment {
            "local" => server::settings::RuntimeEnvironment::Local,
            "test" => server::settings::RuntimeEnvironment::Test,
            _ => server::settings::RuntimeEnvironment::Production,
        }
    }

    fn build_authoritative_runtime_settings(&self) -> server::Settings {
        let mut settings = server::Settings::default();
        settings.runtime_environment = self.runtime_environment();
        settings.auth_server_address = self.authoritative_auth_provider.clone();
        settings
    }

    pub(super) fn account_auth_governance_report(
        &self,
    ) -> server::settings::AccountAuthGovernanceReport {
        self.build_authoritative_runtime_settings()
            .account_auth_governance_report()
    }

    fn repo_bundled_official_entry_snapshot(&self) -> RepoBundledOfficialEntrySnapshotReport {
        match OfficialEntry::try_load() {
            Ok(entry) => {
                let baseline = entry.posture();
                let status = if baseline.non_local_cutover_ready {
                    "repo-bundled-entry-non-local-ready"
                } else {
                    "repo-bundled-entry-transitional"
                };

                RepoBundledOfficialEntrySnapshotReport {
                    status,
                    evidence_scope: "repo/local bundled official_entry baseline only",
                    load_source: "voxygen.official_entry asset via common asset loader",
                    authoritative_for_release_cutover: false,
                    required_external_match_fields: vec![
                        "bundled_official_entry_artifact_identity",
                        "bundled_official_entry_server_address",
                        "bundled_official_entry_auth_server",
                        "bundled_official_entry_use_srv",
                        "bundled_official_entry_use_quic",
                        "bundled_official_entry_validate_tls",
                        "bundled_target_kind",
                        "bundled_target_is_non_local_candidate",
                        "non_local_cutover_gap_reasons",
                    ],
                    baseline: Some(baseline),
                    load_error: None,
                    semantics: "publishes the repo/local bundled official_entry posture that this \
                                workspace currently resolves so operators can compare it against \
                                the shipped Public client artifact review record without creating \
                                a second release authority source",
                }
            },
            Err(error) => RepoBundledOfficialEntrySnapshotReport {
                status: "repo-bundled-entry-unavailable",
                evidence_scope: "repo/local bundled official_entry baseline only",
                load_source: "voxygen.official_entry asset via common asset loader",
                authoritative_for_release_cutover: false,
                required_external_match_fields: vec![
                    "bundled_official_entry_artifact_identity",
                    "bundled_official_entry_server_address",
                    "bundled_official_entry_auth_server",
                    "bundled_official_entry_use_srv",
                    "bundled_official_entry_use_quic",
                    "bundled_official_entry_validate_tls",
                    "bundled_target_kind",
                    "bundled_target_is_non_local_candidate",
                    "non_local_cutover_gap_reasons",
                ],
                baseline: None,
                load_error: Some(error.to_string()),
                semantics: "this process could not load the repo/local bundled official_entry \
                            asset, so machine verification is unavailable here and external \
                            shipped-client review remains the only source for cutover evidence",
            },
        }
    }

    fn build_query_contract_hint(&self) -> veloren_query_server::proto::ServerInfo {
        let settings = self.build_authoritative_runtime_settings();
        server::build_query_server_info(&settings, &server::ServerIdentity::default(), 0)
    }

    pub(super) fn compatibility_contract_report(&self) -> CompatibilityContractReport {
        self.compatibility_contract_report_with_query_hint(self.build_query_contract_hint())
    }

    fn compatibility_contract_report_with_query_hint(
        &self,
        query_hint: veloren_query_server::proto::ServerInfo,
    ) -> CompatibilityContractReport {
        let authoritative_compatibility = common_net::msg::ServerCompatibility::current();
        let authoritative_auth_provider = self.authoritative_auth_provider.clone();
        let authoritative_auth_mode = runtime_auth_mode(authoritative_auth_provider.as_deref());
        let public_entry_handoff = self.public_entry_handoff_report(
            authoritative_auth_mode,
            authoritative_auth_provider.clone(),
            query_hint.auth_required,
        );
        let environment_matches =
            query_server_environment_str(query_hint.environment) == self.environment;
        let compatibility_matches = query_hint.compatibility.generation
            == authoritative_compatibility.generation
            && query_hint.compatibility.minimum_supported_generation
                == authoritative_compatibility.minimum_supported_generation;
        let auth_requirement_matches_runtime_config =
            query_hint.auth_required == authoritative_auth_provider.is_some();

        CompatibilityContractReport {
            status: if environment_matches
                && compatibility_matches
                && auth_requirement_matches_runtime_config
            {
                "compatibility-contract-aligned"
            } else {
                "compatibility-contract-drift"
            },
            environment: self.environment,
            authoritative_handshake: AuthoritativeHandshakeReport {
                surface: "realm-handshake",
                authority_scope: "authoritative",
                environment_truth: self.environment,
                compatibility_generation: authoritative_compatibility.generation,
                minimum_supported_generation: authoritative_compatibility
                    .minimum_supported_generation,
                build_identity_fields: vec!["git_hash", "git_timestamp"],
                auth_signal: "auth_provider -> ServerAuth",
                auth_mode: authoritative_auth_mode.as_str(),
                auth_provider: authoritative_auth_provider,
            },
            query_hint: QueryCompatibilityHintReport {
                surface: "query-server",
                authority_scope: "discovery-hint-only",
                environment_hint: query_server_environment_str(query_hint.environment),
                compatibility_generation: query_hint.compatibility.generation,
                minimum_supported_generation: query_hint.compatibility.minimum_supported_generation,
                auth_required: query_hint.auth_required,
                auth_hint_scope: "auth-requirement-only-hint",
                protocol_version: veloren_query_server::proto::CURRENT_PROTOCOL_VERSION,
                version_selection_policy: veloren_query_server::proto::VERSION_SELECTION_POLICY,
                supports_multi_version_negotiation:
                    veloren_query_server::proto::SUPPORTS_MULTI_VERSION_NEGOTIATION,
                published_protocol_fields:
                    veloren_query_server::proto::PUBLISHED_SERVER_INFO_FIELDS.to_vec(),
            },
            query_protocol_rollout: QueryProtocolRolloutContract {
                protocol_version: veloren_query_server::proto::CURRENT_PROTOCOL_VERSION,
                version_selection_policy: veloren_query_server::proto::VERSION_SELECTION_POLICY,
                supports_multi_version_negotiation:
                    veloren_query_server::proto::SUPPORTS_MULTI_VERSION_NEGOTIATION,
                requires_lockstep_rollout: true,
                current_stage_policy: "phase-3 formal policy is exact-match query protocol \
                                       rollout with lockstep upgrade and rollback",
                policy_change_requirement: "do not permit mixed-version query rollout until \
                                            explicit multi-version negotiation and a replacement \
                                            operator-facing contract land together",
                authoritative_client_path: "official_entry -> EntryPolicy -> realm handshake",
                known_in_repo_consumers: vec![
                    "common/query_server examples",
                    "no shipping client path in this repo consumes query as an authoritative \
                     connection path",
                ],
                mixed_version_policy: "mixed-version query server and query consumer pairs are \
                                       unsupported because the current query protocol is \
                                       exact-match and non-negotiated",
                safe_transition_options: vec![
                    "upgrade operator-managed query consumers and the server query surface in one \
                     release unit",
                    "keep remote query exposure disabled during any mixed-version rollout or \
                     rollback window",
                    "leave Public discovery and connection authority on the realm handshake path \
                     even when query is unavailable",
                ],
                upgrade_order: vec![
                    "prepare and verify matching operator-managed query consumers for the target \
                     query protocol version",
                    "keep the query surface disabled or non-remotely-exposed while consumer and \
                     server versions do not yet match",
                    "deploy the matching server query protocol version",
                    "re-expose the query surface only after server and operator-managed consumers \
                     are on the same protocol version",
                ],
                rollback_order: vec![
                    "withdraw remote query exposure before crossing a query protocol version \
                     boundary",
                    "roll back the server query surface and any operator-managed query consumers \
                     to a matching protocol pair",
                    "re-expose the query surface only after version parity is restored",
                ],
            },
            public_entry_handoff,
            environment_matches,
            compatibility_matches,
            auth_requirement_matches_runtime_config,
            shared_truth_builder: "server::build_query_server_info(...) mirrors the same runtime \
                                   environment and compatibility truth used by the authoritative \
                                   handshake; structured auth mode is derived from the same \
                                   authoritative auth_provider contract",
            operator_consumption: "use the realm handshake as the authoritative rollout, \
                                   compatibility, and auth-mode gate; use query-server only for \
                                   pre-connect hinting and server-list display, and treat \
                                   auth_required as a coarse hint rather than authority",
            mismatch_effect: "treat any handshake/query drift as rollout-blocking until the \
                              published discovery hint matches the authoritative handshake \
                              contract again",
        }
    }

    fn public_entry_handoff_report(
        &self,
        authoritative_auth_mode: common_net::msg::ServerAuthMode,
        authoritative_auth_provider: Option<String>,
        query_auth_requirement_hint: bool,
    ) -> PublicEntryHandoffReport {
        let repo_bundled_official_entry_snapshot = self.repo_bundled_official_entry_snapshot();
        let applies_to_non_local_public_rollout = self.environment != "local";
        let authoritative_auth_provider_ref = authoritative_auth_provider.as_deref();
        let required_external_review_field_contracts = if applies_to_non_local_public_rollout {
            public_entry_handoff_required_review_field_contracts()
        } else {
            Vec::new()
        };
        let required_authority_pairing_checks = if applies_to_non_local_public_rollout {
            public_entry_authority_pairing_checks()
        } else {
            Vec::new()
        };
        let release_blocked = applies_to_non_local_public_rollout
            && !matches!(
                authoritative_auth_mode,
                common_net::msg::ServerAuthMode::ExternalProvider
            );
        let requires_operator_review = applies_to_non_local_public_rollout && !release_blocked;
        let required_cutover_material_checklist = if applies_to_non_local_public_rollout {
            public_entry_cutover_material_checklist(
                self.environment,
                authoritative_auth_mode,
                authoritative_auth_provider_ref,
                &repo_bundled_official_entry_snapshot,
            )
        } else {
            Vec::new()
        };
        let public_entry_transition_contract = if applies_to_non_local_public_rollout {
            Some(public_entry_transition_contract())
        } else {
            None
        };
        let public_entry_lifecycle_transition_contract = if applies_to_non_local_public_rollout {
            Some(public_entry_lifecycle_transition_contract())
        } else {
            None
        };
        let section_instance_validation_contract = if applies_to_non_local_public_rollout {
            Some(public_entry_section_instance_validation_contract(
                &required_external_review_field_contracts,
            ))
        } else {
            None
        };
        let development_stage_closure_status = if applies_to_non_local_public_rollout {
            "development-contract-closure-available"
        } else {
            "not-applicable-local"
        };
        let real_cutover_execution_status = if !applies_to_non_local_public_rollout {
            "not-applicable-local"
        } else if release_blocked {
            "blocked-by-runtime-auth-posture-and-external-materials"
        } else {
            "awaiting-external-materials-and-execution"
        };
        let remaining_external_execution_dependencies =
            public_entry_external_execution_dependencies(
                applies_to_non_local_public_rollout,
                release_blocked,
                authoritative_auth_mode,
                authoritative_auth_provider_ref,
            );

        PublicEntryHandoffReport {
            signal: "public-entry-handoff",
            status: if !applies_to_non_local_public_rollout {
                "not-applicable-local"
            } else if release_blocked {
                "non-local-public-rollout-unsupported"
            } else {
                "external-review-required"
            },
            applies_to_non_local_public_rollout,
            requires_operator_review,
            release_blocked,
            development_stage_closure_available_without_real_materials: true,
            development_stage_closure_status,
            development_stage_closure_scope: "development-stage closure here means the typed \
                                              Public handoff contract, operator checklist, record \
                                              schema, archive/writeback boundary, and repo/local \
                                              comparison baseline are implemented and reviewable \
                                              without claiming a real Public target is already \
                                              ready for cutover",
            real_cutover_still_requires_external_materials: applies_to_non_local_public_rollout,
            real_cutover_execution_status,
            real_cutover_dependency_boundary: if !applies_to_non_local_public_rollout {
                "real non-local Public cutover is outside local-mode scope"
            } else if release_blocked {
                "first real non-local Public cutover still requires a supported external-auth \
                 handshake posture plus external shipped/rollback client artifacts, bundled \
                 official_entry material, rollback path, and external release review/archive \
                 execution"
            } else {
                "first real non-local Public cutover still requires external shipped/rollback \
                 client artifacts, real realm/auth authority material, rollback path, and external \
                 release review/archive execution"
            },
            remaining_external_execution_dependencies,
            authority_scope: "external-release-review-required",
            authoritative_public_target_path: "bundled official_entry.server_address -> \
                                               EntryPolicy -> realm handshake",
            authoritative_public_auth_path: "bundled official_entry.auth_server exact-match pin \
                                             -> EntryPolicy auth trust -> realm handshake \
                                             auth_provider -> ServerAuth",
            expected_handshake_auth_mode: authoritative_auth_mode.as_str(),
            authoritative_handshake_auth_provider: authoritative_auth_provider,
            query_auth_requirement_hint,
            machine_verification_available_in_this_process: repo_bundled_official_entry_snapshot
                .baseline
                .is_some(),
            machine_verification_scope: "repo/local bundled official_entry baseline only",
            machine_verification_limitations: "cannot prove that the shipped Public client \
                                               artifact matches this repo/local baseline; \
                                               external release review remains authoritative for \
                                               cutover approval",
            repo_bundled_official_entry_snapshot,
            required_external_review_fields: external_record_field_names(
                &required_external_review_field_contracts,
            ),
            required_external_review_field_contracts,
            required_cutover_preconditions: if applies_to_non_local_public_rollout {
                vec![
                    "record the shipped Public client artifact reference plus the bundled \
                     official_entry artifact identity, server address, auth pin, transport flags, \
                     and client-exported target posture/gap reasons for the bundle that will be \
                     reopened to Public traffic",
                    "record the target runtime environment, authoritative compatibility \
                     generation, authoritative handshake auth provider, and auth mode from \
                     /health/compatibility before approving cutover",
                    "require /health/ready to report ready and record the observed \
                     ready_report_status for the rollout decision",
                    "link current backup evidence and a recovery drill reference that reached \
                     ready-validated before approving non-local Public cutover",
                    "record rollback_reference, rollback_public_client_artifact_reference, plus \
                     rollback_bundled_official_entry_artifact_identity before reopening Public \
                     traffic so the same release review record points to both the rollback path \
                     and the Public client artifact plus entry material that will be restored if \
                     cutover is reverted",
                ]
            } else {
                Vec::new()
            },
            required_cutover_material_checklist,
            public_entry_transition_contract,
            public_entry_lifecycle_transition_contract,
            section_instance_validation_contract,
            required_authority_pairing_checks,
            supporting_health_endpoints: if applies_to_non_local_public_rollout {
                vec![
                    "/health/compatibility",
                    "/health/ready",
                    "/health/backup",
                    "/health/recovery/drill",
                ]
            } else {
                Vec::new()
            },
            semantics: "this process can publish the expected non-local Public rollout contract, \
                        and it can expose the repo/local bundled official_entry baseline for \
                        comparison, but it still cannot prove bundled client assets were the ones \
                        shipped; external release review must compare the shipped Public client \
                        artifact reference plus official_entry.server_address/auth_server/use_srv \
                        /use_quic/validate_tls against the intended Public target, record the \
                        shipped bundled official_entry artifact identity plus the client-exported \
                        bundled target posture/gap reasons, target runtime environment, \
                        authoritative compatibility generation, authoritative handshake auth \
                        provider, ready report status, backup/recovery evidence references, \
                        rollback reference, rollback Public client artifact reference, and \
                        rollback bundled official_entry artifact identity, and confirm the \
                        authoritative handshake auth posture plus exact-match auth pin and \
                        bundle/runtime pairing checks before Public traffic is opened",
        }
    }

    fn runtime_listener_preflight_component(&self) -> (PreflightComponentReport, Vec<String>) {
        let runtime_listener_entries = match self.runtime_listener_inventory.lock() {
            Ok(entries) => entries.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        };
        let expected_listener_surfaces = self
            .surface_inventory
            .iter()
            .filter(|surface| {
                matches!(surface.name, "game-tcp" | "game-quic" | "query-server")
                    && !matches!(
                        surface.reachability,
                        crate::settings::SurfaceReachability::Disabled
                    )
            })
            .filter_map(|surface| {
                surface
                    .bind_address
                    .map(|bind_address| (surface.name, bind_address))
            })
            .collect::<Vec<_>>();

        let failures = expected_listener_surfaces
            .into_iter()
            .filter_map(|(surface_name, bind_address)| {
                match runtime_listener_entries.iter().find(|entry| {
                    entry.surface.as_str() == surface_name && entry.bind_address == bind_address
                }) {
                    Some(entry) if entry.state == server::RuntimeListenerState::Listening => None,
                    Some(entry) => Some(format!(
                        "{} {} is currently {} ({})",
                        surface_name,
                        bind_address,
                        entry.state.as_str(),
                        entry.detail
                    )),
                    None => Some(format!(
                        "{} {} has no runtime listener record after startup",
                        surface_name, bind_address
                    )),
                }
            })
            .collect::<Vec<_>>();

        (
            PreflightComponentReport {
                signal: "runtime-listeners",
                endpoint: "/health/listeners",
                status: if failures.is_empty() {
                    "runtime-listeners-ready"
                } else {
                    "runtime-listeners-blocked"
                },
                blocking: !failures.is_empty(),
                requires_operator_review: false,
            },
            failures,
        )
    }

    fn world_compat_preflight_component(&self) -> (PreflightComponentReport, Option<String>) {
        let report = self.world_compat_report();

        match report.status {
            "world-compat-clear" => (
                PreflightComponentReport {
                    signal: "world-compat",
                    endpoint: "/health/world-compat",
                    status: report.status,
                    blocking: false,
                    requires_operator_review: false,
                },
                None,
            ),
            "world-compat-not-applicable" => (
                PreflightComponentReport {
                    signal: "world-compat",
                    endpoint: "/health/world-compat",
                    status: report.status,
                    blocking: false,
                    requires_operator_review: false,
                },
                None,
            ),
            _ => (
                PreflightComponentReport {
                    signal: "world-compat",
                    endpoint: "/health/world-compat",
                    status: report.status,
                    blocking: false,
                    requires_operator_review: true,
                },
                Some(report.detail),
            ),
        }
    }

    pub(super) fn liveness_report(&self) -> LiveReport {
        LiveReport {
            status: "live",
            environment: self.environment,
        }
    }

    pub(super) fn health_contract(&self) -> HealthContract {
        HealthContract {
            surface: "health",
            environment: self.environment,
            consumption: "machine-probe",
            cache_policy: "no-store",
            endpoints: vec![
                HealthEndpointContract {
                    path: "/health",
                    signal: "liveness",
                    success_status: StatusCode::OK.as_u16(),
                    failure_status: None,
                    semantics: "basic process liveness probe",
                },
                HealthEndpointContract {
                    path: "/health/live",
                    signal: "liveness",
                    success_status: StatusCode::OK.as_u16(),
                    failure_status: None,
                    semantics: "explicit alias for basic process liveness probe",
                },
                HealthEndpointContract {
                    path: "/health/ready",
                    signal: "readiness",
                    success_status: StatusCode::OK.as_u16(),
                    failure_status: Some(StatusCode::SERVICE_UNAVAILABLE.as_u16()),
                    semantics: "startup and runtime readiness gate for required local state",
                },
                HealthEndpointContract {
                    path: "/health/recovery",
                    signal: "recovery-contract",
                    success_status: StatusCode::OK.as_u16(),
                    failure_status: None,
                    semantics: "machine-readable recovery inventory, staging layout, and runbook \
                                contract",
                },
                HealthEndpointContract {
                    path: "/health/backup",
                    signal: "backup-preflight",
                    success_status: StatusCode::OK.as_u16(),
                    failure_status: Some(StatusCode::SERVICE_UNAVAILABLE.as_u16()),
                    semantics: "preflight probe for required backup scope and restore \
                                preconditions",
                },
                HealthEndpointContract {
                    path: "/health/recovery/drill",
                    signal: "recovery-drill",
                    success_status: StatusCode::OK.as_u16(),
                    failure_status: Some(StatusCode::SERVICE_UNAVAILABLE.as_u16()),
                    semantics: "restore-drill contract for isolated recovery layout and cutover \
                                preconditions",
                },
                HealthEndpointContract {
                    path: "/health/operations",
                    signal: "operational-baseline",
                    success_status: StatusCode::OK.as_u16(),
                    failure_status: None,
                    semantics: "machine-readable runbook, SLO, RPO, RTO, and failure-drill \
                                baseline for the current stage of operations",
                },
                HealthEndpointContract {
                    path: "/health/compatibility",
                    signal: "compatibility-contract",
                    success_status: StatusCode::OK.as_u16(),
                    failure_status: Some(StatusCode::SERVICE_UNAVAILABLE.as_u16()),
                    semantics: "machine-readable contract for authoritative handshake truth \
                                versus discovery query hint, including the current phase-3 \
                                exact-match query protocol policy, authoritative auth-mode \
                                interpretation, first non-local Public entry handoff review \
                                contract, and lockstep rollout/rollback requirements",
                },
                HealthEndpointContract {
                    path: "/health/account-auth",
                    signal: "account-auth-governance",
                    success_status: StatusCode::OK.as_u16(),
                    failure_status: Some(StatusCode::SERVICE_UNAVAILABLE.as_u16()),
                    semantics: "machine-readable account, auth authority, identity anchor, and \
                                environment namespace contract for the current runtime topology",
                },
                HealthEndpointContract {
                    path: "/health/management-auth",
                    signal: "management-auth",
                    success_status: StatusCode::OK.as_u16(),
                    failure_status: None,
                    semantics: "machine-readable management and observability auth inventory, \
                                including loopback guards, secret transport, and remote exposure \
                                posture",
                },
                HealthEndpointContract {
                    path: "/health/transport-security",
                    signal: "transport-security",
                    success_status: StatusCode::OK.as_u16(),
                    failure_status: None,
                    semantics: "machine-readable QUIC/TLS transport inventory, rollout posture, \
                                and startup validation policy",
                },
                HealthEndpointContract {
                    path: "/health/listeners",
                    signal: "runtime-listeners",
                    success_status: StatusCode::OK.as_u16(),
                    failure_status: None,
                    semantics: "machine-readable runtime listener inventory for gameplay and \
                                discovery surfaces, including startup and post-start failure truth",
                },
                HealthEndpointContract {
                    path: "/health/observability",
                    signal: "observability-runtime",
                    success_status: StatusCode::OK.as_u16(),
                    failure_status: None,
                    semantics: "machine-readable runtime status for observability surfaces such \
                                as metrics export so anomalies do not stay log-only",
                },
                HealthEndpointContract {
                    path: "/health/world-compat",
                    signal: "world-compat",
                    success_status: StatusCode::OK.as_u16(),
                    failure_status: None,
                    semantics: "machine-readable runtime world file compatibility audit and \
                                recipe contract status, including strict load fallback review \
                                signals for dedicated startup",
                },
                HealthEndpointContract {
                    path: "/health/preflight",
                    signal: "operational-preflight",
                    success_status: StatusCode::OK.as_u16(),
                    failure_status: Some(StatusCode::SERVICE_UNAVAILABLE.as_u16()),
                    semantics: "aggregated release-facing preflight summary for readiness, backup \
                                scope, recovery drill posture, runtime listener truth, dedicated \
                                world compatibility review, management auth review, and \
                                governance findings",
                },
                HealthEndpointContract {
                    path: "/health/governance",
                    signal: "governance-audit",
                    success_status: StatusCode::OK.as_u16(),
                    failure_status: None,
                    semantics: "machine-readable advisory audit for accepted runtime risk posture \
                                and governance review findings",
                },
                HealthEndpointContract {
                    path: "/health/surfaces",
                    signal: "runtime-surfaces",
                    success_status: StatusCode::OK.as_u16(),
                    failure_status: None,
                    semantics: "machine-readable runtime surface inventory, including discovery, \
                                observability, control-plane, and gameplay posture",
                },
            ],
        }
    }

    pub(super) fn governance_report(&self) -> GovernanceReport {
        GovernanceReport {
            status: if self.governance_findings.is_empty() {
                "governance_clear"
            } else {
                "operator_review_required"
            },
            environment: self.environment,
            findings: self.governance_findings.clone(),
            requires_operator_review: !self.governance_findings.is_empty(),
        }
    }

    pub(super) fn surface_inventory_report(&self) -> SurfaceInventoryReport {
        let authoritative_query_auth_requirement = self.authoritative_auth_provider.is_some();
        SurfaceInventoryReport {
            status: "runtime-surface-inventory",
            environment: self.environment,
            entries: self
                .surface_inventory
                .iter()
                .map(|entry| {
                    let is_query_surface = entry.name == "query-server";
                    let query_surface_active = is_query_surface
                        && !matches!(
                            entry.reachability,
                            crate::settings::SurfaceReachability::Disabled
                        );
                    SurfaceEntryReport {
                        surface: entry.name,
                        bind_address: entry.bind_address.map(|address| address.to_string()),
                        reachability: entry.reachability.as_str(),
                        auth_scheme: entry.auth.as_str(),
                        credential_bootstrap: entry.credential_bootstrap.as_str(),
                        review_status: entry.review_status.as_str(),
                        remote_exposure_policy: entry.remote_exposure_policy.as_str(),
                        purpose: entry.purpose.as_str(),
                        consumption: entry.consumption.as_str(),
                        authority_scope: query_surface_active.then_some("discovery-hint-only"),
                        published_protocol_fields: if query_surface_active {
                            veloren_query_server::proto::PUBLISHED_SERVER_INFO_FIELDS.to_vec()
                        } else {
                            Vec::new()
                        },
                        auth_required: query_surface_active
                            .then_some(authoritative_query_auth_requirement),
                        detail: if is_query_surface {
                            "the query server publishes lightweight discovery hints for realm, \
                             environment, compatibility, auth requirement, build identity, and \
                             population state, but must not become the authoritative realm \
                             targeting, handshake, or release-routing source"
                                .to_owned()
                        } else {
                            format!(
                                "{} is currently classified as {} with {} reachability",
                                entry.name,
                                entry.purpose.as_str(),
                                entry.reachability.as_str()
                            )
                        },
                    }
                })
                .collect(),
        }
    }

    pub(super) fn management_auth_report(&self) -> ManagementAuthReport {
        let environment = match self.environment {
            "local" => crate::settings::Environment::Local,
            "test" => crate::settings::Environment::Test,
            _ => crate::settings::Environment::Production,
        };
        let review_surfaces =
            crate::settings::Settings::management_auth_review_surfaces_for_environment(
                environment,
                &self.management_auth_inventory,
            );

        ManagementAuthReport {
            status: if review_surfaces.is_empty() {
                "management-auth-inventory"
            } else {
                "operator_review_required"
            },
            environment: self.environment,
            requires_operator_review: !review_surfaces.is_empty(),
            review_surfaces,
            entries: self
                .management_auth_inventory
                .iter()
                .map(|entry| ManagementAuthEntryReport {
                    surface: entry.surface,
                    bind_address: entry.bind_address.map(|address| address.to_string()),
                    reachability: entry.reachability.as_str(),
                    review_status: entry.review_status.as_str(),
                    remote_exposure_policy: entry.remote_exposure_policy.as_str(),
                    capability: entry.capability.as_str(),
                    auth_scheme: entry.auth_scheme.as_str(),
                    credential_bootstrap: entry.credential_bootstrap.as_str(),
                    credential_transport: entry.credential_transport.as_str(),
                    secret_config_id: entry.secret_config_id,
                    proxy_forwarding_forbidden: entry.proxy_forwarding_forbidden,
                    detail: entry.detail.clone(),
                })
                .collect(),
        }
    }

    pub(super) fn transport_security_report(&self) -> TransportSecurityReport {
        TransportSecurityReport {
            status: "transport-security-inventory",
            environment: self.environment,
            entries: self
                .transport_security_inventory
                .iter()
                .map(|entry| TransportSecurityEntryReport {
                    surface: entry.surface,
                    bind_address: entry.bind_address.to_string(),
                    transport: entry.transport,
                    encryption: entry.encryption,
                    cert_file_path: entry.cert_file_path.display().to_string(),
                    key_file_path: entry.key_file_path.display().to_string(),
                    rollout_policy: entry.rollout_policy.as_str(),
                    validation_policy: entry.validation_policy.as_str(),
                    material_state: entry.material_state.as_str(),
                    detail: entry.detail.clone(),
                })
                .collect(),
        }
    }

    pub(super) fn runtime_listener_report(&self) -> RuntimeListenerReport {
        let entries = match self.runtime_listener_inventory.lock() {
            Ok(entries) => entries.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        };

        RuntimeListenerReport {
            status: "runtime-listener-inventory",
            environment: self.environment,
            entries: entries
                .into_iter()
                .map(|entry| RuntimeListenerEntryReport {
                    surface: entry.surface.as_str(),
                    bind_address: entry.bind_address.to_string(),
                    state: entry.state.as_str(),
                    detail: entry.detail,
                })
                .collect(),
        }
    }

    pub(super) fn runtime_observability_report(&self) -> RuntimeObservabilityReport {
        let entries =
            snapshot_runtime_observability_inventory(&self.runtime_observability_inventory);
        let requires_operator_review = entries
            .iter()
            .any(|entry| entry.state != RuntimeObservabilityState::Healthy);

        RuntimeObservabilityReport {
            status: if requires_operator_review {
                "operator_review_required"
            } else {
                "observability_clear"
            },
            environment: self.environment,
            requires_operator_review,
            entries: entries
                .into_iter()
                .map(|entry| {
                    let (
                        configured_mode,
                        compat_entry,
                        compat_decision,
                        compat_failure,
                        strict_load_contract_gap,
                        world_recipe_hash,
                        chunk_recipe_hash,
                        topology_id,
                        preset_id,
                    ) = match entry.context {
                        RuntimeObservabilityContext::None => {
                            (None, None, None, None, None, None, None, None, None)
                        },
                        #[cfg(feature = "worldgen")]
                        RuntimeObservabilityContext::WorldCompat(context) => (
                            Some(context.configured_mode),
                            Some(context.audit.entry.as_str()),
                            Some(context.audit.decision.as_str()),
                            Some(context.audit.failure_kind.as_str()),
                            Some(context.strict_load_contract_gap),
                            Some(context.world_recipe_hash),
                            Some(context.chunk_recipe_hash),
                            Some(context.topology_id),
                            Some(context.preset_id),
                        ),
                    };

                    RuntimeObservabilityEntryReport {
                        surface: entry.surface.as_str(),
                        state: entry.state.as_str(),
                        detail: entry.detail,
                        configured_mode,
                        compat_entry,
                        compat_decision,
                        compat_failure,
                        strict_load_contract_gap,
                        world_recipe_hash,
                        chunk_recipe_hash,
                        topology_id,
                        preset_id,
                    }
                })
                .collect(),
        }
    }

    pub(super) fn world_compat_report(&self) -> WorldCompatReport {
        #[cfg(not(feature = "worldgen"))]
        {
            WorldCompatReport {
                status: "world-compat-not-applicable",
                environment: self.environment,
                requires_operator_review: false,
                detail: "world generation is disabled for this build, so no world file \
                         compatibility contract applies"
                    .to_owned(),
                configured_mode: None,
                compat_entry: None,
                compat_decision: None,
                compat_failure: None,
                strict_load_contract_gap: None,
                world_recipe_hash: None,
                chunk_recipe_hash: None,
                topology_id: None,
                preset_id: None,
                source_surface: "world-compat",
            }
        }

        #[cfg(feature = "worldgen")]
        {
            let entries =
                snapshot_runtime_observability_inventory(&self.runtime_observability_inventory);
            let world_compat_entry = entries
                .into_iter()
                .find(|entry| entry.surface == RuntimeObservabilitySurface::WorldCompat);

            match world_compat_entry {
                Some(entry) => match entry.context {
                    RuntimeObservabilityContext::WorldCompat(context) => WorldCompatReport {
                        status: if context.strict_load_contract_gap {
                            "world-compat-review-required"
                        } else {
                            "world-compat-clear"
                        },
                        environment: self.environment,
                        requires_operator_review: context.strict_load_contract_gap,
                        detail: entry.detail,
                        configured_mode: Some(context.configured_mode),
                        compat_entry: Some(context.audit.entry.as_str()),
                        compat_decision: Some(context.audit.decision.as_str()),
                        compat_failure: Some(context.audit.failure_kind.as_str()),
                        strict_load_contract_gap: Some(context.strict_load_contract_gap),
                        world_recipe_hash: Some(context.world_recipe_hash),
                        chunk_recipe_hash: Some(context.chunk_recipe_hash),
                        topology_id: Some(context.topology_id),
                        preset_id: Some(context.preset_id),
                        source_surface: "world-compat",
                    },
                    RuntimeObservabilityContext::None => WorldCompatReport {
                        status: "world-compat-unrecorded",
                        environment: self.environment,
                        requires_operator_review: true,
                        detail: "world compatibility runtime surface is present but did not \
                                 publish structured compatibility context"
                            .to_owned(),
                        configured_mode: None,
                        compat_entry: None,
                        compat_decision: None,
                        compat_failure: None,
                        strict_load_contract_gap: None,
                        world_recipe_hash: None,
                        chunk_recipe_hash: None,
                        topology_id: None,
                        preset_id: None,
                        source_surface: "world-compat",
                    },
                },
                None => WorldCompatReport {
                    status: "world-compat-unrecorded",
                    environment: self.environment,
                    requires_operator_review: true,
                    detail: "dedicated startup did not publish a world compatibility status \
                             surface"
                        .to_owned(),
                    configured_mode: None,
                    compat_entry: None,
                    compat_decision: None,
                    compat_failure: None,
                    strict_load_contract_gap: None,
                    world_recipe_hash: None,
                    chunk_recipe_hash: None,
                    topology_id: None,
                    preset_id: None,
                    source_surface: "world-compat",
                },
            }
        }
    }

    pub(super) fn preflight_report(&self) -> PreflightReport {
        self.preflight_report_with_compatibility_contract(self.compatibility_contract_report())
    }

    fn preflight_report_with_compatibility_contract(
        &self,
        compatibility_contract: CompatibilityContractReport,
    ) -> PreflightReport {
        let ready = self.readiness_report();
        let backup = self.backup_report();
        let recovery_drill = self.recovery_drill_report();
        let public_entry_handoff = compatibility_contract.public_entry_handoff.clone();
        let (world_compat, world_compat_review_detail) = self.world_compat_preflight_component();
        let (runtime_listeners, runtime_listener_failures) =
            self.runtime_listener_preflight_component();
        let management_auth = self.management_auth_report();
        let governance = self.governance_report();
        let components = vec![
            PreflightComponentReport {
                signal: "readiness",
                endpoint: "/health/ready",
                status: ready.status,
                blocking: ready.status != "ready",
                requires_operator_review: false,
            },
            PreflightComponentReport {
                signal: "backup-preflight",
                endpoint: "/health/backup",
                status: backup.status,
                blocking: backup.status != "backup_ready",
                requires_operator_review: false,
            },
            PreflightComponentReport {
                signal: "recovery-drill",
                endpoint: "/health/recovery/drill",
                status: recovery_drill.status,
                blocking: recovery_drill.status != "drill_ready",
                requires_operator_review: false,
            },
            PreflightComponentReport {
                signal: "compatibility-contract",
                endpoint: "/health/compatibility",
                status: compatibility_contract.status,
                blocking: compatibility_contract.status != "compatibility-contract-aligned",
                requires_operator_review: false,
            },
            PreflightComponentReport {
                signal: public_entry_handoff.signal,
                endpoint: "/health/compatibility",
                status: public_entry_handoff.status,
                blocking: public_entry_handoff.release_blocked,
                requires_operator_review: public_entry_handoff.requires_operator_review,
            },
            world_compat,
            runtime_listeners,
            PreflightComponentReport {
                signal: "management-auth",
                endpoint: "/health/management-auth",
                status: management_auth.status,
                blocking: false,
                requires_operator_review: management_auth.requires_operator_review,
            },
            PreflightComponentReport {
                signal: "governance-audit",
                endpoint: "/health/governance",
                status: governance.status,
                blocking: false,
                requires_operator_review: governance.requires_operator_review,
            },
        ];
        let release_blocked = components.iter().any(|component| component.blocking);
        let requires_operator_review = components
            .iter()
            .any(|component| component.requires_operator_review);
        let blocking_signals = components
            .iter()
            .filter(|component| component.blocking)
            .map(|component| component.signal)
            .collect::<Vec<_>>();
        let review_signals = components
            .iter()
            .filter(|component| component.requires_operator_review)
            .map(|component| component.signal)
            .collect::<Vec<_>>();
        let follow_up_endpoints = components
            .iter()
            .filter_map(|component| {
                if component.blocking {
                    Some(PreflightFollowUpEndpoint {
                        signal: component.signal,
                        endpoint: component.endpoint,
                        blocking: true,
                        owner: "service-operator",
                        reason: "inspect the detailed component report and clear blocking runtime \
                                 preflight failures before rollout",
                    })
                } else if component.requires_operator_review {
                    let reason = match component.signal {
                        "public-entry-handoff" => {
                            "inspect the Public entry handoff contract and record cutover / \
                             rollback review fields before rollout"
                        },
                        "world-compat" => {
                            "inspect the dedicated world compatibility surface and record explicit \
                             review before rollout"
                        },
                        _ => {
                            "inspect advisory governance findings and record explicit review \
                             before rollout"
                        },
                    };
                    Some(PreflightFollowUpEndpoint {
                        signal: component.signal,
                        endpoint: component.endpoint,
                        blocking: false,
                        owner: "release-operator",
                        reason,
                    })
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        let mut review_decision_contracts = Vec::new();
        let mut operator_review_items = Vec::new();

        if release_blocked {
            operator_review_items.push(PreflightReviewItem {
                kind: "resolve-blocking-runtime-checks",
                blocking: true,
                detail: "clear required failures in /health/ready, /health/backup, \
                         /health/recovery/drill, /health/compatibility, and /health/listeners \
                         before treating the instance as rollout-ready"
                    .to_owned(),
            });
        }

        if compatibility_contract.status != "compatibility-contract-aligned" {
            operator_review_items.push(PreflightReviewItem {
                kind: "compatibility-contract-drift",
                blocking: true,
                detail: format!(
                    "authoritative handshake truth and query hint contract disagree in {} \
                     environment; current query protocol is exact-match, so keep operator-managed \
                     query consumers and the server in lockstep and inspect /health/compatibility \
                     before rollout",
                    self.environment
                ),
            });
        }

        if !runtime_listener_failures.is_empty() {
            operator_review_items.push(PreflightReviewItem {
                kind: "runtime-listener-failure",
                blocking: true,
                detail: format!(
                    "required listener startup truth is not clean: {}",
                    runtime_listener_failures.join("; ")
                ),
            });
        }

        if public_entry_handoff.release_blocked {
            operator_review_items.push(PreflightReviewItem {
                kind: "public-entry-handoff-blocked",
                blocking: true,
                detail: format!(
                    "non-local Public rollout requires external auth authority, but the \
                     authoritative handshake auth mode is {}. Inspect /health/compatibility \
                     before rollout",
                    public_entry_handoff.expected_handshake_auth_mode
                ),
            });
        } else if public_entry_handoff.requires_operator_review {
            operator_review_items.extend(repo_bundled_official_entry_snapshot_review_items(
                &public_entry_handoff.repo_bundled_official_entry_snapshot,
            ));
            operator_review_items.push(public_entry_handoff_review_item());
            review_decision_contracts.push(
                public_entry_handoff_preflight_review_decision_contract(&public_entry_handoff),
            );
        }

        if let Some(detail) = world_compat_review_detail {
            operator_review_items.push(PreflightReviewItem {
                kind: "world-compat-review",
                blocking: false,
                detail: format!(
                    "review dedicated world compatibility status in /health/world-compat before \
                     rollout: {detail}"
                ),
            });
        }

        operator_review_items.extend(governance.findings.iter().map(|finding| {
            PreflightReviewItem {
                kind: "governance-finding-review",
                blocking: false,
                detail: format!(
                    "review governance finding {} for {}: {}",
                    finding.id, finding.subject, finding.detail
                ),
            }
        }));
        if governance.requires_operator_review {
            let mut governance_supporting_endpoints = Vec::new();
            for finding in &governance.findings {
                for contract in finding.supporting_contracts() {
                    if let Some(existing) = governance_supporting_endpoints.iter_mut().find(
                        |endpoint: &&mut PreflightSupportingEndpoint| {
                            endpoint.signal == contract.signal
                                && endpoint.endpoint == contract.endpoint
                        },
                    ) {
                        if !existing.related_findings.iter().any(|id| *id == finding.id) {
                            existing.related_findings.push(finding.id);
                        }
                    } else {
                        governance_supporting_endpoints.push(PreflightSupportingEndpoint {
                            signal: contract.signal,
                            endpoint: contract.endpoint,
                            owner: "release-operator",
                            purpose: contract.purpose,
                            related_findings: vec![finding.id],
                        });
                    }
                }
            }
            review_decision_contracts.push(governance_preflight_review_decision_contract(
                governance_supporting_endpoints,
            ));
        }

        if management_auth.requires_operator_review {
            operator_review_items.push(PreflightReviewItem {
                kind: "management-auth-review",
                blocking: false,
                detail: format!(
                    "review remote management auth posture for {} in /health/management-auth \
                     before rollout",
                    management_auth.review_surfaces.join(", ")
                ),
            });
            review_decision_contracts.push(management_auth_preflight_review_decision_contract());
        }

        PreflightReport {
            status: if release_blocked {
                "preflight_blocked"
            } else if requires_operator_review {
                "operator_review_required"
            } else {
                "preflight_clear"
            },
            environment: self.environment,
            release_blocked,
            requires_operator_review,
            development_stage_closure_status: compatibility_contract
                .public_entry_handoff
                .development_stage_closure_status,
            real_cutover_execution_status: compatibility_contract
                .public_entry_handoff
                .real_cutover_execution_status,
            remaining_external_execution_dependencies: compatibility_contract
                .public_entry_handoff
                .remaining_external_execution_dependencies
                .clone(),
            repo_bundled_official_entry_snapshot: compatibility_contract
                .public_entry_handoff
                .repo_bundled_official_entry_snapshot
                .clone(),
            components,
            blocking_signals,
            review_signals,
            follow_up_endpoints,
            review_decision_contracts,
            operator_review_items,
            required_signoff_fields: vec![
                "reviewed_by",
                "approval_decision",
                "decision_recorded_at_utc",
                "release_reference",
            ],
            post_review_actions: vec![
                "record the preflight outcome in the external release tracker before reopening \
                 traffic",
                "link governance finding review notes or rollback references when exceptions are \
                 accepted",
                "after terminal external review sections are archived, write archive receipt and \
                 post_archive verification fields back to the same external release tracker \
                 section",
            ],
            release_gate: "do not reopen traffic or treat the instance as rollout-ready until \
                           blocking preflight checks are clear and governance findings have been \
                           reviewed by an operator",
        }
    }

    pub(super) fn metrics_contract(&self) -> MetricsContract {
        MetricsContract {
            surface: "metrics",
            environment: self.environment,
            consumption: "machine-scrape",
            content_type: "text/plain; version=0.0.4; charset=utf-8",
            cache_policy: "no-store",
            scrape_mode: "prometheus-text-export",
            readiness_signal: false,
            interpretation_boundary: "metrics are observability signals for scrape, trend, and \
                                      anomaly detection; they do not replace readiness, \
                                      preflight, or management authorization contracts",
            signal_families: vec![
                MetricsSignalFamilyContract {
                    family: "server-loop-and-world-state",
                    purpose: "server tick cadence, active world footprint, build identity, and \
                              coarse runtime state",
                    example_metrics: vec![
                        "tick_time",
                        "tick_time_hist",
                        "chunks_count",
                        "entity_count",
                        "veloren_build_info",
                    ],
                    rollout_use: "watch the steady-state gameplay loop and coarse world pressure \
                                  during rollout and incident review",
                },
                MetricsSignalFamilyContract {
                    family: "player-and-session-traffic",
                    purpose: "connected client/player counts and disconnect reasons",
                    example_metrics: vec![
                        "clients_connected",
                        "players_connected",
                        "clients_disconnected",
                    ],
                    rollout_use: "watch player-impacting churn, connection health, and live \
                                  population trends",
                },
                MetricsSignalFamilyContract {
                    family: "chunk-and-job-pipeline",
                    purpose: "chunk request/generation/serialization pressure and slow-job queue \
                              timing",
                    example_metrics: vec![
                        "chunks_requested",
                        "chunks_generation_triggered",
                        "job_execution_hst",
                        "job_queried_hst",
                    ],
                    rollout_use: "watch terrain generation and worker-pool pressure when \
                                  diagnosing throughput or latency regressions",
                },
                MetricsSignalFamilyContract {
                    family: "ecs-and-server-events",
                    purpose: "ECS system execution timing and handled server event volume",
                    example_metrics: vec![
                        "system_length_hist",
                        "system_length_time",
                        "event_count",
                    ],
                    rollout_use: "inspect which systems or event classes dominate runtime cost \
                                  during a bad tick or live incident",
                },
                MetricsSignalFamilyContract {
                    family: "network-and-discovery",
                    purpose: "network request handling and query-server request/response health",
                    example_metrics: vec![
                        "chunks_request_dropped",
                        "chunks_served_lossless",
                        "query_server::received_packets",
                        "query_server::failed_responses",
                    ],
                    rollout_use: "watch gameplay-adjacent network load and discovery-plane health \
                                  without treating the discovery plane as handshake authority",
                },
                MetricsSignalFamilyContract {
                    family: "physics-and-collision",
                    purpose: "collision-check and collision-detection counters for physics \
                              hotspots",
                    example_metrics: vec![
                        "entity_entity_collision_checks_count",
                        "entity_entity_collisions_count",
                    ],
                    rollout_use: "inspect physics hotspot growth when tick-time regressions \
                                  correlate with dense simulation or combat activity",
                },
            ],
        }
    }

    pub(super) fn operational_baseline_report(&self) -> OperationalBaselineReport {
        OperationalBaselineReport {
            status: "operational-baseline",
            environment: self.environment,
            target_tick_rate: crate::TPS,
            target_tick_interval_millis: 1_000 / crate::TPS,
            failure_drill_cadence: "recurring restore drill program outside the game server \
                                    process",
            runbook_baseline: vec![
                "treat /health/live as the basic process liveness probe and /health/ready as the \
                 local runtime dependency gate",
                "treat /health/preflight as the release-facing rollout gate before reopening \
                 traffic",
                "freeze or stop writes before taking a filesystem-level snapshot and require \
                 /health/backup to stay clear",
                "restore only into the isolated recovery staging layout and require \
                 /health/recovery/drill plus /health/ready before cutover",
                "inspect /health/listeners and local operational audit trail whenever a \
                 configured listener fails to start or stops unexpectedly",
            ],
            observability_channels: vec![
                ObservabilityChannelContract {
                    channel: "runtime-log-stream",
                    purpose: "interactive diagnostics and process-level tracing",
                    sink: "stdout/stderr stream plus optional in-process tui buffer".to_owned(),
                    retention_policy: "no in-process retention contract; rely on external process \
                                       supervisor or terminal capture",
                    retention_owner: "external-process-supervisor",
                    authoritative_for_ops_audit: false,
                },
                ObservabilityChannelContract {
                    channel: "operational-audit-trail",
                    purpose: "authoritative local operator action and runtime failure trail",
                    sink: self.server_state.audit_log_file.display().to_string(),
                    retention_policy: "startup-maintained local retention policy with archive \
                                       rotation and capped archive count",
                    retention_owner: "this-process",
                    authoritative_for_ops_audit: true,
                },
                ObservabilityChannelContract {
                    channel: "backup-evidence-sink",
                    purpose: "local collaborator trail for backup execution status transitions",
                    sink: self
                        .server_state
                        .backup_evidence_log_file
                        .display()
                        .to_string(),
                    retention_policy: "best-effort local sink; authoritative long-term archive \
                                       remains external",
                    retention_owner: "external-orchestrator",
                    authoritative_for_ops_audit: false,
                },
                ObservabilityChannelContract {
                    channel: "recovery-drill-evidence-sink",
                    purpose: "local collaborator trail for restore-drill execution status \
                              transitions",
                    sink: self
                        .server_state
                        .recovery_drill_evidence_log_file
                        .display()
                        .to_string(),
                    retention_policy: "best-effort local sink; authoritative long-term archive \
                                       remains external",
                    retention_owner: "external-orchestrator",
                    authoritative_for_ops_audit: false,
                },
            ],
            objectives: vec![
                OperationalBaselineObjective {
                    category: "slo",
                    name: "gameplay-tick-rate",
                    target: "hold the configured server loop tick-rate baseline",
                    measurement: "the configured server loop tick-rate and tick-interval baseline \
                                  exported in this report",
                    owner: "this-process",
                    supporting_endpoints: vec!["/metrics"],
                },
                OperationalBaselineObjective {
                    category: "slo",
                    name: "rollout-readiness",
                    target: "do not reopen traffic until readiness and preflight are both clear",
                    measurement: "operator checks /health/ready and /health/preflight before \
                                  declaring the instance rollout-ready",
                    owner: "service-operator",
                    supporting_endpoints: vec![
                        "/health/ready",
                        "/health/preflight",
                        "/health/listeners",
                    ],
                },
                OperationalBaselineObjective {
                    category: "rpo",
                    name: "backup-recovery-point",
                    target: "must-keep state enters a quiesced filesystem-level backup scope and \
                             is restore-verified before acceptance",
                    measurement: "operator checks /health/backup plus backup evidence before \
                                  treating a snapshot as an accepted recovery point",
                    owner: "external-orchestrator",
                    supporting_endpoints: vec!["/health/backup", "/health/recovery"],
                },
                OperationalBaselineObjective {
                    category: "rto",
                    name: "staged-restore-cutover",
                    target: "restore into isolated staging, reach ready-validated state, and \
                             record sign-off before cutover",
                    measurement: "operator checks /health/recovery/drill, /health/ready, and \
                                  drill evidence before approving cutover",
                    owner: "external-orchestrator",
                    supporting_endpoints: vec![
                        "/health/recovery/drill",
                        "/health/ready",
                        "/health/preflight",
                    ],
                },
            ],
        }
    }

    pub(super) fn recovery_contract(&self) -> RecoveryContract {
        let state_inventory = self
            .server_state
            .inventory()
            .into_iter()
            .map(|entry| RecoveryEntryContract {
                kind: match entry.kind {
                    server::ServerStateKind::ConfigDir => "config-dir",
                    server::ServerStateKind::InstanceIdentity => "instance-identity",
                    server::ServerStateKind::CharacterDatabase => "character-database",
                    server::ServerStateKind::RtSimState => "rtsim-state",
                    server::ServerStateKind::TerrainPersistence => "terrain-persistence",
                    server::ServerStateKind::OperationalAuditTrail => "operational-audit-trail",
                    server::ServerStateKind::BackupEvidenceTrail => "backup-evidence-trail",
                    server::ServerStateKind::RecoveryDrillEvidenceTrail => {
                        "recovery-drill-evidence-trail"
                    },
                },
                path: entry.path.display().to_string(),
                data_domain: match entry.domain {
                    server::ServerStateDomain::EnvironmentConfig => "environment-config",
                    server::ServerStateDomain::InstanceMetadata => "instance-metadata",
                    server::ServerStateDomain::CharacterPersistence => "character-persistence",
                    server::ServerStateDomain::WorldRuntime => "world-runtime",
                    server::ServerStateDomain::OperationalEvidence => "operational-evidence",
                },
                write_owner: match entry.write_owner {
                    server::ServerStateWriteOwner::ServerCoreSettings => "server-core-settings",
                    server::ServerStateWriteOwner::ServerCoreIdentity => "server-core-identity",
                    server::ServerStateWriteOwner::ServerCorePersistence => {
                        "server-core-persistence"
                    },
                    server::ServerStateWriteOwner::ServerCoreRtSim => "server-core-rtsim",
                    server::ServerStateWriteOwner::ServerCoreTerrainPersistence => {
                        "server-core-terrain-persistence"
                    },
                    server::ServerStateWriteOwner::ServerCliOperations => "server-cli-ops",
                },
                consistency_requirement: match entry.consistency {
                    server::ServerStateConsistency::StableAuthoritative => "stable-authoritative",
                    server::ServerStateConsistency::AuthoritativeWithOperatorReview => {
                        "authoritative-with-operator-review"
                    },
                    server::ServerStateConsistency::DerivedRebuildable => "derived-rebuildable",
                    server::ServerStateConsistency::AppendOnlyEvidence => "append-only-evidence",
                },
                migration_strategy: match entry.migration {
                    server::ServerStateMigration::ManualFileReview => "manual-file-review",
                    server::ServerStateMigration::PreserveOrRepair => "preserve-or-repair",
                    server::ServerStateMigration::SchemaManagedInProcess => {
                        "schema-managed-in-process"
                    },
                    server::ServerStateMigration::RebuildOrDiscard => "rebuild-or-discard",
                    server::ServerStateMigration::RotateAndArchive => "rotate-and-archive",
                },
                recovery_class: match entry.recovery {
                    server::RecoveryClass::MustKeep => "must-keep",
                    server::RecoveryClass::ManualRepair => "manual-repair",
                    server::RecoveryClass::Rebuildable => "rebuildable",
                },
                backup_expectation: match entry.recovery {
                    server::RecoveryClass::MustKeep => "required-in-backup",
                    server::RecoveryClass::ManualRepair => "recommended-in-backup",
                    server::RecoveryClass::Rebuildable => "may-be-rebuilt",
                },
            })
            .collect();

        RecoveryContract {
            surface: "health",
            environment: self.environment,
            cache_policy: "no-store",
            state_inventory,
            recovery_staging: RecoveryStagingContract {
                root_dir: self.recovery_staging_state.data_dir.display().to_string(),
                isolated_from_live_state: crate::settings::recovery_drill_overlap_details(
                    &self.server_state,
                    &self.recovery_staging_state,
                )
                .is_empty(),
                expected_identity_file: self
                    .recovery_staging_state
                    .identity_file
                    .display()
                    .to_string(),
                expected_database_file: self
                    .recovery_staging_state
                    .database_file
                    .display()
                    .to_string(),
                expected_config_dir: self.recovery_staging_state.config_dir.display().to_string(),
            },
            audit_retention: AuditRetentionContract {
                active_log_path: self.server_state.audit_log_file.display().to_string(),
                archive_pattern: audit_archive_pattern(&self.server_state.audit_log_file),
                format: "ron-line",
                rotation_trigger: "startup-if-active-file-exceeds-threshold",
                max_active_file_mebibytes: self.audit_retention.max_active_file_mebibytes,
                max_archive_files: self.audit_retention.max_archive_files,
                maintenance_point: "server-cli startup",
            },
            minimum_runbook: vec![
                "quiesce writes before taking a filesystem-level backup snapshot",
                "include must-keep and manual-repair paths in backup scope",
                "verify identity.ron is parseable and db.sqlite is openable as SQLite with at \
                 least one non-SQLite table before declaring the instance ready",
                "preserve the audit trail when practical so operator actions remain \
                 reconstructable",
            ],
            minimum_recovery_drill: vec![
                "restore a backup into an isolated directory instead of the live data directory",
                "verify parseable realm identity, db.sqlite openable as SQLite with at least one \
                 non-SQLite table, and required config before startup",
                "start the restored instance with writes quiesced and confirm readiness before \
                 reopening traffic",
            ],
        }
    }

    pub(super) fn recovery_drill_report(&self) -> RecoveryDrillReport {
        let overlap_details = crate::settings::recovery_drill_overlap_details(
            &self.server_state,
            &self.recovery_staging_state,
        );
        let live_identity_file_probe =
            server::settings::inspect_identity_file(&self.server_state.data_dir);
        let live_database_file_probe =
            server::persistence::inspect_database_file(&self.server_state.database_file);
        let staged_identity_file_probe =
            server::settings::inspect_identity_file(&self.recovery_staging_state.data_dir);
        let staged_database_file_probe =
            server::persistence::inspect_database_file(&self.recovery_staging_state.database_file);
        let live_settings_file_probe =
            server::settings::inspect_settings_file(&self.server_state.data_dir);
        let staged_settings_file_probe =
            server::settings::inspect_settings_file(&self.recovery_staging_state.data_dir);
        let mut checks = vec![
            RecoveryDrillCheck {
                name: "recovery-staging-dir-present",
                ok: self.recovery_staging_state.data_dir.is_dir(),
                required: true,
                detail: format!(
                    "expected isolated restore root at {}",
                    self.recovery_staging_state.data_dir.display()
                ),
            },
            RecoveryDrillCheck {
                name: "recovery-staging-layout-isolated",
                ok: overlap_details.is_empty(),
                required: true,
                detail: if overlap_details.is_empty() {
                    format!(
                        "recovery staging layout is isolated from live state under {}",
                        self.recovery_staging_state.data_dir.display()
                    )
                } else {
                    overlap_details.join("; ")
                },
            },
            RecoveryDrillCheck {
                name: "live-identity-file-ready",
                ok: live_identity_file_probe.is_ready(),
                required: true,
                detail: live_identity_file_probe.detail(),
            },
            RecoveryDrillCheck {
                name: "live-database-file-ready",
                ok: live_database_file_probe.is_ready(),
                required: true,
                detail: live_database_file_probe.detail(),
            },
            RecoveryDrillCheck {
                name: "live-config-source-present",
                ok: self.server_state.config_dir.is_dir(),
                required: true,
                detail: format!(
                    "expected live config source at {}",
                    self.server_state.config_dir.display()
                ),
            },
            RecoveryDrillCheck {
                name: "live-settings-file-ready",
                ok: live_settings_file_probe.is_ready(),
                required: true,
                detail: live_settings_file_probe.detail(),
            },
            RecoveryDrillCheck {
                name: "staged-identity-file-ready",
                ok: staged_identity_file_probe.is_ready(),
                required: true,
                detail: staged_identity_file_probe.detail(),
            },
            RecoveryDrillCheck {
                name: "staged-database-file-ready",
                ok: staged_database_file_probe.is_ready(),
                required: true,
                detail: staged_database_file_probe.detail(),
            },
            RecoveryDrillCheck {
                name: "staged-config-present",
                ok: self.recovery_staging_state.config_dir.is_dir(),
                required: true,
                detail: format!(
                    "expected restored config under isolated staging layout at {}",
                    self.recovery_staging_state.config_dir.display()
                ),
            },
            RecoveryDrillCheck {
                name: "staged-settings-file-ready",
                ok: staged_settings_file_probe.is_ready(),
                required: true,
                detail: staged_settings_file_probe.detail(),
            },
            RecoveryDrillCheck {
                name: "staged-audit-trail-clear",
                ok: !path_exists(&self.recovery_staging_state.audit_log_file),
                required: true,
                detail: if path_exists(&self.recovery_staging_state.audit_log_file) {
                    format!(
                        "remove restored local operational audit trail from isolated staging \
                         layout before cutover: {}",
                        self.recovery_staging_state.audit_log_file.display()
                    )
                } else {
                    format!(
                        "isolated staging layout is clear of restored local audit trail at {}",
                        self.recovery_staging_state.audit_log_file.display()
                    )
                },
            },
            RecoveryDrillCheck {
                name: "staged-backup-evidence-clear",
                ok: !path_exists(&self.recovery_staging_state.backup_evidence_log_file),
                required: true,
                detail: if path_exists(&self.recovery_staging_state.backup_evidence_log_file) {
                    format!(
                        "remove restored local backup evidence trail from isolated staging layout \
                         before cutover: {}",
                        self.recovery_staging_state
                            .backup_evidence_log_file
                            .display()
                    )
                } else {
                    format!(
                        "isolated staging layout is clear of restored local backup evidence at {}",
                        self.recovery_staging_state
                            .backup_evidence_log_file
                            .display()
                    )
                },
            },
            RecoveryDrillCheck {
                name: "staged-recovery-drill-evidence-clear",
                ok: !path_exists(&self.recovery_staging_state.recovery_drill_evidence_log_file),
                required: true,
                detail: if path_exists(
                    &self.recovery_staging_state.recovery_drill_evidence_log_file,
                ) {
                    format!(
                        "remove restored local recovery-drill evidence trail from isolated \
                         staging layout before cutover: {}",
                        self.recovery_staging_state
                            .recovery_drill_evidence_log_file
                            .display()
                    )
                } else {
                    format!(
                        "isolated staging layout is clear of restored local recovery-drill \
                         evidence at {}",
                        self.recovery_staging_state
                            .recovery_drill_evidence_log_file
                            .display()
                    )
                },
            },
        ];
        let evidence_sink_checks = evidence_sink_checks(
            "recovery-drill-evidence-sink",
            &self.server_state.recovery_drill_evidence_log_file,
            [
                &self.server_state.audit_log_file,
                &self.server_state.backup_evidence_log_file,
            ],
        );

        let ready = checks.iter().all(|check| !check.required || check.ok)
            && evidence_sink_checks
                .iter()
                .all(|check| !check.required || check.ok);

        RecoveryDrillReport {
            status: if ready {
                "drill_ready"
            } else {
                "drill_blocked"
            },
            environment: self.environment,
            live_data_dir: self.server_state.data_dir.display().to_string(),
            staging_data_dir: self.recovery_staging_state.data_dir.display().to_string(),
            checks: std::mem::take(&mut checks),
            evidence_sink_checks,
            cutover_preconditions: vec![
                "restore the snapshot into the isolated recovery staging directory, not the live \
                 data directory",
                "verify restored parseable identity.ron, db.sqlite openable as SQLite with at \
                 least one non-SQLite table, and parseable settings.ron exist under the staging \
                 layout",
                "clear restored local audit and evidence trails from the staging layout before \
                 treating the restore as cutover-ready",
                "start the restored instance from the staging layout and require /health/ready to \
                 report ready before reopening traffic",
            ],
            responsibility_boundary: ResponsibilityBoundary {
                this_process: vec![
                    "publish the isolated recovery staging layout and required live restore \
                     sources",
                    "report restore-drill validation checks and cutover preconditions",
                ],
                external_orchestrator: vec![
                    "schedule and execute periodic restore drills",
                    "record drill evidence, operator sign-off, and post-drill findings",
                    "coordinate traffic drain, cutover approval, and rollback outside the game \
                     server process",
                ],
            },
            evidence_sink: EvidenceSinkContract {
                path: self
                    .server_state
                    .recovery_drill_evidence_log_file
                    .display()
                    .to_string(),
                format: "ron-line",
                owner: "external-orchestrator",
                semantics: "append-only local evidence trail for restore-drill executions when a \
                            colocated operator or orchestrator persists drill results on the host",
            },
            evidence_write_contract: EvidenceWriteContract {
                format: "ron-line",
                write_mode: "append-only",
                record_granularity: "one-record-per-drill-status-transition",
                minimum_common_fields: vec!["timestamp_utc", "status", "summary", "writer"],
                in_process_maintenance: "none",
                local_sink_retention_role: "best-effort local collaborator trail, not the \
                                            authoritative long-term archive",
                external_archive_required: true,
            },
            archive_handoff_contract: EvidenceArchiveHandoffContract {
                authoritative_archive_owner: "external-orchestrator",
                local_record_completion_signal: "a local append-only drill status-transition \
                                                 record with the minimum common fields written to \
                                                 the evidence sink",
                handoff_ready_states: vec![
                    "restored",
                    "ready-validated",
                    "cutover-approved",
                    "rolled-back",
                ],
                terminal_states_requiring_archive_receipt: vec!["cutover-approved", "rolled-back"],
                required_archive_receipt_fields: vec![
                    "archive_reference",
                    "archived_at_utc",
                    "archived_by",
                    "source_record_status",
                ],
                local_record_not_sufficient_without_archive: true,
            },
            execution_contract: RecoveryDrillExecutionContract {
                execution_owner: "external-orchestrator",
                cadence_expectation: "recurring restore drill program outside the game server \
                                      process, not a one-off manual reminder",
                release_gate: "do not approve cutover unless the drill reached ready-validated, \
                               evidence review is complete, and operator sign-off is recorded",
                required_signoff_fields: vec![
                    "drill_id",
                    "reviewed_by",
                    "approval_decision",
                    "decision_recorded_at_utc",
                ],
                required_post_drill_actions: vec![
                    "record findings or deviations discovered during the drill",
                    "record rollback reference when cutover is rejected or reverted",
                    "handoff terminal drill evidence into the authoritative external archive",
                ],
            },
            evidence_requirements: vec![
                EvidenceFieldContract {
                    name: "drill_id",
                    owner: "external-orchestrator",
                    semantics: "stable identifier for one restore drill execution",
                },
                EvidenceFieldContract {
                    name: "backup_artifact_id",
                    owner: "external-orchestrator",
                    semantics: "backup artifact or snapshot identifier used for the drill",
                },
                EvidenceFieldContract {
                    name: "restored_layout_root",
                    owner: "this-process-and-operator",
                    semantics: "isolated staging path that received the restored state",
                },
                EvidenceFieldContract {
                    name: "ready_report_status",
                    owner: "this-process",
                    semantics: "observed /health/ready result before any cutover decision",
                },
                EvidenceFieldContract {
                    name: "cutover_decision",
                    owner: "external-orchestrator",
                    semantics: "approved, rejected, or rolled-back decision recorded after the \
                                drill",
                },
                EvidenceFieldContract {
                    name: "rollback_reference",
                    owner: "external-orchestrator",
                    semantics: "reference to the rollback note, ticket, or procedure used if the \
                                drill did not proceed",
                },
            ],
            result_status_model: vec![
                ResultStatusContract {
                    state: "planned",
                    semantics: "drill defined but restore execution not yet recorded",
                },
                ResultStatusContract {
                    state: "restored",
                    semantics: "backup material restored into the isolated staging layout",
                },
                ResultStatusContract {
                    state: "ready-validated",
                    semantics: "restored instance reached the required readiness gate before any \
                                cutover decision",
                },
                ResultStatusContract {
                    state: "cutover-approved",
                    semantics: "operators approved cutover after reviewing drill evidence",
                },
                ResultStatusContract {
                    state: "rolled-back",
                    semantics: "drill stopped or reverted and the rollback reference must be \
                                recorded",
                },
            ],
        }
    }

    pub(super) fn backup_report(&self) -> BackupReport {
        let checks = self
            .server_state
            .inventory()
            .into_iter()
            .map(|entry| {
                let required = matches!(entry.recovery, server::RecoveryClass::MustKeep);
                let backup_expectation = match entry.recovery {
                    server::RecoveryClass::MustKeep => "required-in-backup",
                    server::RecoveryClass::ManualRepair => "recommended-in-backup",
                    server::RecoveryClass::Rebuildable => "may-be-rebuilt",
                };
                let (ok, detail) = match entry.kind {
                    server::ServerStateKind::CharacterDatabase => {
                        let probe = server::persistence::inspect_database_file(&entry.path);
                        (probe.is_ready(), probe.detail())
                    },
                    _ => {
                        let ok = path_exists(&entry.path);
                        let detail = if ok {
                            format!("backup source path present at {}", entry.path.display())
                        } else {
                            format!("backup source path missing at {}", entry.path.display())
                        };
                        (ok, detail)
                    },
                };
                BackupCheck {
                    kind: match entry.kind {
                        server::ServerStateKind::ConfigDir => "config-dir",
                        server::ServerStateKind::InstanceIdentity => "instance-identity",
                        server::ServerStateKind::CharacterDatabase => "character-database",
                        server::ServerStateKind::RtSimState => "rtsim-state",
                        server::ServerStateKind::TerrainPersistence => "terrain-persistence",
                        server::ServerStateKind::OperationalAuditTrail => "operational-audit-trail",
                        server::ServerStateKind::BackupEvidenceTrail => "backup-evidence-trail",
                        server::ServerStateKind::RecoveryDrillEvidenceTrail => {
                            "recovery-drill-evidence-trail"
                        },
                    },
                    path: entry.path.display().to_string(),
                    ok,
                    required,
                    backup_expectation,
                    detail,
                }
            })
            .collect::<Vec<_>>();
        let evidence_sink_checks = evidence_sink_checks(
            "backup-evidence-sink",
            &self.server_state.backup_evidence_log_file,
            [
                &self.server_state.audit_log_file,
                &self.server_state.recovery_drill_evidence_log_file,
            ],
        );
        let ready = checks.iter().all(|check| !check.required || check.ok)
            && evidence_sink_checks
                .iter()
                .all(|check| !check.required || check.ok);

        BackupReport {
            status: if ready {
                "backup_ready"
            } else {
                "backup_blocked"
            },
            environment: self.environment,
            checks,
            evidence_sink_checks,
            quiesce_requirement: "freeze or stop writes before taking a filesystem-level snapshot",
            restore_verification: vec![
                "identity.ron should exist and match the intended realm",
                "db.sqlite should be openable as SQLite and expose at least one non-SQLite table \
                 before restore is accepted as complete",
                "manual-repair paths should be reviewed before reopening traffic",
            ],
            responsibility_boundary: ResponsibilityBoundary {
                this_process: vec![
                    "publish the required backup scope from the live state inventory",
                    "fail preflight when required local backup sources are missing or unusable",
                    "describe quiesce and restore verification expectations for filesystem-level \
                     backups",
                ],
                external_orchestrator: vec![
                    "schedule consistent snapshots or backup jobs",
                    "copy backup artifacts off-host or into external object storage",
                    "enforce archive retention, replication, and restore evidence outside the \
                     local instance",
                ],
            },
            evidence_sink: EvidenceSinkContract {
                path: self
                    .server_state
                    .backup_evidence_log_file
                    .display()
                    .to_string(),
                format: "ron-line",
                owner: "external-orchestrator",
                semantics: "append-only local evidence trail for retained backup artifacts when a \
                            colocated operator or orchestrator persists backup results on the host",
            },
            evidence_write_contract: EvidenceWriteContract {
                format: "ron-line",
                write_mode: "append-only",
                record_granularity: "one-record-per-backup-status-transition",
                minimum_common_fields: vec!["timestamp_utc", "status", "summary", "writer"],
                in_process_maintenance: "none",
                local_sink_retention_role: "best-effort local collaborator trail, not the \
                                            authoritative long-term archive",
                external_archive_required: true,
            },
            archive_handoff_contract: EvidenceArchiveHandoffContract {
                authoritative_archive_owner: "external-orchestrator",
                local_record_completion_signal: "a local append-only backup status-transition \
                                                 record with the minimum common fields written to \
                                                 the evidence sink",
                handoff_ready_states: vec!["captured", "restore-verified", "rejected"],
                terminal_states_requiring_archive_receipt: vec!["restore-verified", "rejected"],
                required_archive_receipt_fields: vec![
                    "archive_reference",
                    "archived_at_utc",
                    "archived_by",
                    "source_record_status",
                ],
                local_record_not_sufficient_without_archive: true,
            },
            evidence_requirements: vec![
                EvidenceFieldContract {
                    name: "backup_artifact_id",
                    owner: "external-orchestrator",
                    semantics: "stable identifier for the snapshot or backup artifact set",
                },
                EvidenceFieldContract {
                    name: "backup_completed_at_utc",
                    owner: "external-orchestrator",
                    semantics: "completion timestamp for the consistent backup point",
                },
                EvidenceFieldContract {
                    name: "backup_scope_reference",
                    owner: "this-process-and-operator",
                    semantics: "reference to the state inventory or scope definition used for the \
                                backup",
                },
                EvidenceFieldContract {
                    name: "storage_location",
                    owner: "external-orchestrator",
                    semantics: "off-host or object-storage destination for the retained backup",
                },
                EvidenceFieldContract {
                    name: "restore_verification_result",
                    owner: "external-orchestrator",
                    semantics: "recorded outcome of restore-side verification against the backup \
                                contract",
                },
            ],
            result_status_model: vec![
                ResultStatusContract {
                    state: "pending-capture",
                    semantics: "backup scope identified but no retained artifact recorded yet",
                },
                ResultStatusContract {
                    state: "captured",
                    semantics: "backup artifact completed and retention destination recorded",
                },
                ResultStatusContract {
                    state: "restore-verified",
                    semantics: "artifact has been checked against restore verification \
                                expectations",
                },
                ResultStatusContract {
                    state: "rejected",
                    semantics: "backup artifact or metadata failed verification and should not be \
                                used for cutover",
                },
            ],
        }
    }

    pub(super) fn readiness_report(&self) -> ReadyReport {
        let recovery_overlap_details = crate::settings::recovery_drill_overlap_details(
            &self.server_state,
            &self.recovery_staging_state,
        );
        let runtime_state_layout_conflicts =
            crate::settings::runtime_state_layout_conflict_details(&self.server_state);
        let live_identity_file_probe =
            server::settings::inspect_identity_file(&self.server_state.data_dir);
        let live_database_file_probe =
            server::persistence::inspect_database_file(&self.server_state.database_file);
        let live_settings_file_probe =
            server::settings::inspect_settings_file(&self.server_state.data_dir);
        let mut checks = vec![
            dir_check("server-data-dir", &self.server_state.data_dir, true),
            dir_check("server-config-dir", &self.server_state.config_dir, true),
            dir_check("server-ops-dir", &self.server_state.ops_dir, true),
            dir_check("server-database-dir", &self.server_state.database_dir, true),
            dir_check(
                "recovery-staging-dir",
                &self.recovery_staging_state.data_dir,
                true,
            ),
        ];
        checks.push(HealthCheck {
            name: "server-identity-file".to_owned(),
            ok: live_identity_file_probe.is_ready(),
            required: true,
            detail: live_identity_file_probe.detail(),
        });
        checks.push(HealthCheck {
            name: "server-database-file".to_owned(),
            ok: live_database_file_probe.is_ready(),
            required: true,
            detail: live_database_file_probe.detail(),
        });
        checks.push(HealthCheck {
            name: "server-settings-file".to_owned(),
            ok: live_settings_file_probe.is_ready(),
            required: self.environment != "local",
            detail: live_settings_file_probe.detail(),
        });
        checks.push(file_check(
            "server-rtsim-state-file",
            &self.server_state.rtsim_data_file,
            false,
        ));
        checks.push(dir_check(
            "server-terrain-persistence-dir",
            &self.server_state.terrain_dir,
            false,
        ));
        checks.extend(readiness_local_trail_checks(
            "audit-log",
            "operational audit trail",
            &self.server_state.audit_log_file,
            [
                &self.server_state.backup_evidence_log_file,
                &self.server_state.recovery_drill_evidence_log_file,
            ],
        ));
        checks.extend(readiness_evidence_sink_checks(
            "backup-evidence-sink",
            &self.server_state.backup_evidence_log_file,
            [
                &self.server_state.audit_log_file,
                &self.server_state.recovery_drill_evidence_log_file,
            ],
        ));
        checks.extend(readiness_evidence_sink_checks(
            "recovery-drill-evidence-sink",
            &self.server_state.recovery_drill_evidence_log_file,
            [
                &self.server_state.audit_log_file,
                &self.server_state.backup_evidence_log_file,
            ],
        ));

        checks.push(HealthCheck {
            name: "auth-runtime-policy".to_owned(),
            ok: self.auth_server_configured || self.environment == "local",
            required: self.environment != "local",
            detail: if self.auth_server_configured {
                "auth_server_address configured".to_owned()
            } else if self.environment == "local" {
                "local environment allows optional auth".to_owned()
            } else {
                "non-local environment requires auth_server_address".to_owned()
            },
        });

        checks.push(HealthCheck {
            name: "recovery-staging-layout".to_owned(),
            ok: recovery_overlap_details.is_empty(),
            required: true,
            detail: if recovery_overlap_details.is_empty() {
                format!(
                    "recovery staging layout is isolated from live state under {}",
                    self.recovery_staging_state.data_dir.display()
                )
            } else {
                recovery_overlap_details.join("; ")
            },
        });
        checks.push(HealthCheck {
            name: "runtime-state-layout".to_owned(),
            ok: runtime_state_layout_conflicts.is_empty(),
            required: true,
            detail: if runtime_state_layout_conflicts.is_empty() {
                "live runtime state layout keeps config, identity, database, rtsim, terrain, and \
                 operational trails distinct"
                    .to_owned()
            } else {
                runtime_state_layout_conflicts.join("; ")
            },
        });

        for (index, entry) in self.transport_security_inventory.iter().enumerate() {
            checks.push(HealthCheck {
                name: format!("quic-binding-{index}-tls-material"),
                ok: matches!(
                    entry.material_state,
                    crate::settings::TransportSecurityMaterialState::Valid
                ),
                required: matches!(
                    entry.validation_policy,
                    crate::settings::TransportSecurityValidationPolicy::FailFastAtStartup
                ),
                detail: format!(
                    "{} at {} uses {} validation: {}",
                    entry.surface,
                    entry.bind_address,
                    entry.validation_policy.as_str(),
                    entry.detail
                ),
            });
        }

        let ready = checks.iter().all(|check| !check.required || check.ok);
        ReadyReport {
            status: if ready { "ready" } else { "not_ready" },
            environment: self.environment,
            checks,
        }
    }
}

fn dir_check(name: impl Into<String>, path: &Path, required: bool) -> HealthCheck {
    let ok = path.is_dir();
    HealthCheck {
        name: name.into(),
        ok,
        required,
        detail: format!("expected directory at {}", path.display()),
    }
}

fn file_check(name: impl Into<String>, path: &Path, required: bool) -> HealthCheck {
    let ok = path.is_file();
    HealthCheck {
        name: name.into(),
        ok,
        required,
        detail: format!("expected file at {}", path.display()),
    }
}

fn public_entry_handoff_review_item() -> PreflightReviewItem {
    PreflightReviewItem {
        kind: "public-entry-handoff-review",
        blocking: false,
        detail: "verify the shipped Public client artifact reference plus bundled official_entry \
                 artifact identity, official_entry.server_address, official_entry.auth_server, \
                 and official_entry transport flags against the intended Public rollout target; \
                 exact-match the bundled auth pin against the authoritative handshake \
                 auth_provider from /health/compatibility; use the repo/local bundled baseline \
                 snapshot only as an advisory comparison when it is available, and still record \
                 the client-exported bundled target posture/gap reasons, target environment, \
                 compatibility generation, ready/backup/drill evidence references, rollback \
                 reference, rollback Public client artifact reference, and rollback bundled \
                 official_entry artifact identity using /health/compatibility, /health/ready, \
                 /health/backup, and /health/recovery/drill before reopening Public traffic; \
                 after terminal archive handoff completes, append post_archive verification \
                 fields on the same external review section instead of treating archive receipt \
                 alone as closure"
            .to_owned(),
    }
}

fn public_entry_handoff_preflight_review_decision_contract(
    public_entry_handoff: &PublicEntryHandoffReport,
) -> PreflightReviewDecisionContract {
    let required_decision_field_contracts = if public_entry_handoff
        .required_external_review_field_contracts
        .is_empty()
    {
        public_entry_handoff_required_review_field_contracts()
    } else {
        public_entry_handoff
            .required_external_review_field_contracts
            .clone()
    };
    let required_decision_fields = if public_entry_handoff
        .required_external_review_fields
        .is_empty()
    {
        external_record_field_names(&required_decision_field_contracts)
    } else {
        public_entry_handoff.required_external_review_fields.clone()
    };
    let exception_record_field_contracts = public_entry_handoff_exception_field_contracts();
    let public_entry_transition_contract = public_entry_handoff
        .public_entry_transition_contract
        .clone()
        .or_else(|| Some(public_entry_transition_contract()));
    let public_entry_lifecycle_transition_contract = public_entry_handoff
        .public_entry_lifecycle_transition_contract
        .clone()
        .or_else(|| Some(public_entry_lifecycle_transition_contract()));
    let section_instance_validation_contract = public_entry_handoff
        .section_instance_validation_contract
        .clone()
        .or_else(|| {
            Some(public_entry_section_instance_validation_contract(
                &required_decision_field_contracts,
            ))
        });
    let required_authority_pairing_checks = if public_entry_handoff
        .required_authority_pairing_checks
        .is_empty()
    {
        public_entry_authority_pairing_checks()
    } else {
        public_entry_handoff
            .required_authority_pairing_checks
            .clone()
    };
    let terminal_states_requiring_archive_receipt = public_entry_lifecycle_transition_contract
        .as_ref()
        .map(|contract| contract.terminal_states_requiring_archive_receipt.clone())
        .unwrap_or_default();
    let archive_handoff_contract = release_review_record_archive_handoff_contract(
        "public-entry-handoff",
        "release-review-record reached a terminal Public cutover decision state with the bundled \
         artifact review, required evidence references, and rollback path recorded where \
         applicable",
        terminal_states_requiring_archive_receipt.clone(),
        terminal_states_requiring_archive_receipt,
    );
    let post_archive_writeback_fields = release_review_post_archive_writeback_field_names();
    let section_record_template = release_review_section_template_contract(
        "public-entry-handoff",
        "draft",
        &required_decision_field_contracts,
        &exception_record_field_contracts,
        &archive_handoff_contract,
        &post_archive_writeback_fields,
        vec![
            "keep this section inside the same release-review-record keyed by release_reference",
            "replace illustrative placeholders with rollout-specific truth before approving \
             cutover",
            "do not treat this template as a second authority source; it is only the minimum \
             schema to populate in the external release tracker",
        ],
    );
    let minimum_section_example = release_review_section_example_contract(
        "public-entry-handoff",
        "cutover-approved",
        &required_decision_field_contracts,
        vec![
            "illustrative only; prod.realm.example and auth.realm.example are reserved example \
             hostnames, not real rollout values",
            "use the shipped bundle review, /health/compatibility, /health/ready, /health/backup, \
             and /health/recovery/drill to replace every example field with rollout-specific truth",
        ],
    );
    let section_execution_workflow =
        release_review_section_execution_workflow("public-entry-handoff");
    let terminal_mutation_contract = release_review_terminal_mutation_contract(
        "public-entry-handoff",
        &archive_handoff_contract,
        &post_archive_writeback_fields,
    );
    let execution_boundary_contract = release_review_record_execution_boundary_contract(
        "public-entry-handoff",
        &required_decision_field_contracts,
        &exception_record_field_contracts,
        &archive_handoff_contract,
    );
    let validator_integration_readiness_summary = section_instance_validation_contract
        .as_ref()
        .map(validator_integration_readiness_summary);

    PreflightReviewDecisionContract {
        signal: "public-entry-handoff",
        review_owner: "release-operator",
        decision_scope: "bundled Public official entry server/auth pin handoff for non-local \
                         rollout",
        required_decision_fields,
        required_decision_field_contracts: required_decision_field_contracts.clone(),
        exception_record_fields: external_record_field_names(&exception_record_field_contracts),
        exception_record_field_contracts: exception_record_field_contracts.clone(),
        record_lifecycle_contract: release_review_record_lifecycle_contract(
            &required_decision_field_contracts,
        ),
        archive_handoff_contract,
        retention_contract: release_review_record_retention_contract(
            "public-entry-handoff",
            &post_archive_writeback_fields,
        ),
        terminal_mutation_contract,
        public_entry_transition_contract,
        public_entry_lifecycle_transition_contract,
        section_instance_validation_contract,
        validator_integration_readiness_summary,
        authority_pairing_checks: required_authority_pairing_checks,
        execution_boundary_contract,
        result_status_model: public_entry_handoff_result_status_model(),
        section_record_template,
        minimum_section_example,
        section_execution_workflow,
        accepted_exception_follow_up: vec![
            "public-entry-handoff currently does not support exception-accepted as a valid \
             terminal/archive lifecycle; keep exception records informational only until a \
             dedicated exception path is formalized",
        ],
        external_record_owner: "release-operator",
        external_record_authority: "external-release-tracker",
        decision_reference_kind: "release-review-record",
        exception_reference_kind: "public-entry-handoff-exception-record",
        local_contract_role: "minimum-schema-only",
        supporting_endpoints: public_entry_handoff_supporting_endpoints(public_entry_handoff),
    }
}

fn public_entry_handoff_supporting_endpoints(
    public_entry_handoff: &PublicEntryHandoffReport,
) -> Vec<PreflightSupportingEndpoint> {
    public_entry_handoff
        .supporting_health_endpoints
        .iter()
        .map(|endpoint| match *endpoint {
            "/health/compatibility" => PreflightSupportingEndpoint {
                signal: "compatibility-contract",
                endpoint,
                owner: "release-operator",
                purpose: "compare authoritative handshake auth mode, target environment, \
                          compatibility generation, and query auth_required hint with the \
                          external bundled official_entry review record",
                related_findings: Vec::new(),
            },
            "/health/ready" => PreflightSupportingEndpoint {
                signal: "readiness",
                endpoint,
                owner: "release-operator",
                purpose: "record the observed ready_report_status before approving non-local \
                          Public cutover",
                related_findings: Vec::new(),
            },
            "/health/backup" => PreflightSupportingEndpoint {
                signal: "backup-preflight",
                endpoint,
                owner: "release-operator",
                purpose: "link current backup evidence before approving non-local Public cutover",
                related_findings: Vec::new(),
            },
            "/health/recovery/drill" => PreflightSupportingEndpoint {
                signal: "recovery-drill",
                endpoint,
                owner: "release-operator",
                purpose: "link ready-validated recovery drill evidence and rollback posture \
                          before approving non-local Public cutover",
                related_findings: Vec::new(),
            },
            _ => PreflightSupportingEndpoint {
                signal: "public-entry-supporting-contract",
                endpoint,
                owner: "release-operator",
                purpose: "consume the supporting health contract required for external Public \
                          cutover review",
                related_findings: Vec::new(),
            },
        })
        .collect()
}

fn governance_preflight_review_decision_contract(
    supporting_endpoints: Vec<PreflightSupportingEndpoint>,
) -> PreflightReviewDecisionContract {
    let required_decision_field_contracts = governance_required_decision_field_contracts();
    let exception_record_field_contracts = governance_exception_field_contracts();
    let archive_handoff_contract = release_review_record_archive_handoff_contract(
        "governance-audit",
        "release-review-record reached a terminal governance review state with findings, \
         decision, and rollback path recorded where applicable",
        vec!["approved", "exception-accepted", "rejected", "rolled-back"],
        vec!["approved", "exception-accepted", "rejected", "rolled-back"],
    );
    let post_archive_writeback_fields = release_review_post_archive_writeback_field_names();
    let section_record_template = release_review_section_template_contract(
        "governance-audit",
        "draft",
        &required_decision_field_contracts,
        &exception_record_field_contracts,
        &archive_handoff_contract,
        &post_archive_writeback_fields,
        vec![
            "keep governance review in the same release-review-record as public-entry-handoff and \
             management-auth for the rollout unit",
            "exception fields are only required when governance exceptions are accepted",
        ],
    );
    let minimum_section_example = release_review_section_example_contract(
        "governance-audit",
        "approved",
        &required_decision_field_contracts,
        vec![
            "illustrative only; replace the example release reference and operator id with real \
             rollout data",
        ],
    );
    let section_execution_workflow = release_review_section_execution_workflow("governance-audit");
    let terminal_mutation_contract = release_review_terminal_mutation_contract(
        "governance-audit",
        &archive_handoff_contract,
        &post_archive_writeback_fields,
    );
    let execution_boundary_contract = release_review_record_execution_boundary_contract(
        "governance-audit",
        &required_decision_field_contracts,
        &exception_record_field_contracts,
        &archive_handoff_contract,
    );
    let section_instance_validation_contract =
        governance_section_instance_validation_contract(&required_decision_field_contracts);
    let validator_integration_readiness_summary =
        validator_integration_readiness_summary(&section_instance_validation_contract);

    PreflightReviewDecisionContract {
        signal: "governance-audit",
        review_owner: "release-operator",
        decision_scope: "accepted runtime governance findings and explicit risk posture exceptions",
        required_decision_fields: external_record_field_names(&required_decision_field_contracts),
        required_decision_field_contracts: required_decision_field_contracts.clone(),
        exception_record_fields: external_record_field_names(&exception_record_field_contracts),
        exception_record_field_contracts,
        record_lifecycle_contract: release_review_record_lifecycle_contract(
            &required_decision_field_contracts,
        ),
        archive_handoff_contract,
        retention_contract: release_review_record_retention_contract(
            "governance-audit",
            &post_archive_writeback_fields,
        ),
        terminal_mutation_contract,
        public_entry_transition_contract: None,
        public_entry_lifecycle_transition_contract: None,
        section_instance_validation_contract: Some(section_instance_validation_contract),
        validator_integration_readiness_summary: Some(validator_integration_readiness_summary),
        authority_pairing_checks: Vec::new(),
        execution_boundary_contract,
        result_status_model: governance_review_result_status_model(),
        section_record_template,
        minimum_section_example,
        section_execution_workflow,
        accepted_exception_follow_up: vec![
            "link governance finding review notes before reopening traffic",
            "record rollback references for any accepted governance exception",
        ],
        external_record_owner: "release-operator",
        external_record_authority: "external-release-tracker",
        decision_reference_kind: "release-review-record",
        exception_reference_kind: "governance-exception-record",
        local_contract_role: "minimum-schema-only",
        supporting_endpoints,
    }
}

fn management_auth_preflight_review_decision_contract() -> PreflightReviewDecisionContract {
    let required_decision_field_contracts = management_auth_required_decision_field_contracts();
    let exception_record_field_contracts = management_auth_exception_field_contracts();
    let archive_handoff_contract = release_review_record_archive_handoff_contract(
        "management-auth",
        "release-review-record reached a terminal management auth review state with exposure \
         findings, decision, and rollback path recorded where applicable",
        vec!["approved", "exception-accepted", "rejected", "rolled-back"],
        vec!["approved", "exception-accepted", "rejected", "rolled-back"],
    );
    let post_archive_writeback_fields = release_review_post_archive_writeback_field_names();
    let section_record_template = release_review_section_template_contract(
        "management-auth",
        "draft",
        &required_decision_field_contracts,
        &exception_record_field_contracts,
        &archive_handoff_contract,
        &post_archive_writeback_fields,
        vec![
            "keep management-auth review in the same release-review-record as the rest of the \
             rollout unit",
            "exception fields are only required when remote exposure is accepted with \
             compensating controls",
        ],
    );
    let minimum_section_example = release_review_section_example_contract(
        "management-auth",
        "approved",
        &required_decision_field_contracts,
        vec![
            "illustrative only; replace example values with the real management surface review \
             for the rollout unit",
        ],
    );
    let section_execution_workflow = release_review_section_execution_workflow("management-auth");
    let terminal_mutation_contract = release_review_terminal_mutation_contract(
        "management-auth",
        &archive_handoff_contract,
        &post_archive_writeback_fields,
    );
    let execution_boundary_contract = release_review_record_execution_boundary_contract(
        "management-auth",
        &required_decision_field_contracts,
        &exception_record_field_contracts,
        &archive_handoff_contract,
    );
    let section_instance_validation_contract =
        management_auth_section_instance_validation_contract(&required_decision_field_contracts);
    let validator_integration_readiness_summary =
        validator_integration_readiness_summary(&section_instance_validation_contract);

    PreflightReviewDecisionContract {
        signal: "management-auth",
        review_owner: "release-operator",
        decision_scope: "remote management and observability auth posture for unaudited control \
                         or unauthenticated observability surfaces",
        required_decision_fields: external_record_field_names(&required_decision_field_contracts),
        required_decision_field_contracts: required_decision_field_contracts.clone(),
        exception_record_fields: external_record_field_names(&exception_record_field_contracts),
        exception_record_field_contracts,
        record_lifecycle_contract: release_review_record_lifecycle_contract(
            &required_decision_field_contracts,
        ),
        archive_handoff_contract,
        retention_contract: release_review_record_retention_contract(
            "management-auth",
            &post_archive_writeback_fields,
        ),
        terminal_mutation_contract,
        public_entry_transition_contract: None,
        public_entry_lifecycle_transition_contract: None,
        section_instance_validation_contract: Some(section_instance_validation_contract),
        validator_integration_readiness_summary: Some(validator_integration_readiness_summary),
        authority_pairing_checks: Vec::new(),
        execution_boundary_contract,
        result_status_model: management_auth_review_result_status_model(),
        section_record_template,
        minimum_section_example,
        section_execution_workflow,
        accepted_exception_follow_up: vec![
            "record which management or observability surfaces remain remotely exposed",
            "link compensating control notes before reopening traffic",
        ],
        external_record_owner: "release-operator",
        external_record_authority: "external-release-tracker",
        decision_reference_kind: "release-review-record",
        exception_reference_kind: "management-auth-exception-record",
        local_contract_role: "minimum-schema-only",
        supporting_endpoints: Vec::new(),
    }
}

fn validator_integration_readiness_summary(
    validation_contract: &ExternalSectionInstanceValidationContract,
) -> SectionValidatorIntegrationReadinessSummary {
    SectionValidatorIntegrationReadinessSummary {
        status: "validator-contract-ready",
        input_snapshot_kind: validation_contract.snapshot_input_contract.snapshot_kind,
        field_values_key: validation_contract.snapshot_input_contract.field_values_key,
        lifecycle_state_field: validation_contract.lifecycle_state_field,
        output_result_kind: validation_contract.validation_result_contract.result_kind,
        output_stage_status_field: validation_contract
            .validation_result_contract
            .stage_status_field,
        blocking_interpretation: validation_contract.blocking_interpretation,
    }
}

fn public_entry_external_execution_dependencies(
    applies_to_non_local_public_rollout: bool,
    release_blocked: bool,
    authoritative_auth_mode: common_net::msg::ServerAuthMode,
    authoritative_auth_provider: Option<&str>,
) -> Vec<ExternalExecutionDependencyReport> {
    if !applies_to_non_local_public_rollout {
        return Vec::new();
    }

    vec![
        ExternalExecutionDependencyReport {
            id: "external-auth-runtime-authority",
            owner: "service-operator",
            blocks_development_stage_closure: false,
            blocks_real_cutover: true,
            current_stage_status: if release_blocked {
                "runtime-posture-unsupported"
            } else {
                "runtime-authority-exported"
            },
            detail: if release_blocked {
                format!(
                    "the authoritative handshake currently exposes {} instead of a supported \
                     external-auth posture, so the first non-local Public cutover cannot be \
                     executed yet even though the development-stage handoff contract is already \
                     available",
                    authoritative_auth_mode.as_str()
                )
            } else {
                format!(
                    "the authoritative handshake currently exports external auth authority{} in \
                     /health/compatibility, but the first real non-local Public cutover still \
                     requires the shipped bundled auth pin and external release review record to \
                     exact-match that runtime authority",
                    authoritative_auth_provider
                        .map(|provider| format!(" ({provider})"))
                        .unwrap_or_default()
                )
            },
            operator_next_step: if release_blocked {
                "configure a supported external auth authority before attempting the first \
                 non-local Public cutover"
            } else {
                "record the current auth authority in the external release review and exact-match \
                 it against the shipped bundled auth pin"
            },
            supporting_endpoints: vec!["/health/compatibility"],
        },
        ExternalExecutionDependencyReport {
            id: "shipped-public-artifacts-and-rollback-material",
            owner: "release-operator",
            blocks_development_stage_closure: false,
            blocks_real_cutover: true,
            current_stage_status: "external-material-required",
            detail: "the real shipped Public client artifact, rollback client artifact, bundled \
                     official_entry material, and rollout-specific rollback path are not created \
                     or frozen by this local process, so module C can close its development-stage \
                     contract without them but cannot claim a real non-local Public cutover"
                .to_owned(),
            operator_next_step: "freeze the forward and rollback client artifacts, review the \
                                 bundled official_entry payload, and attach the rollout-specific \
                                 references to the external release review record",
            supporting_endpoints: vec![
                "/health/compatibility",
                "/health/ready",
                "/health/backup",
                "/health/recovery/drill",
            ],
        },
        ExternalExecutionDependencyReport {
            id: "external-release-review-and-archive-execution",
            owner: "external-release-tracker",
            blocks_development_stage_closure: false,
            blocks_real_cutover: true,
            current_stage_status: "external-execution-required",
            detail: "the repo now exports the minimum section schema, workflow, archive handoff, \
                     and post-archive writeback contracts, but the real release-review instance, \
                     terminal archive handoff, retention, and post-archive verification still \
                     happen outside this repository and process"
                .to_owned(),
            operator_next_step: "populate a real release-review-record instance externally, drive \
                                 it through terminal archive handoff, and write back the required \
                                 archive receipt and post-archive verification fields",
            supporting_endpoints: vec!["/health/compatibility", "/health/preflight"],
        },
    ]
}

fn runtime_auth_mode(authoritative_auth_provider: Option<&str>) -> common_net::msg::ServerAuthMode {
    if authoritative_auth_provider.is_some() {
        common_net::msg::ServerAuthMode::ExternalProvider
    } else {
        common_net::msg::ServerAuthMode::NoExternalAuth
    }
}

fn query_server_environment_str(
    environment: veloren_query_server::proto::ServerEnvironment,
) -> &'static str {
    match environment {
        veloren_query_server::proto::ServerEnvironment::Local => "local",
        veloren_query_server::proto::ServerEnvironment::Test => "test",
        veloren_query_server::proto::ServerEnvironment::Production => "production",
    }
}

fn path_exists(path: &Path) -> bool { path.is_file() || path.is_dir() }

fn readiness_local_trail_checks<I, P>(
    trail_name: &'static str,
    trail_role: &'static str,
    trail_path: &Path,
    conflict_paths: I,
) -> Vec<HealthCheck>
where
    I: IntoIterator<Item = P>,
    P: AsRef<Path>,
{
    let conflicts = conflict_paths
        .into_iter()
        .map(|path| path.as_ref().to_path_buf())
        .filter(|path| normalized_paths_equal(trail_path, path))
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>();

    vec![
        HealthCheck {
            name: format!("{trail_name}-target-file"),
            ok: !trail_path.exists() || trail_path.is_file(),
            required: false,
            detail: format!(
                "advisory: expected {trail_role} target at {} to be absent or a regular file",
                trail_path.display()
            ),
        },
        HealthCheck {
            name: format!("{trail_name}-path-conflict"),
            ok: conflicts.is_empty(),
            required: false,
            detail: if conflicts.is_empty() {
                format!(
                    "advisory: {trail_role} path {} is distinct from other local operational \
                     trails",
                    trail_path.display()
                )
            } else {
                format!(
                    "advisory: {trail_role} path {} conflicts with {}",
                    trail_path.display(),
                    conflicts.join(", ")
                )
            },
        },
    ]
}

fn readiness_evidence_sink_checks<I, P>(
    sink_name: &'static str,
    sink_path: &Path,
    conflict_paths: I,
) -> Vec<HealthCheck>
where
    I: IntoIterator<Item = P>,
    P: AsRef<Path>,
{
    let parent = sink_path.parent().unwrap_or_else(|| Path::new("."));
    let conflicts = conflict_paths
        .into_iter()
        .map(|path| path.as_ref().to_path_buf())
        .filter(|path| normalized_paths_equal(sink_path, path))
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>();

    vec![
        HealthCheck {
            name: format!("{sink_name}-parent-dir"),
            ok: parent.is_dir(),
            required: false,
            detail: format!(
                "advisory: expected evidence sink parent directory at {}",
                parent.display()
            ),
        },
        HealthCheck {
            name: format!("{sink_name}-target-file"),
            ok: !sink_path.exists() || sink_path.is_file(),
            required: false,
            detail: format!(
                "advisory: expected evidence sink target at {} to be absent or a regular file",
                sink_path.display()
            ),
        },
        HealthCheck {
            name: format!("{sink_name}-path-conflict"),
            ok: conflicts.is_empty(),
            required: false,
            detail: if conflicts.is_empty() {
                format!(
                    "advisory: evidence sink path {} is distinct from other operational trails",
                    sink_path.display()
                )
            } else {
                format!(
                    "advisory: evidence sink path {} conflicts with {}",
                    sink_path.display(),
                    conflicts.join(", ")
                )
            },
        },
    ]
}

fn evidence_sink_checks<I, P>(
    sink_name: &'static str,
    sink_path: &Path,
    conflict_paths: I,
) -> Vec<HealthCheck>
where
    I: IntoIterator<Item = P>,
    P: AsRef<Path>,
{
    let mut checks = Vec::new();
    let parent = sink_path.parent().unwrap_or_else(|| Path::new("."));
    checks.push(HealthCheck {
        name: format!("{sink_name}-parent-dir"),
        ok: parent.is_dir(),
        required: true,
        detail: format!(
            "expected evidence sink parent directory at {}",
            parent.display()
        ),
    });
    checks.push(HealthCheck {
        name: format!("{sink_name}-target-file"),
        ok: !sink_path.exists() || sink_path.is_file(),
        required: true,
        detail: format!(
            "expected evidence sink target at {} to be absent or a regular file",
            sink_path.display()
        ),
    });

    let conflicts = conflict_paths
        .into_iter()
        .map(|path| path.as_ref().to_path_buf())
        .filter(|path| normalized_paths_equal(sink_path, path))
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>();
    checks.push(HealthCheck {
        name: format!("{sink_name}-path-conflict"),
        ok: conflicts.is_empty(),
        required: true,
        detail: if conflicts.is_empty() {
            format!(
                "evidence sink path {} is distinct from other operational trails",
                sink_path.display()
            )
        } else {
            format!(
                "evidence sink path {} conflicts with {}",
                sink_path.display(),
                conflicts.join(", ")
            )
        },
    });
    checks
}

fn normalized_paths_equal(left: &Path, right: &Path) -> bool {
    normalize_path(left) == normalize_path(right)
}

fn normalize_path(path: &Path) -> std::path::PathBuf {
    use std::path::Component;

    let mut normalized = std::path::PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {},
            Component::ParentDir => {
                let _ = normalized.pop();
            },
            Component::Normal(part) => normalized.push(part),
        }
    }
    normalized
}

fn audit_archive_pattern(path: &Path) -> String {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("audit-log");
    let extension = path.extension().and_then(|extension| extension.to_str());

    match extension {
        Some(extension) => parent
            .join(format!("{stem}.<timestamp>.{extension}"))
            .display()
            .to_string(),
        None => parent
            .join(format!("{stem}.<timestamp>"))
            .display()
            .to_string(),
    }
}

#[cfg(test)] mod tests;
