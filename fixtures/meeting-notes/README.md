# Riley's notebook and shared meeting recaps

Thirty-six fictional Markdown documents for local ingestion demos and the
real-model retrieval and citation tests. The tests use bounded
[search expectations](../search-relevance.json); they do not establish support
for arbitrary natural-language questions or generated answers. The documents use
ordinary Markdown, without Obsidian wikilinks.

For the story, relationships, chronology, and continuity rules behind these
documents, see the [fixture narrative](OVERVIEW.md). That overview is a maintainer
reference, not an ingestion source.

## Two separate collections

```text
fixtures/meeting-notes/
  Obsidian Notes/    22 private meeting notes, 2 personal notes, and a people map
  AI Summaries/     11 long Teams meeting recaps shared with all participants
  README.md         Fixture guide, not an input document
  OVERVIEW.md       Narrative and continuity guide, not an input document
```

## Notebook owner

**Riley Patel is a project coordinator in Support Operations. She uses Obsidian
for her own working notes.**

Project Lantern is one part of her work, not her whole notebook. Her manager is
Nina Alvarez; their private 1:1s cover priorities, workload, and a new-hire
onboarding guide. Outside work, Riley volunteers with Women in Tech Circle,
helping Aisha Rahman and Leila Hassan prepare an October 15 community event.
She also helps Sam and Devon Reed investigate operational escalation handoffs,
without owning their queues or setting response commitments. Her weekly plan
and facilitation reflection connect these responsibilities to her own development.

Riley coordinates pilot sessions, invitations, the readiness checklist, and
cross-team follow-ups. Maya leads product, Jordan leads engineering, Priya owns
design, and Sam represents Support and recruits pilot participants. Riley does
not own their decisions or implementation work.

The Obsidian collection contains ten Lantern notes, three onboarding meetings,
three escalation meetings, two manager 1:1s, four community meetings, a weekly
plan, a facilitation reflection, and her personal people map. These are short,
selective jottings: what matters to Riley, her impressions and questions, and her own
commitments. They are not comprehensive minutes or notes circulated to the team.

Exactly half of the 22 meetings have a much longer Teams AI recap covering the
wider discussion, group decisions, and follow-up assignments. The weekly plan,
reflection, and people map are not meetings and do not enter the denominator.
These are the generic notes everyone receives, not Riley's private notes and not
summaries generated from those notes. They are fictional Teams-style documents,
not actual exports from Microsoft Teams.

| Thread | Meeting notes | Shared recaps |
| --- | --- | --- |
| Project Lantern | 10 | 6 |
| Support onboarding | 3 | 2 |
| Escalation process | 3 | 3 |
| Private manager 1:1s | 2 | 0 |
| Women in Tech Circle | 4 | 0 |
| **Total meetings** | **22** | **11** |

| Meeting | Riley's private note | Shared Teams AI recap |
| --- | --- | --- |
| September 7: Design review | [Obsidian note](Obsidian%20Notes/2026-09-07%20Design%20review.md) | [AI summary](AI%20Summaries/2026-09-07%20Design%20review%20-%20AI%20summary.md) |
| September 8: Escalation handoff | [Obsidian note](Obsidian%20Notes/2026-09-08%20Escalation%20handoff%20review.md) | [AI summary](AI%20Summaries/2026-09-08%20Escalation%20handoff%20review%20-%20AI%20summary.md) |
| September 9: Engineering planning | [Obsidian note](Obsidian%20Notes/2026-09-09%20Engineering%20planning.md) | [AI summary](AI%20Summaries/2026-09-09%20Engineering%20planning%20-%20AI%20summary.md) |
| September 11: Privacy review | [Obsidian note](Obsidian%20Notes/2026-09-11%20Privacy%20review.md) | [AI summary](AI%20Summaries/2026-09-11%20Privacy%20review%20-%20AI%20summary.md) |
| September 15: Onboarding scope | [Obsidian note](Obsidian%20Notes/2026-09-15%20Onboarding%20scope%20review.md) | [AI summary](AI%20Summaries/2026-09-15%20Onboarding%20scope%20review%20-%20AI%20summary.md) |
| September 17: Escalation trial | [Obsidian note](Obsidian%20Notes/2026-09-17%20Escalation%20trial%20planning.md) | [AI summary](AI%20Summaries/2026-09-17%20Escalation%20trial%20planning%20-%20AI%20summary.md) |
| September 18: Pilot scope | [Obsidian note](Obsidian%20Notes/2026-09-18%20Pilot%20scope%20and%20schedule.md) | [AI summary](AI%20Summaries/2026-09-18%20Pilot%20scope%20and%20schedule%20-%20AI%20summary.md) |
| September 21: Onboarding corrections | [Obsidian note](Obsidian%20Notes/2026-09-21%20Onboarding%20routing%20corrections.md) | [AI summary](AI%20Summaries/2026-09-21%20Onboarding%20routing%20corrections%20-%20AI%20summary.md) |
| September 22: Import triage | [Obsidian note](Obsidian%20Notes/2026-09-22%20Import%20and%20ownership%20triage.md) | [AI summary](AI%20Summaries/2026-09-22%20Import%20and%20ownership%20triage%20-%20AI%20summary.md) |
| September 25: Readiness | [Obsidian note](Obsidian%20Notes/2026-09-25%20Pilot%20readiness%20review.md) | [AI summary](AI%20Summaries/2026-09-25%20Pilot%20readiness%20review%20-%20AI%20summary.md) |
| September 25: Escalation check-in | [Obsidian note](Obsidian%20Notes/2026-09-25%20Escalation%20trial%20check-in.md) | [AI summary](AI%20Summaries/2026-09-25%20Escalation%20trial%20check-in%20-%20AI%20summary.md) |

`Obsidian Notes/Project Lantern team map.md` names the eight Lantern participants,
their teams and roles, plus Riley's manager, operational and community contacts.
It reflects her understanding and relationships, not an official org chart, machine-readable
vocabulary, or authored graph.

The notes follow a team planning an internal handoff-tracking pilot in September
2026. They include decisions, changing ownership, unresolved questions, and a
pilot date moving from October 5 to October 12. The September 25 readiness note
includes a September 28 follow-up; its AI summary intentionally remains unchanged.
Neither Riley's interpretations nor the unreviewed AI summaries should be
silently treated as confirmed group decisions.

Other threads have their own changes: the onboarding channel reference is fixed
while permanent maintenance ownership stays open; a staffed-hours escalation
trial does not establish a ten-minute SLA; three interested community mentors
become two, still unconfirmed, and a proposed $60 supplies budget becomes an
approved $40 limit. Personal plans and worries must not leak into shared recaps.
All source documents stop at September 28; planned October events remain future.

From the repository root:

Ingest only the two source directories below, not the fixture root. This keeps
`README.md` and `OVERVIEW.md` out of Commonplace.

```sh
cargo run --bin commonplace -- --store .commonplace init
cargo run --bin commonplace -- --store .commonplace ingest \
  "fixtures/meeting-notes/Obsidian Notes" "fixtures/meeting-notes/AI Summaries"
```

On a fresh store, ingestion adds 36 documents. Repeating it without edits reports
36 unchanged. Use the returned IDs with `get`; IDs are assigned by the store and
are not fixed fixture identifiers. Ingestion creates embeddings locally; missing
model artifacts may be downloaded unless a strict offline cache is configured
(see the root README).

To demonstrate revisions, copy the two input directories to a scratch directory,
ingest those copies, then edit and re-ingest a file at the same path. The document
keeps its identity, receives a new revision, and retains the old revision and
passages. Keep generated stores, model caches, and command outputs outside this
fixture directory.
