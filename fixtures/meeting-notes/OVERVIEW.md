# Riley and Project Lantern: fixture narrative

**Maintainer reference, not an ingestion source.** This document coordinates the
fictional story behind the fixture. Keep it outside `Obsidian Notes/` and
`AI Summaries/`, and do not include it in Commonplace ingestion or retrieval
results. It describes the intended continuity; claims retrieved by Commonplace
must still come from the actual source documents, not this overview.

The current story runs from September 1 through September 28, 2026. October
events are plans, not completed outcomes.

Riley has five active threads: coordinating Lantern, owning the Support onboarding
refresh, helping investigate escalation handoffs, volunteering with Women in Tech
Circle, and developing her own delegation and facilitation skills. They overlap
through people and limited time, not through a single project hierarchy.

## Riley's life, not just a project archive

Riley Patel is a project coordinator in Support Operations. She is good at
turning a loose discussion into something people can actually do: a session
plan, a clear invitation, a checklist with an owner, or a reminder sent to the
right person. That makes her useful on Project Lantern, but it also creates
her central tension. She can start feeling responsible for everything she
tracks, including work that belongs to someone else.

Her Obsidian notebook is where she sorts this out. She does not write minutes
for the organization. She jots down what changes her next action, what she
needs to ask, what she is worried about, and what she should avoid doing.
Sometimes a major technical discussion gets one line because only one part
affects her session plan. Sometimes a small comment from her manager takes up
most of a note because it changes how she thinks about her workload.

Lantern is only one part of her job. She also owes her manager, Nina Alvarez,
an updated onboarding guide for new Support starters. Outside work, Riley
volunteers with Women in Tech Circle. Those threads belong in the same personal
notebook because this is how she remembers her life, not because they are all
part of one business initiative.

## What Lantern is trying to fix

Project Lantern is a small internal effort to improve customer handoffs between
Sales and Support. The recurring problem is not simply that a checklist has
empty boxes. People can complete the paperwork while leaving the next action
or its owner unclear. A promise to a customer can be recorded somewhere and
still fail to reach the person expected to act on it.

The proposed tool gives Support a task-oriented view of a handoff: what needs
doing, who owns it, and whether it has enough information to be ready for
Support. It is not a replacement CRM. The pilot deliberately avoids live CRM
synchronization and automatic notifications.

Maya Chen, the Product lead, owns scope and the eventual go/no-go decision.
Jordan Ellis leads the engineering work, including the UI and deployment.
Theo Brooks builds the task API and works on imports. Priya Shah owns design,
interaction wording, and the facilitator guide. Sam Rivera represents Customer
Support and recruits pilot participants. Alex Morgan is a Support specialist
whose real workflow informs discovery and usability sessions. Elena Park
reviews privacy boundaries.

Riley connects those contributions to a usable pilot. She arranges sessions,
maintains readiness tracking, and checks that the invitations, task prompts,
and facilitator materials agree. Maya is the project lead, not Riley's line
manager. Sam recruits participants; Riley schedules them. Priya writes the
guide; Riley checks it against the session plan. Jordan rehearses deployment;
Riley records whether that dependency is ready.

## Early September: finding the right question

At the September 1 kickoff, the team aims for an October 5 internal pilot.
Riley starts assembling the practical pieces, but she should not yet send
firm invitations or promise that participants can bring customer data.

The September 3 discovery conversation changes how she thinks about the
exercise. Alex describes a handoff whose owner changed twice. The checklist
looked complete, but the customer's training date was still unconfirmed.
Riley realizes that asking someone to complete a checklist would test the
wrong thing. Her session should ask who needs to act next.

On September 7, Priya presents a status board and an owner-grouped list. Sam
prefers the list because it fits Support's queue-based work, and Maya agrees.
The group keeps completed tasks available through a filter, wants overdue text
labels rather than color alone, and defers bulk reassignment. Stable employee
identities and the exact readiness definition still need follow-up.

Riley's personal takeaway is much smaller: one script instead of two, an
unassigned-task scenario to prepare, and a warning not to coach participants
by explaining why the list is supposedly better. She also notes that she
cannot turn an unresolved due-date question into a test criterion.

By September 9, the directory is confirmed to provide stable IDs. Maya settles
the date rule: tasks can start without a due date, but a handoff cannot be
marked ready while an open task lacks one. Theo is involved in the task API,
while the CSV format is still uncertain. Riley begins backing away from the
idea of letting participants bring arbitrary exports.

## Mid-September: the pilot becomes more concrete

The September 11 privacy discussion corrects an important assumption. The
pilot is an internal workflow exercise using synthetic handoffs, not approval
to use real customer records. Elena's later access review is a separate
approval path. Riley spots the problem in her own draft invitation: asking
people to bring a recent handoff could invite exactly the data they should
not use.

By September 14, the list and unassigned state are working, but keyboard
navigation and imports still need attention. Different CSV exports contain
different columns and ambiguous owners. The team chooses one supported
template rather than guessing. Riley is nervous about the schedule, but her
worry is not yet a decision to postpone anything.

The September 16 walkthrough exposes smaller, practical failures. Alex cannot
easily find reassignment and reads "ready" as "all done." Priya and Jordan
have design and implementation work to do. Riley has a facilitation lesson:
her instinct to explain a confusing label would hide the very problem the
walkthrough is meant to reveal.

On September 18, the team formally moves the pilot from October 5 to October 12.
Engineering needs more time, and Sam has a Support training conflict on the
original date. Maya chooses to keep ownership-change activity history rather
than cut the feature to preserve the earlier schedule.

The pilot will involve six Support participants, with two short sessions each.
It will observe task completion and confusion, not rank employee performance.
For Riley, the decision means fresh availability, revised invitations, and
removing the old date from the announcement. Six is still a recruitment target,
not six confirmed acceptances.

## Late September: readiness is not the same as approval

On September 22, Sam's sample exposes duplicate imports and poor handling of
blank dates. Theo takes over duplicate-import work previously assigned to
Jordan, who stays focused on keyboard navigation and reassignment. Riley must
update her checklist or she will chase the wrong person.

An unknown employee ID remains unresolved at that meeting. Sam proposes
leaving the task unassigned and blocking readiness. That is a suggestion,
not an accepted policy. At the September 25 review, the group instead agrees
to reject the import row explicitly rather than silently discard its intended
assignment.

The September 25 review also includes a successful repeat-import demonstration
and reports of completed workflow, keyboard, error-focus, and log-review work.
But Sam's participant confirmations, Priya's facilitator guide, and Jordan's
deployment and rollback rehearsal are still outstanding. Maya sets October 8
as the go/no-go checkpoint for the planned October 12 pilot. Nobody has made
the final go decision.

Riley later appends a September 28 update to her personal September 25 note:
Sam has sent all six confirmations and Priya has delivered the guide. Riley
can now match people to slots and review the guide. Jordan's rehearsal remains
open. The original Teams recap is not updated to know what happened three days
after its meeting.

## Her manager sees a different part of the story

Nina Alvarez manages Riley in Support Operations. Their September 8 and
September 24 conversations are private 1:1s, not Lantern status meetings.
Nina sees Riley spending energy making other people's work look complete
and asks her to separate ownership from coordination.

There is concrete work outside Lantern: an onboarding guide for two new
Support starters. Its escalation-channel information is out of date. Riley
asks Sam for the correct channel and has fixed that section by September 24,
but the draft still needs a final read. She commits to sending it to Nina on
September 29. The fixture does not yet say she has sent it.

Nina's advice connects the threads without making her a Lantern decision-maker.
Waiting for Jordan's rehearsal is a status Riley tracks, not a rehearsal Riley
owes. Moving the pilot does not move every other deadline. Riley needs room
to finish her own work rather than filling every gap with another follow-up.

## Onboarding is something Riley actually owns

The onboarding refresh starts with a September 4 inventory with Sam. Two new
Support colleagues are expected in October, and the first-week material has
drifted: an obsolete escalation-channel reference, screenshots that assume
someone already knows the tools, and no clear distinction between a guide and
the conversations a buddy should have with a new starter. Riley owns producing
a usable guide and checklist, not the underlying Support policies.

At a September 15 review with Nina, Sam, and Alex, the group chooses a small
first-week refresh instead of rebuilding the entire training program. Alex
will walk through the instructions as an experienced Support specialist and
flag missing context. Sam will verify routing and operational instructions.
Riley will consolidate the changes; Nina will review the draft. Ownership of
long-term maintenance remains unresolved. Sam suggests Support Operations
could maintain everything, but Nina does not accept that as an assignment.

The escalation investigation complicates the guide. On September 21, Sam
confirms that `#support-escalations`, not the old `#support-triage` reference,
is the current intake channel. That correction is settled. A named-receiver
handoff experiment is not settled policy and must not be turned into a
permanent instruction or a promised response time for new starters.

Riley corrects the channel reference and separates the temporary experiment
from the guide's stable instructions. Alex identifies missing context around
when to ask a buddy for help. By September 24, the channel section is fixed,
but the guide still needs a final read. Riley's September 29 draft commitment
is to Nina, not a publication date or an approved new onboarding policy.

## Escalations are an operational problem, not a Lantern feature

On September 8, Sam and Alex bring Riley a de-identified example, ESC-17, of a
Support case passed twice between Customer Support and Service Reliability
without an acknowledged receiving owner. Devon Reed, a Service Reliability
engineer who coordinates with that team's duty rota, joins the investigation.
Devon is not part of Lantern's engineering team, and this case is not a
Lantern import or a pilot task.

Riley's role is to map the handoff and record what people actually do. She
does not run the reliability rota, resolve the technical case, or set a
service-level commitment. Sam owns the Support side of the discussion; Devon
checks the receiving side. A destination queue alone does not establish that
someone has accepted responsibility.

At the September 17 working session, Sam suggests a ten-minute acknowledgement
target and direct assignment into Reliability's queue. Devon explains that
neither can be treated as an agreed commitment without coverage and authority
being established. The group instead agrees to a limited staffed-hours trial:
Support identifies a receiving person through the current escalation route
and records acknowledgement before treating a handoff as accepted. The
existing out-of-hours process is unchanged.

They will review examples on September 25. Riley records the experiment and
its open questions, not a permanent policy. This distinction matters because
her onboarding guide could otherwise make a temporary workaround sound like
an organization-wide rule.

At the September 25 review, the group has only two trial examples: one with a
clear acknowledgement and one whose receiving person is unclear after a shift
change. That is insufficient evidence to claim the process is fixed. The
limited trial continues while Sam and Devon check coverage and responsibility
at shift boundaries. No ten-minute SLA, permanent process, or wider rollout
has been approved by September 28.

Sam appears in Lantern, onboarding, and escalations for different reasons.
Devon's work is separate from Jordan's deployment rehearsal. Shared words such
as "handoff," "owner," and "ready" should not collapse the operational incident
and the product pilot into the same activity.

## Women in Tech Circle is a separate community

Riley helps Aisha Rahman and Leila Hassan organize an October 15 career-stories
evening. Aisha organizes the local volunteer meetups; Leila coordinates mentors.
Riley helps with logistics and welcoming people. This is outside her job and
is not a company recruitment event or a Lantern activity.

At the September 10 planning conversation, the community library is only a
possible venue. Access, layout, and booking still need checking.

A September 16 planning call introduces a small practical constraint: refreshments
and printing need a budget, and Riley must not pay first and assume she will
be reimbursed. A proposed $60 supplies allowance has not been approved.
Venue confirmation still belongs to Aisha at that point.

By September 23, Aisha has confirmed a room with a step-free entrance, space
for 24, and tea allowed in a side area. Three mentors are interested, but their
participation is not yet confirmed.

On September 26, Leila reports that one of the three interested mentors can no
longer attend. Two remain interested; neither is recorded as confirmed.
The group keeps the small-conversation format without advertising a guaranteed
mentor-to-attendee ratio or formal mentoring matches. Aisha agrees a $40
supplies limit, below the earlier proposal. Riley will simplify printing and
refreshments within that limit; no purchase or reimbursement is recorded.

Riley likes the idea of small conversations rather than a long panel. She wants
the welcome to include people who are not already working in technology, and
she can speak from her own Support Operations experience rather than pretend
every tech career is a programming role. She is nervous about the two-minute
welcome; Nina encourages her to practice instead of endlessly rewriting it.

The close dates create a believable personal pressure: Lantern is planned for
October 12, and the community event is October 15. Riley worries about mixing
up materials. The events nevertheless retain different people, permissions,
purposes, and invitation lists. Neither has happened yet.

## Riley is also learning how to work

Her September 20 weekly plan is not a meeting record. It puts Lantern preparation,
the onboarding draft, escalation documentation, and community logistics on the
same page. A request about operational handoffs has consumed the writing time
she meant to reserve for onboarding. Her response is to distinguish what she
owns, what she is waiting on, and what she can decline, rather than pretend
everything has become equally urgent.

In a September 28 reflection, Riley records practicing the community welcome
over the weekend. It ran longer than two minutes, so she still needs to shorten
it. She sees the same facilitation habit at work and outside it: explaining too
much instead of giving people room to participate. This is private reflection,
not a formal performance assessment or a goal approved by the project team.

There is progress without a tidy resolution. Riley has a better sense of her
boundaries, but the guide still needs review, escalation coverage is unresolved,
Lantern awaits a rehearsal and decision, and the community mentors are not yet
confirmed. Her notebook should feel like work in motion.

## Two kinds of documents, two kinds of knowledge

`Obsidian Notes/` contains Riley's private, selective account. The voice is first
person, sometimes fragmentary, and oriented toward her next action. A concern
can be tentative. A checkbox can remain open. A later update can change her
understanding without rewriting what she thought at the time. Her people map
is also her working understanding, not an official organizational record.

`AI Summaries/` contains the longer, generic Microsoft Teams-style recaps
distributed to everyone at six Lantern meetings (September 7, 9, 11, 18, 22,
and 25), two onboarding meetings (September 15 and 21), and three escalation
meetings (September 8, 17, and 25). This gives 11 shared recaps for 22 meetings.
The weekly plan, personal reflection, and people map are not meetings and do not
enter that denominator.
They cover the broader discussion, decisions, actions, and unanswered questions.
They are not generated from Riley's Obsidian notes. The two accounts overlap
because they concern the same event, not because one is a rewritten copy of
the other.

All documents are fictional. Within that fiction, the Teams text remains
AI-generated and unreviewed; it should not become infallible evidence merely
because it is longer or more formal. Shared recaps must not reveal Riley's
private manager conversations, volunteer worries, or unspoken observations.
The fixture currently contains no Teams recaps for her 1:1s or community meetings.

## Continuity rules for extending the fixture

- Keep the chronology visible. October 5 is the original pilot plan; October 12
  replaces it on September 18. October 8 is the decision checkpoint, not the
  pilot date. October 15 belongs to Women in Tech Circle.
- Preserve the distinction between proposed, agreed, reported complete, and
  independently verified. Sam's September 22 suggestion is not the September 25
  decision. An interested mentor is not a confirmed mentor.
- Keep responsibility separate from visibility. Riley knowing about a task does
  not make it hers. Nina manages Riley; Maya leads Lantern.
- Respect each document's time horizon. September 25 shared notes cannot
  contain September 28 confirmations. Historical notes do not need to be
  rewritten every time circumstances change.
- Do not silently complete open work. As of September 28, the rehearsal, final
  pilot approval, real-data access approval, onboarding draft delivery, and
  mentor confirmations have no recorded completion. Long-term onboarding
  maintenance and permanent escalation policy are also unresolved.
- Keep stable instructions separate from experiments. `#support-escalations`
  is the confirmed current channel; the named-receiver trial is temporary and
  staffed-hours only. A proposed ten-minute target is not an approved SLA.
- Track community changes by date: three interested mentors on September 23
  become two on September 26, still unconfirmed. The proposed $60 allowance
  becomes an approved $40 limit; approval is not evidence of spending.
- Let the personal notes omit things. Riley does not need to reproduce every
  decision, attendee statement, or action in the shared recap.
- Do not invent additional reporting lines, participant identities, exact
  deadlines, or completed events just to fill a table. Add a dated source
  document when a new fact or development becomes part of the story.
- Treat this overview as authoring guidance, never as hidden evidence supplied
  to Commonplace. If a future question needs an answer, ensure an appropriate
  ingested document actually supports it.
