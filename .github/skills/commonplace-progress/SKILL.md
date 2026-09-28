---
name: commonplace-progress
description: Plan and coordinate Commonplace implementation packets, parallel builds, and subsessions. Use for implementation planning, kicking off or resuming packets, orchestrating child sessions, independent rubber-duck plan review, approving plans, delegated implementation review, reporting progress, updating blockers, review handoffs, and verifying integration.
---

# Commonplace packet planning and coordination

Use this repo-local workflow for `/commonplace-progress`, implementation planning,
parallel packet or subsession work, plan approval, and progress/handoff requests.
It complements the generic `orchestrate` skill: that skill handles session
mechanics; this one owns the repository's packet workflow. If the client has not
discovered this skill, read this file directly before proceeding. It is a
procedure, not a background monitor. Do not create automations, new sessions,
issues, or PRs merely to report status.

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
acceptance outcome, local-only checks, isolated build-output directory, approval
boundaries, the review gates below, and report-back instructions.
Every child must read those sources, not rely only on the prompt.

## Isolate build output per worktree

- Every session builds and runs tests using its own worktree-local `target/`.
  Leave `CARGO_TARGET_DIR` unset or set it to that worktree's absolute `target`
  path. Never point multiple sessions at the same target directory.
- Separate source worktrees do not isolate shared build output. Cargo build locks
  do not protect a test run from another session replacing its executable.
  Do not share mutable executables or hardlink/symlink mutable build artifacts
  across sessions, or serialize otherwise independent work to compensate for sharing.
- Read-only pinned model artifacts and normal package-manager caches may be
  shared. Extra build time or disk usage is preferable to mixing artifacts.
- Run acceptance against the executable built from the reported source revision.
  If shared output contaminated a run, repeat it with isolated output; do not
  treat the affected result as evidence about that session's code.

## Independent plan review before implementation

Every implementation packet needs an independent `rubber-duck` agent review
before the coordinator approves its plan or the owner starts implementation.
The author's self-review and the coordinator's contract review do not replace it.

This is a blocker-only review: "Will this plan fail to deliver the approved
slice?" It is not an opportunity to redesign or expand the slice.

- Run the review in the owning subsession, using a separate agent context. Give
  the reviewer the concrete plan, approved scope and decisions, relevant
  specifications, and existing code to inspect, not just a summary of the plan.
- Report only concrete blockers to the approved acceptance criteria or existing
  invariants. Each finding must identify the failure scenario, supporting
  code/specification evidence, and the smallest in-scope correction. Missing
  coverage is a blocker only when it leaves required acceptance unverified.
- Do not request new features, speculative future-proofing, optional refactors,
  style changes, or broader test/platform matrices. Do not turn hypothetical
  concerns into requirements. If there are no supported blockers, report that
  and stop; do not fill the review with optional suggestions.
- Investigate each finding against the code and owning specification. Record
  whether it was fixed, rejected with a reason, or explicitly deferred. Findings
  are questions to resolve, not automatic permission to expand scope. If a real
  blocker cannot be fixed within scope, report it to the coordinator for a
  decision rather than silently adding work.
- Report the reviewer identity or agent ID, findings, dispositions, and revised
  plan location to the coordinator. Link that evidence from the packet issue.
  Resolve blocking findings before requesting native plan approval. If the
  reviewer is unavailable, report the blocker rather than skipping the gate.
- The coordinator reads the review and final plan before approving implementation.
  If a paused plan must return to planning for review, reject it with that scoped
  instruction instead of approving implementation to unblock the reviewer.
- Reuse the reviewer for material plan changes affecting the reviewed decisions;
  do not repeat reviews for unchanged plans or minor wording edits. Plan approval
  does not replace implementation review or combined acceptance before merging.

## Implementation review in a separate context

- Delegate feature-packet code review to a `general-purpose` agent in a separate
  context, normally one reviewer per PR. Keep small documentation-only reviews
  direct. This is distinct from the earlier `rubber-duck` plan review.
- Give the reviewer the exact PR head and integration base, approved scope and
  decisions, owning specifications, and acceptance evidence. The reviewer reads
  the actual code and tests, read-only; it does not edit, merge, or spawn more
  reviewers.
- Apply the same blocker-only standard above. Return concise findings with
  file/line references, a concrete failure scenario, and the smallest in-scope
  fix, or explicitly report no blocking findings. Do not copy large code excerpts
  or exploration logs into the coordinator's context.
- The coordinator owns decisions, acceptance checks, integration, and merge.
  Do not duplicate the delegated full-code review in the coordinator session;
  inspect only the focused evidence needed to resolve findings. While review
  runs, independent acceptance against a source-associated binary can continue.
- Send supported findings to the existing implementation owner. Reuse the
  reviewer for relevant fixes or integration changes rather than launching
  repeated full reviews. Record the reviewed head and dispositions in the packet
  issue; verify the final head and combined acceptance before merging.

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
