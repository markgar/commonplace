---
name: commonplace-progress
description: Track Commonplace implementation packets and coordinate build handoffs. Use when starting or resuming a packet, reporting build progress, updating blockers or review readiness, handing work to another session, or verifying integration.
---

# Commonplace build progress

Use this repo-local workflow for `/commonplace-progress`, a packet kickoff, or a
progress/handoff request. It is a procedure, not a background monitor. Do not
create automations, new sessions, issues, or PRs merely to report status.

## Sources of truth

Paths below are relative to the repository root.

1. Read `docs/specification/README.md` first and follow its ownership hierarchy.
2. Read the packet in `docs/specification/implementation.md`, including its
   dependencies, relevant owning sections, and delivery rules. Follow its issue
   link; do not hardcode another issue map in this skill.
3. Read that issue and its linked PR before editing progress. Specifications own
   scope and acceptance; the issue owns live progress; Git/PRs prove integration.
   Spike reports provide evidence, not replacement contracts.
4. Read `.github/copilot-instructions.md` and `README.md` for current repository
   rules and applicable local checks.

A specification update not yet merged is not visible to a fresh default-branch
session. Name the exact approved revision in the handoff or wait for integration;
never silently let the child implement an older contract.

## Start or resume work

- Reuse the existing packet issue and owning session. Discover before creating
  anything; do not duplicate work because a session is quiet.
- Check prerequisite issues and merge evidence. A completed local gate is not
  proof that its evidence or a prerequisite implementation has been integrated.
  A closed issue or merged intermediate PR alone is insufficient.
- Claim the packet's `Owner/session` with session name and branch, then set
  `State` to `Building`. Use a resolvable app URL only if returned by the app,
  never a constructed URL. Do not assign a GitHub user without authorization.
- Record the concrete next action and relevant blocker. Coordinate shared files
  with the other lane. Keep one accountable owner for each packet.
- If a prerequisite is missing, report the exact missing integration. Do not
  silently start a stacked branch or substitute another backend.

Every child kickoff must include: packet issue, specification index and relevant
sections, approved revision/base, dependencies, owned surfaces, smallest complete
acceptance outcome, local-only checks, approval boundaries, and report-back
instructions. Every child must read those sources, not rely only on the prompt.

## Maintain the issue, not another tracker

Keep the issue body's `Progress` table current:

| Field | Content |
| --- | --- |
| State | Not started / Building / Ready for review / Blocked / Integrated |
| Owner/session | Accountable session and branch, or Unassigned |
| Dependencies | Linked prerequisite issues and any named gate |
| Blocker | Concrete cause and unblock action, or None |
| PR | Current PR link, or explicitly not opened |
| Evidence | Exact commit, relevant commands/results, or report/artifact location |
| Next action | One concrete next step |
| Updated | Date of the verified update |

Update at starts, material blockers/decisions, review handoff, and integration.
Do not post per-tool narration or speculative completion percentages. Re-read
the issue before editing; preserve human notes and concurrent updates. Use a
short comment for a durable decision or handoff, not as a replacement for current
state in the body. If GitHub is unavailable, report the unsynced update plainly;
do not claim it was saved or create a second canonical local ledger.

Ordinary prerequisite waiting is `Not started`. Use `Blocked` when progress
requires a specific unblock action and name who or what can provide it. After
unblocking, return to the appropriate state; do not infer completion.

`Ready for review` means a complete reviewable diff and the applicable local
acceptance evidence are available. Record whether it is committed and whether
a PR exists. It does not mean merged. An idle agent or passing isolated test
suite is not independently proof of readiness.

## Verify integration and hand off

Before setting `Integrated`, verify all of:

- The current PR is actually merged into the intended integration base, normally
  main. Record its merge commit; a child-stack merge is not main integration.
- The whole packet's acceptance is satisfied in the combined code, not merely
  in an earlier isolated branch. A cancelled or partially completed packet is
  not integrated.
- Applicable checks/evidence are recorded; documentation-only work does not
  require a build unless documentation checks exist.

Only then update the issue to `Integrated` and close it. Never merge merely to
advance a status. If auto-close happened before these conditions, do not treat
it as proof; flag and correct the premature tracking state with the owner.

For P6/P8, the later packet to integrate owns combined fact/removal coverage.
Record the evidence in that issue and link it from the other; P10 requires it.
Track target execution/packaging in P10 independently of feature integration.
Local spike PASS, cross-compilation, and prior-engine evidence do not prove the
selected production runtime works on an untested target.

Handoffs include issue/PR, commit, evidence, remaining work/blockers, and next
action. Do not copy or reinterpret the product contract into the issue.

## Coordinator status report

Read linked issues and current PR states. If app tools are available, refresh
`get_sessions_status` for live activity and human-gated prompts. Busy without a
waiting prompt is not a stall; idle is not integration. Without app access,
report activity as unknown instead of guessing.

Report only newly integrated work, active work, blockers/needed approvals, and
next eligible packets. Use exact app-provided links for sessions. The dependency
map stays in the implementation spec; issue state stays in GitHub.

## Boundaries

Run development checks locally. Do not enable/dispatch Actions, use paid remote
runners, broaden scope, change public/durable contracts, merge, or release
without the applicable explicit approval. Do not rerun completed feasibility
spikes merely to update progress. Use real SQLite/Oxigraph where their behavior
is tested and deterministic substitutes only for expensive inference, following
the owning specification.
