# Microsoft Teams AI recap: Lantern design review

Meeting: September 7, 2026, 11:00 AM
Participants: Maya Chen, Priya Shah, Jordan Ellis, Sam Rivera, Riley Patel
Source: Microsoft Teams meeting discussion
Distribution: All meeting participants

AI-generated content. Review for accuracy before relying on decisions or assigned
actions. This is a fictional Teams-style recap for the Project Lantern fixture,
not an actual Microsoft Teams export.

## Meeting overview

The team reviewed two interface directions for the Project Lantern internal
pilot: a board organized by status and a task list organized by owner. Discussion
focused on how Support identifies the next action in a customer handoff, how
ownership should be represented, and which interactions are necessary for the
first pilot. Maya approved continuing with the owner-grouped list and excluding
the board from the pilot scope.

Participants also discussed unassigned work, overdue indicators, completed-task
visibility, and the meaning of a handoff being ready. Several design choices
were agreed, but the due-date policy and exact readiness definition remained
open. Riley will adapt pilot preparation to the selected flow while Priya
revises the design and Jordan investigates employee identity support.

## Discussion summary

### Selecting the primary task view

Priya presented both concepts as ways to make handoff work visible. The board
emphasized movement between statuses, while the list emphasized the person
responsible for the next action. Sam explained that Support already works from
a queue and needs to identify who should act, rather than maintain another view
of task status. A board could show progress without resolving ambiguous ownership.

Maya agreed that the pilot should address that specific workflow rather than
offer multiple interchangeable views. The owner-grouped list was selected as
the default. The board is excluded from this pilot; the discussion did not
establish a commitment to build it in a later release.

### Open, completed, and unassigned tasks

The initial view will show open tasks grouped by owner. Completed tasks should
remain available through a filter so that users can check previous work without
having it compete with current actions. Hiding completed work from the default
view does not mean deleting its history.

Priya will add an explicit unassigned state. Participants distinguished between
representing incomplete work and declaring a handoff ready for Support. The
interface needs to show an unassigned task clearly, but a handoff cannot be
marked ready without a named owner. No default employee or automatic assignment
rule was approved.

### Overdue indicators and due-date policy

The group agreed that overdue work needs a text label in addition to color.
Support should not have to infer meaning from a visual treatment alone, and
the design should remain understandable when color is difficult to distinguish.
Priya will incorporate this requirement into the revised list.

The discussion did not resolve whether every task must have a due date.
Participants recognized that a task can exist before all handoff details are
known, but they did not settle when a date becomes mandatory. This remains a
product decision, not a rule that design or pilot facilitation should assume.

### Employee identity and reassignment

Jordan raised the case of an employee leaving the team or changing their
display name. Ownership needs to refer to a stable person identity rather than
depend only on visible text. Jordan will check whether the employee directory
provides stable user IDs before the implementation relies on them.

Individual reassignment is needed for the workflow, but its interaction was
not finalized in this meeting. Bulk reassignment was explicitly deferred to
keep the pilot small. The team did not agree an automatic rule for transferring
all of a departing employee's tasks.

### Readiness wording and pilot preparation

Maya will write a one-sentence definition of readiness so that the interface and
pilot explanation describe the same state. Requiring a named owner was agreed;
the complete readiness definition was not. Riley identified this wording as a
dependency for the facilitator introduction and will use the approved definition
rather than supply a separate interpretation.

Riley will update the task script to use the list-based flow. Priya's revised
unassigned state will inform the corresponding scenario. The pilot should test
whether participants can find the next action without requiring the facilitator
to explain the layout in advance.

## Decisions captured

| Topic | Outcome |
| --- | --- |
| Primary view | Use open tasks grouped by owner. |
| Board view | Exclude it from the pilot. |
| Completed work | Keep it accessible through a filter. |
| Overdue work | Use text labels as well as color. |
| Ownership | Require a named owner before a handoff can be marked ready. |
| Bulk reassignment | Defer it beyond the pilot; no delivery date agreed. |

## Follow-up tasks

| Owner | Action | Dependency or timing |
| --- | --- | --- |
| Priya Shah | Revise the list, add the unassigned state, and include overdue text labels. | Completion date not recorded. |
| Jordan Ellis | Confirm whether the employee directory provides stable user IDs. | Needed before relying on directory-backed ownership; date not recorded. |
| Maya Chen | Write the one-sentence readiness definition and resolve the outstanding due-date policy. | Date not recorded. |
| Riley Patel | Revise the pilot script for the list and align the introduction with Maya's readiness definition. | Depends on product wording and revised designs; date not recorded. |

## Open questions and dependencies

- At what point, if any, must every open task have a due date?
- What exact wording distinguishes readiness from completion?
- How should a user reassign one task when its owner changes?
- Does the directory provide the stable identity needed for ownership records?

This recap covers the September 7 discussion only. It does not establish that
follow-up work was completed or incorporate decisions from subsequent meetings.
