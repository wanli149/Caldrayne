# World-Compat Rehearsal Fill Guide

Use this guide before replacing any sample values.

Boundary:

- This guide does not define a new schema.
- It only explains how to turn the current rehearsal samples into a simulated external
  execution pack without pretending they are real production evidence.

## 1. Live-Copy Fields

The following fields must be copied verbatim from the current `/health/world-compat`
 response at the time of review:

- `world_compat_status`
- `configured_mode`
- `load_legacy_mode`
- `load_or_generate_sidecarless_mode`
- `compat_entry`
- `compat_decision`
- `compat_failure`
- `strict_load_contract_gap`
- `managed_recipe_sidecar_missing`
- `world_recipe_hash`
- `chunk_recipe_hash`
- `topology_id`
- `preset_id`

Rules:

- Do not reuse values from a superseded terminal.
- Do not normalize or rename the string values.
- If current runtime posture is `allow`, copy `allow`; do not silently preserve an older
  `deny/deny` sample shape.

## 2. Execution Placeholders

The following fields remain placeholders until a real external execution happens:

- `release_reference`
- `reviewed_by`
- `decision_recorded_at_utc`
- `rollback_reference`
- `archive_reference`
- `post_archive_verification_reference`

Rules:

- Replace every placeholder with rollout-specific truth before using the pack in a live
  operator rehearsal.
- Do not treat sample references as real evidence.
- `rollback_reference` must point to the rollback path for the current follow-up, not a
  reused historical note from another follow-up.

## 3. Same-Section Checks

These checks must hold for both follow-up sample packs:

- same `record_kind = release-review-record`
- same `section_signal = world-compat`
- same `release_reference` within one follow-up history chain
- `prior_result_statuses` proves only the direct-prior superseded terminal
- `archive_reference` and `post_archive_verification_reference` point to the same direct-prior superseded terminal
- `source_record_state == result_status`

Rules:

- Never mix materials from another section.
- Never use an older non-direct-prior terminal as the history proof.
- Never rewrite the archived terminal in place; the workflow is append-only.

## 4. Stage 1 Freeze Checks

These checks must hold for each `T1/T2` cycle:

- `T1` and `T2` share the same `release_reference`
- `world_compat_status = world-compat-clear`
- `load_legacy_mode = deny`
- `load_or_generate_sidecarless_mode = deny`
- `managed_recipe_sidecar_missing = false`
- `transition_window_open = false`

Rules:

- If any of the above drifts before `T2`, the cycle is not freeze-ready.
- `Cycle N` and `Cycle N+1` must use different `release_reference` values.
- The packet shape must remain homologous across cycles.

## 5. Completion Check

Before calling the rehearsal pack complete, confirm:

1. Every live-copy field is marked and treated as runtime truth.
2. Every placeholder field is marked and treated as external execution input.
3. Every follow-up sample stays same-record, same-section, append-only.
4. Every Stage 1 packet stays same-release for `T1/T2` and homologous across `Cycle N/N+1`.
