# World-Compat Rehearsal Execution Pack Index

This file ties the existing rehearsal samples into one simulated but complete external
execution pack.

Boundary:

- This is a rehearsal index, not an authoritative execution record.
- Every referenced file remains `sample` / `rehearsal-only`.
- Real execution must still happen in the external release tracker.

## 1. Follow-Up Pack

Use these files when rehearsing same-section append-only follow-up history on one
`release-review-record`.

### 1.1 Accepted Window Closure

Workflow intent:

- close a previously archived `exception-accepted` window
- append an explicit `approved` follow-up on the same `world-compat` section

Files:

- [README.md](</j:/Caldrayne/release-review/world-compat-rehearsal/README.md>)
- [fill-guide.md](</j:/Caldrayne/release-review/world-compat-rehearsal/fill-guide.md>)
- [world-compat-follow-up-exception-accepted-to-approved.sample.txt](</j:/Caldrayne/release-review/world-compat-rehearsal/follow-up/world-compat-follow-up-exception-accepted-to-approved.sample.txt>)
- [world-compat-follow-up-bundle.sample.txt](</j:/Caldrayne/release-review/world-compat-rehearsal/follow-up/world-compat-follow-up-bundle.sample.txt>)

Execution checklist:

1. Copy current `/health/world-compat` truth into the current terminal snapshot fields.
2. Replace rollout-specific placeholders such as `release_reference`, operator identity, and archive/runbook references.
3. Confirm the superseded terminal is the direct-prior `exception-accepted` state on the same section.
4. Confirm `source_record_state = approved` and `prior_result_statuses = ["exception-accepted"]`.

### 1.2 Approved Reopen / Rollback

Workflow intent:

- append an explicit `rolled-back` follow-up on the same `world-compat` section
- prove that the superseded direct-prior terminal was `approved`

Files:

- [README.md](</j:/Caldrayne/release-review/world-compat-rehearsal/README.md>)
- [fill-guide.md](</j:/Caldrayne/release-review/world-compat-rehearsal/fill-guide.md>)
- [world-compat-follow-up-approved-to-rolled-back.sample.txt](</j:/Caldrayne/release-review/world-compat-rehearsal/follow-up/world-compat-follow-up-approved-to-rolled-back.sample.txt>)
- [world-compat-follow-up-bundle.sample.txt](</j:/Caldrayne/release-review/world-compat-rehearsal/follow-up/world-compat-follow-up-bundle.sample.txt>)

Execution checklist:

1. Copy current `/health/world-compat` truth into the current terminal snapshot fields.
2. Replace rollout-specific placeholders such as `release_reference`, operator identity, and archive/runbook references.
3. Confirm the superseded terminal is the direct-prior `approved` state on the same section.
4. Confirm `source_record_state = rolled-back` and `prior_result_statuses = ["approved"]`.

## 2. Stage 1 Exception-Freeze Pack

Use these files when rehearsing one dedicated `Stage 1` exception-freeze observation
window, then a second cycle for `Cycle N+1`.

Files:

- [README.md](</j:/Caldrayne/release-review/world-compat-rehearsal/README.md>)
- [fill-guide.md](</j:/Caldrayne/release-review/world-compat-rehearsal/fill-guide.md>)
- [cycle-overview.md](</j:/Caldrayne/release-review/world-compat-rehearsal/stage1-exception-freeze/cycle-overview.md>)
- [rel-2026-05-world-compat-01](</j:/Caldrayne/release-review/world-compat-rehearsal/stage1-exception-freeze/rel-2026-05-world-compat-01>)
- [rel-2026-05-world-compat-02](</j:/Caldrayne/release-review/world-compat-rehearsal/stage1-exception-freeze/rel-2026-05-world-compat-02>)

Execution checklist:

1. Rehearse `T1` and `T2` within the same `release_reference`.
2. Confirm both `T1` and `T2` remain `deny / deny + clear` and `transition_window_open = false`.
3. Confirm the archived terminal, archive receipt, post-archive verification, and rollback reference all point to one direct-prior terminal on the same section.
4. Repeat the same packet shape on a second `release_reference` for `Cycle N+1`.

## 3. Completion Boundary

This pack is complete enough for repo-side operator rehearsal when all three are true:

- the follow-up samples are readable as one same-section append-only workflow
- the Stage 1 packet is readable as one same-release `T1/T2` cycle plus one homologous second cycle
- the fill guide is sufficient to prevent sample values from being mistaken for live runtime or authoritative external record values

Even after this pack is complete, the following are still pending outside the repo:

- real authoritative external `release-review-record`
- real archive receipt and post-archive verification
- real dedicated `Stage 1` freeze execution evidence
- real dedicated `Stage 2` removal execution evidence
