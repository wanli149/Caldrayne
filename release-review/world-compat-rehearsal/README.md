# World-Compat Rehearsal Samples

This directory contains `sample` / `rehearsal` assets only.

These files are not authoritative production evidence. They are not:

- the real external `release-review-record`
- the real archive receipt
- the real post-archive verification record
- the real rollback execution evidence

Authority remains outside this repository. The authoritative owner is the external
release tracker and the operators who execute the rollout.

Usage boundary:

- Treat every value here as illustrative unless the file explicitly says it is copied
  from a live runtime sample.
- Real execution must copy current `/health/world-compat` truth verbatim at the time
  of review.
- Real follow-up history must stay on the same `release-review-record`, the same
  `world-compat` section, and remain append-only.
- Local repo files must not be used as a substitute for the authoritative external
  review record or its archive copy.

Directory layout:

- `follow-up/`
  External `world-compat` follow-up rehearsal samples.
- `stage1-exception-freeze/`
  Dedicated `Stage 1` exception-freeze rehearsal evidence packs.
- `execution-pack.index.md`
  One-page pack index that maps each sample file to its intended external workflow step.
- `fill-guide.md`
  Field-by-field replacement rules: what must be live-copied, what stays placeholder until real execution, and what must remain same release-reference / same section.
- `stage1-exception-freeze/cycle-overview.md`
  Cycle-level view for `T1/T2` and `Cycle N/N+1`, used to rehearse the freeze packet in sequence instead of as isolated files.
