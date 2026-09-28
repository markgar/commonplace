# Copilot instructions for Commonplace

## Guiding rule

**Build for today, architect for tomorrow.**

Build the smallest complete vertical slice that creates a useful user capability.
Minimize scope, never engineering quality: keep code typed, deterministic,
tested, and explicit about failures.

Add a module, trait, dependency, configuration value, command, or operational
mechanism only when the current slice uses it. Do not create empty placeholders
or speculative frameworks. Prefer one direct implementation path; do not add
parallel backends, compatibility layers, deprecated aliases, or fallback paths
unless explicitly approved.

Treat reviewer findings as questions to investigate, not automatic reasons to
expand scope.

## Sources of truth

Start with [`docs/specification/README.md`](../docs/specification/README.md) and
follow its ownership hierarchy. Preserve the specification's core invariants by
reference; do not restate or reinterpret them here.

- The specification owns normative product, architecture, persistence,
  sequencing, and acceptance decisions.
- Spike reports under [`docs/spikes/`](../docs/spikes/) are implementation
  evidence, not product contracts.
- `Cargo.toml` and `Cargo.lock` own dependency versions.
- [`README.md`](../README.md) describes behavior that is currently shipped.

## Approval boundaries

Explain the need and obtain approval before adding or changing a public command
or contract, a durable format, another backend or provider, background
processing, migrations, broad platform CI, or another product-boundary
expansion.

Commonplace is pre-release software. Prefer direct replacement over migration
or compatibility machinery, but never silently mutate or delete an incompatible
store. Keep ordinary CI focused; run manual platform packaging only when it is
specifically needed and authorized.

## Implementation workflow

1. State the user-visible outcome and the smallest acceptance test.
2. Inspect the relevant specification and existing code.
3. Implement one complete vertical slice through the real user path.
4. Add focused behavior, failure, and invariant tests. Use real SQLite and
   Oxigraph when their behavior is under test; use deterministic substitutes only
   for expensive inference providers.
5. Run the applicable checks documented in `README.md`.
6. Review the complete diff for unnecessary scope, duplicated contracts, and
   conflicts with the specification.
7. Stop when the acceptance test passes.

Do not merge unless explicitly requested.

## Packet progress and handoffs

Before starting/resuming a packet or reporting build progress, follow the
repo-local [`commonplace-progress` skill](skills/commonplace-progress/SKILL.md).
If the client has not discovered the skill, read that file directly.
The implementation plan links the packet issues; issues own live status,
ownership, blockers, PRs, and evidence. Update them at meaningful transitions.
Do not equate a committed change, idle session, or closed issue with integration.
