# Commonplace: Capability Gap and Simplification Audit

Status: Working revision after SQLite schema review  
Revision: 0.42  
Previously approved: revision 0.41 on 2026-09-27  
Review: Revision 0.42 requires joint rereview with the SQLite persistence design.  
Companions: `design.md`, `class-architecture.md`

## 1. Purpose

Identify implemented capabilities that are not yet explicitly defined in the v2
design, then decide whether each capability should be:

- **keep** because it directly supports ingestion or query;
- **simplify** because the product value can be retained with much less machinery;
- **remove** because it does not materially support the personal ingest/query
  loop; or
- **decide** because it changes the meaning of stored knowledge or query results.

This is not a compatibility checklist. V2 does not preserve old APIs, schemas,
commands, or storage. Existing behavior is evidence about possible product needs,
not a contract.

## 2. Decision standard

A capability belongs in v2 only if it directly enables:

1. ingesting local or streamed sources;
2. preserving exact useful evidence;
3. complete hybrid retrieval;
4. explicit evidence-backed knowledge authoring;
5. complete Ladybug graph projection;
6. useful text or graph queries; or
7. correctness necessary for those operations.

Being sophisticated, defensive, general, or already implemented is not enough.

## 3. High-impact semantic decisions

These questions affect what query results mean. They should be decided before
the physical schema is finalized.

### 3.1 Knowledge remains explicit when sources change

Current behavior binds knowledge to an exact source state. A source edit can make
previously authored knowledge ineligible even when the quoted historical passage
still exists.

Decision: **use an explicit knowledge lifecycle**.

A fact remains current until explicitly withdrawn. Source edits do not silently
change authored knowledge, and citations to retained historical revisions remain
inspectable. Explicit source removal deletes that source and its evidence links
without withdrawing affected facts.

Why:

- sources are mostly immutable;
- authored knowledge represents the agent's accepted interpretation, not merely
  a live view over current source files;
- implicit invalidation requires dependency-state machinery throughout reads,
  graph projection, counts, and history;
- explicit withdrawal is understandable and already part of v2; and
- the user can decide whether a materially changed source warrants correcting its
  knowledge.

A revised source makes the older citation historical. An explicitly removed
source deletes its citations but does not automatically withdraw the fact.

Sources and knowledge represent different things:

- meeting notes, email, and imported summaries record what was said or received
  and are normally immutable;
- proposed outcomes and decisions are authored knowledge that may evolve;
- later meetings or messages are new sources;
- changed outcomes are new facts, optionally accompanied by withdrawal of facts
  that should no longer be current.

### 3.2 Knowledge items have optional passage evidence

Decision: **type memberships and facts may have zero or more passage citations,
with no seed subsystem**.

Use one table:

```text
knowledge_item_evidence(knowledge_item_id, passage_id)
```

A cited membership or fact has rows; an uncited item has none. Query and graph
results always return a support array, which may be empty. Aliases and identifiers
are add-only metadata initially and are not individually cited or withdrawn.

This avoids:

- seed-set identities;
- seed slots;
- set generations;
- membership events;
- separate seed authorization;
- seed-specific entity and classification rules; and
- mixed source/seed support semantics.

It also avoids forcing the agent to ingest a synthetic note merely to record
personal knowledge. The tradeoff is explicit rather than hidden: the KG cannot
claim every authored fact is grounded. Hybrid search remains fully grounded.

### 3.3 Mention records

Current behavior can store a passage-level mention of an entity without asserting
a relationship or merging identity.

Potential value:

- ask where an entity was discussed;
- connect graph entities to otherwise unstructured passages;
- distinguish mere mention from an asserted fact.

Cost:

- another fact kind;
- mention-specific passage-only validation;
- graph nodes or edges;
- additional authoring and withdrawal shapes; and
- more graph volume.

Decision: **exclude first-class mentions initially**.

Hybrid search can find textual mentions. Explicit facts already link entities to
supporting passages. Add mention records only if a real query cannot be answered
well by those two paths.

### 3.4 Minimal author attribution

Current behavior retains owner, writer, producer, producer version, model,
configuration, retry identity, timestamps, and protected rationale.

Decision: **keep minimal attribution**:

```text
created_at
created_by      optional short agent/tool label
```

Optionally retain a free-form `note` or rationale only when supplied. Do not build
ownership, writer bindings, model authorization, protected attribution, or
provenance-specific read rules.

This preserves enough information to understand whether the user, an agent, or an
importer authored a fact without creating a security subsystem.

### 3.5 Knowledge authoring permits duplicates

Decision: **do not implement knowledge-write idempotency or automatic semantic
deduplication**.

Repeating an authoring command may create duplicate facts. Each fact receives a
stable SQLite ID. The agent may use Cypher to identify likely duplicates, present
them for user review, and withdraw unwanted facts.

Consequences:

- graph counts count active stored fact records;
- identical-looking relationships may appear more than once;
- no write receipt or caller retry state is required; and
- an uncertain caller should inspect canonical item IDs before blindly repeating a
  knowledge write.

Source ingestion remains idempotent through stable source keys because duplicate
documents would be common and expensive. Knowledge authoring deliberately stays
append-only and simpler. The single `record_knowledge` boundary preserves a seam
for adding persistent request IDs later without implementing them now.

### 3.6 Knowledge inputs are atomic as a whole

Decision: **ordinary knowledge-command failures roll back both the attempted
knowledge change and candidate graph; rare finalization crashes are detected
rather than automatically repaired**.

`kg record` resolves and creates entities, type assignments, aliases, identifiers,
facts, and evidence links in one transaction held through candidate graph
construction. `kg withdraw` validates and withdraws its complete submitted
`KnowledgeItemId` set in a separate whole-request transaction held through its
candidate graph construction. Either command rolls back completely on invalid or
ambiguous input or graph-build failure.

This removes partial knowledge-batch outcomes and prevents a logically related
extraction from being half-written during ordinary command execution. Final graph
activation and SQLite commit share a monotonic `knowledge_version`, but not a
physical transaction. A crash between them may produce a mismatch; graph queries
refuse that state, and initial recovery is a fresh store. `GraphRuntime` preserves
a seam for immutable generations later without implementing them now.

Document ingestion intentionally uses the other failure boundary: each document
publishes atomically, successful documents remain committed, and the aggregate
result is `complete`, `partial`, or `failed`. These policies remain explicit in
their workflows rather than being generalized into a transaction framework.

This avoids a public graph-rebuild recovery command, generation orchestration,
and transaction coordination. Document ingestion remains independently committed
per source because document/model work is a different workload.

## 4. Evidence capability decisions

### 4.1 Supplied anchors

Current ingestion can accept caller-supplied exact anchors in addition to generated
passages.

Decision: **remove supplied anchors initially**.

Use one deterministic passage strategy. External adapters may preserve useful
structure through normalized Markdown headings and paragraphs. Adding arbitrary
caller-defined anchor sets creates a second evidence-production path and more
identity rules.

If real sources later require adapter-defined segmentation, extend
`DocumentInput` with optional segments and route both forms through the same
passage publication function.

### 4.2 Separate anchors and passages

Current storage distinguishes source anchors, generated anchors, passages,
passage sets, anchor sets, and their memberships.

Decision: **use passages as the single citable source unit**.

A passage contains:

- exact quote;
- offsets;
- revision;
- ordering; and
- optional structural metadata such as heading path.

Do not preserve a parallel anchor hierarchy unless a demonstrated query needs
multiple independently managed segmentation systems.

The initial passage function is deterministic and shared by every source:
paragraph accumulation to a target size, with oversized paragraphs split into
fixed nonoverlapping windows. Adapters do not supply boundaries.

### 4.3 Metadata snapshots

Current citations can retain a separately identified metadata snapshot for every
document state.

Decision: **store all user-visible metadata on the document revision**.

One revision record contains the text, title, source type, occurred-at timestamp,
and custom metadata used to present its citations. Changing any of those values
creates a new revision. Internal operational metadata may update in place.

Do not introduce independent metadata snapshots or metadata-only state
transitions.

### 4.4 Rich source context and revision comparison

Implemented reads include source ranges, heading-bounded context, revision lists,
activation-aware comparisons, anchor inventories, and state histories.

Decision: provide one minimal `kg get ID` command for document, revision, passage,
entity, and knowledge-item IDs.

- A document includes its revision IDs.
- A revision returns its complete stored text and metadata.
- A passage returns its exact citation.
- An entity returns current types and fact IDs.
- A knowledge item identifies whether it is a type membership or fact and remains
  readable after withdrawal.

Do not initially provide centered context windows, semantic revision comparison,
activation history, a generic history mode, or separate noun-specific read
services.

### 4.5 Exact byte edge cases

Current storage deliberately preserves unusual valid text forms including empty
content, CRLF, BOM, NUL, combining marks, and non-BMP characters.

Recommendation: **preserve exact valid Unicode text and offsets, but reject
non-text input**.

Do not normalize newlines or Unicode. Reject undecodable bytes. No binary-document
storage or exotic compatibility suite is required beyond proving citations remain
exact for normal Unicode text.

Canonical offsets are zero-based, half-open UTF-8 byte ranges into the exact
stored revision. Both ends must be valid character boundaries and the selected
bytes must equal the stored passage quote.

## 5. Search and indexing capability decisions

### 5.0 Vector storage and retrieval

Decision: **use one SQLite vector extension from the first version**.

Do not implement an application-level exact-scan path or support multiple vector
backends. The selected extension owns dense candidate retrieval; SQLite remains
the single canonical database. The exact extension is chosen after the
implementation language and packaging constraints are known.

### 5.1 Public indexing lifecycle

Current APIs expose process, status, pending scans, rebuild modes, cleanup, attempt
history, stale staging, and independent readiness.

Decision: **remove the public indexing lifecycle**.

`kg ingest` performs complete preparation. A failed command is rerun. Derived rows
for an updated document are replaced transactionally. There is no public
`index process`, `pending`, `cleanup`, or attempt-history API initially.

Do not store a per-document search fingerprint or add a full rebuild command.
Passage generation, embedding identity, vector representation, and lexical
storage are part of the exact v2 store format. An incompatible change refuses
the old store; the experimental recovery is a fresh store and reingestion. Add
in-place rebuilding only if a real retained dataset later makes that loss
unacceptable.

### 5.2 Multiple embedding profiles

Current behavior supports multiple pinned profiles and contextual/noncontextual
representations.

Recommendation: **one model and one representation**.

Keep only enough stored model identity to reject vectors produced by incompatible
code or dimensions. Do not expose profile selection in the first CLI.

For batch ingestion, load that model once per `kg ingest` process and reuse the
same session across all changed documents. Passages are sent through bounded
inference batches, which may span documents; the implementation must not reload
the model per document or hold the entire source collection in memory.

Construct model providers as lightweight handles and load weights lazily on the
first actual inference call. `init`, schema, knowledge, exact-record, and graph
commands therefore do not initialize embedding or reranking models. Keep this
private to the two concrete providers; do not add provider factories or a
general lifecycle framework.

This is a required ownership rule, not an optional optimization: one command owns
one provider session and may batch passages across documents while preserving
independent document publication.

### 5.3 Search filters

Some implemented search surfaces support source, date, and subject filters. V2
currently defines only query and limit.

Decision:

- keep optional `--since`;
- keep repeatable `--source-type`;
- apply both identically to lexical and vector candidate selection; and
- do not add subject/graph-expansion filtering to hybrid search initially.

The calling agent can use graph query for structured subject scope and hybrid
search for textual relevance. Combining graph expansion into text candidate
generation would couple the two query engines again.

### 5.4 Search explanations

Current behavior can expose stage membership, model identities, lexical/dense
positions, fusion contributions, reranker scores, counts, truncation, and quotes.

Recommendation: **remove the explanation subsystem**.

Ordinary search results may include final rank and optional lightweight final
score. Use ordinary debug logging during development. Add a narrow
`--debug-search` response only if retrieval evaluation actually needs candidate
stage data.

Do not build retained reports, event unions, disclosure groups, report lookup, or
separate explained wrappers.

### 5.5 Search consistency during writes

Current search uses elaborate observer and final-release fencing to reject any
intervening commit.

Recommendation: **one SQLite read transaction is sufficient**.

It provides a consistent source/index view for one local search. A process-level
writer lock and short transactions are enough for v2. Do not add global generation
observers or post-query authorization fences.

## 6. Knowledge capability decisions

### 6.0 Type membership storage

Decision: **use a dedicated `entity_type_membership` table**.

Encoding types in the generic fact table would reduce the table count but add
nullable columns, special object semantics, repeated kind filters, and more
branching in validation and graph projection. The dedicated table keeps those
operations direct while sharing the same created-by, evidence, and withdrawal
lifecycle as other authored knowledge.

### 6.1 Schema history

Current behavior stores complete immutable schema revisions, proposal provenance,
approval records, and historical reads.

Decision: **keep only additive schema state and a monotonic version**.

Because terms are never redefined or removed:

- current schema contains all usable historical terms;
- each term may record the version in which it was introduced;
- each fact may record the schema version under which it was validated.

Do not store complete repeated schema-definition snapshots or provide historical
schema reconstruction.

SQLite is the only complete vocabulary catalog. Schema application does not
rebuild Ladybug; graph snapshots include only types and predicate identifiers
used by active memberships and facts. Unused terms remain visible through
`kg schema show` and are not duplicated into the graph.

### 6.2 Special decision encoding

Current behavior gives a registered decision predicate special encoding and
dedicated query/count behavior.

Decision: **decisions are ordinary literal assertions**.

The user's schema may define a predicate named `decision`, but SQLite, Ladybug,
and Cypher do not need a separate decision subsystem. Cypher can filter and count
that predicate.

### 6.3 Actions, blockers, conflicts, and status

Implemented demonstration behavior has dedicated record types and commands for
actions, blockers, conflicts, status, replacement, and evidence gaps.

Decision: **do not implement them as built-in product concepts**.

They can be user-defined predicates and entities queried through Cypher. If a
repeated workflow later deserves CLI sugar, add a thin saved query rather than a
new storage model.

### 6.4 Record supersession

Implemented demonstration records can supersede earlier keyed records, including
cycle and conflict handling.

Recommendation: **remove supersession**.

Use the uniform add/withdraw lifecycle. A replacement is a new fact plus explicit
withdrawal of the old fact. Do not add version-key graphs, winner selection, or
effective-record resolution.

### 6.5 Independent entity support

Current behavior distinguishes an entity's existence support from classifications,
aliases, identifiers, and assertions.

Recommendation: **keep this capability in a simpler form**.

Creating an entity does not require a separate evidence lifecycle. A bare but
identified entity may exist in SQLite before relationships are known, but it is
not graph-active until it participates in an active type membership or fact.

### 6.6 Multiple evidence references

Current source support can be conjunctive across several exact references.

Recommendation: **keep multiple evidence references per knowledge item**.

This directly improves query trust and uses one shared lifecycle table:
`knowledge_item_evidence(knowledge_item_id, passage_id)`.

The first version does not need formal conjunctive eligibility logic if facts
remain active until withdrawal. The evidence list means "these passages supported
the authoring decision."

### 6.7 Literal value kinds

Current schema supports string, integer, boolean, timestamp, and entity objects.

Recommendation: **keep these five kinds**.

They map cleanly to SQLite and JSON and support useful personal queries. Do not add
arbitrary nested JSON, units, decimals, geospatial values, or custom codecs until
needed.

## 7. Graph capability decisions

### 7.1 Evidence available directly in Cypher

Decision: project:

- document nodes with source key, title, source type, and occurred time;
- passage nodes with passage ID, quote, offsets, and revision ID;
- semantic membership/fact edges carrying canonical `knowledge_item_id`;
- matching `KnowledgeItem` nodes;
- `SUPPORTED_BY` edges from knowledge items to passages; and
- `FROM` edges from passages to documents.

This lets one Cypher query return useful evidence without a second CLI call.
SQLite remains authoritative; graph evidence is a query projection.

Project only passages supporting active knowledge items and the documents that
contain them. All other passages remain available through SQLite hybrid search.
This avoids duplicating the corpus and avoids graph rebuilds for ordinary
document ingestion.

Document nodes carry only stable document identity and source key. Each passage
node carries the title, source type, occurred-at value, and other display
metadata from its own revision so citations from different revisions remain
accurate.

### 7.2 Historical and withdrawn facts in Ladybug

Decision: **project current active facts only**.

Historical and withdrawn facts remain readable from SQLite by ID. Do not double
the graph model with active flags, withdrawal edges, and historical query modes
until a real graph-history query is needed.

### 7.3 Incremental graph refresh

Decision: **remove initially**. Completely rebuild Ladybug after every successful
knowledge authoring or withdrawal command. The `GraphRuntime` boundary allows a
later incremental implementation if measured personal graph size proves rebuilds
too slow.

### 7.4 Fixed relationship and decision APIs

Recommendation: **remove**, as already decided. Arbitrary Cypher replaces fixed
one-hop traversal, relationship-to-decision joins, specialized counts, and proof
assembly only if the selected Ladybug integration proves native read-only
execution and real cancellation. Failure reopens the graph-query architecture or
runtime choice before implementation; do not ship an unsafe approximation.

### 7.5 Retained graph sessions

Recommendation: **remove**.

Each CLI query opens the current graph, executes, and closes. Internal connection
reuse within one process is not a public lifecycle or correctness contract.

## 8. Query framework capabilities not needed in v2

Current behavior includes query plans with dependent steps, selected output
closure, exact counts, retained result sets, support inspection, continuation,
operation budgets, process supervision, and nested diagnostics.

Recommendation: **remove the query framework entirely**.

V2 has direct operations:

- hybrid `kg search`;
- exact `kg get`;
- read-only `kg graph query`.

Cypher performs graph composition and counts. There is no need for a second
general query-plan language.

Specifically remove:

- query step DAGs;
- resolve/records/count/evidence plan composition;
- result-set IDs;
- retained membership;
- five-minute support inspection;
- continuation tokens tied to service memory;
- spawned query workers;
- semantic operation accounting;
- parent/child diagnostic captures; and
- special incomplete-count semantics.

Use ordinary bounded result limits and Cypher `COUNT`, `LIMIT`, and predicates.

## 9. Operational capabilities to remove

### 9.1 Multi-user policy and authorization

Remove:

- principals;
- grants;
- namespaces;
- corpus access contexts;
- document and knowledge writer bindings;
- policy versions;
- policy replacement; and
- disclosure reauthorization.

The filesystem and local OS account are the security boundary.

### 9.2 Processing control

Remove:

- plans;
- worker registration;
- scheduling;
- claims;
- leases;
- heartbeats;
- failure classification;
- backoff;
- retries;
- recovery;
- checkpoints;
- batches;
- synchronization scans; and
- processing guards.

The CLI executes work synchronously.

### 9.3 Durable receipts and commit certainty

Remove:

- write receipts as durable product records;
- expiring retry keys;
- tombstones;
- response replay;
- max-observed clocks;
- unknown-commit outcomes; and
- operation correlation ledgers.

Keep ordinary created/updated IDs in command results and database uniqueness
constraints for idempotency.

### 9.4 Execution diagnostics

Remove:

- explained variants of every operation;
- retained reports;
- report discovery;
- event type unions;
- quote opt-in diagnostics;
- disclosure groups;
- diagnostic target authorization; and
- process-lifetime report retention.

Use concise errors and optional ordinary logging.

### 9.5 Fine-grained resource accounting

Remove:

- row-visit budgets;
- SQLite instruction budgets;
- semantic operation units;
- scratch reservations;
- staged output accounting; and
- provider-position preflight frameworks.

Keep simple concrete limits:

- maximum document bytes;
- maximum documents per batch if needed;
- maximum passages per document;
- search candidate and result limits;
- graph row limit; and
- query timeout.

### 9.6 Capability discovery

Current services expose detailed capability documents distinguishing installed,
unsupported, and validation-only behavior.

Recommendation: **remove `kg capabilities` initially**.

V2 has one supported product configuration. Standard `--help`,
`kg schema show`, and `kg graph schema` provide the information an agent needs.
Add capability discovery only if optional runtime combinations later become a
real supported product state.

### 9.7 Profile attachment and model approval workflow

Current CLI supports profile creation, attachment, explicit model approval, model
cache selection, and separate configuration/data roots.

Decision: **use one pinned local embedding model and reranker with the selected
runtime's standard model cache**.

`kg init` creates the databases and local configuration. It does not implement
profile attachment, identity provisioning, model approval state, cache profiles,
or a separate model-preparation workflow. First ingest/search loads the pinned
models and may retrieve missing artifacts through the runtime's ordinary supported
mechanism. Unavailable artifacts fail clearly. Source text is never sent to remote
inference.

## 10. Public API decisions

### 10.1 Stable Python SDK

The current implementation exposes extensive typed Python service APIs in
addition to the CLI.

Decision: **make the CLI JSON contract the only stable public interface
initially**.

Internal application components remain clean and testable in the selected
implementation language, but they are not public. Do not promise long-term module
paths, constructors, traits/interfaces, or service models.

This can remove a large amount of duplicated contract documentation and
compatibility pressure.

Rust is the selected implementation language. Distribution is a self-contained
release: users do not install Rust, Python, Node.js, .NET, obtain the source, or
run a build. The release may bundle required native libraries and pinned model
files alongside the executable.

This choice is gated by a disposable integration spike because the complete
product depends on several native surfaces. The spike must prove the selected
Ladybug version, one SQLite vector extension, local embedding, local reranking,
native read-only and cancellable Cypher, and release packaging together. Record
the tested platform and versions, exact commands, artifact contents, and observed
pass/fail results. If Rust requires substantial custom binding or packaging
infrastructure, the language choice is reopened rather than preserving that
machinery.

### 10.2 Internal seams without optionality

Decision: **preserve change locality without building multiple choices**.

Use small internal seams around likely substitutions: model runtimes, reranking,
passage policy, fusion, SQLite vector SQL, source adapters, and Ladybug projection.
Each has one selected implementation. Do not add plugin systems, provider
registries, runtime strategy selection, duplicate storage paths, or compatibility
adapters merely so a decision can be reversed later.

### 10.3 Machine-readable input schemas

Decision: **expose focused machine-readable descriptions for complex JSON
boundaries**.

Provide:

```sh
kg ingest --describe --json
kg schema apply --describe --json
kg record --describe --json
kg graph schema --json
```

Generate input JSON Schema and examples from the actual boundary validators.
`kg record --describe` also returns the current user vocabulary, endpoint rules,
literal kinds, and schema version in the same call. This code earns its weight by
preventing agents from guessing input shapes or chaining separate discovery calls.

Do not create a separate foundation-contract package or hand-maintained duplicate
schemas.

### 10.3 Skills and examples

Recommendation: keep one concise agent skill and a few executable examples only
after the CLI stabilizes. They are documentation over the actual commands, not a
parallel behavior contract.

## 11. File and directory adapter details

Current ingestion contains behaviors not yet defined in v2:

- include/exclude patterns;
- symlink handling;
- maximum file size;
- stable handling of file moves;
- parser-specific Markdown blocks;
- manifest metadata mappings; and
- absence-based source removal.

Recommendations:

- support `.md` and `.txt` recursively;
- allow simple repeated `--include` and `--exclude` globs only if needed;
- skip symlinks by default;
- impose one explicit maximum document size;
- treat a path rename as a new source key unless a manifest or JSON Lines input
  supplies a stable key;
- preserve Markdown as text rather than extracting tasks/decisions automatically;
- accept explicit metadata in JSONL/manifest;
- never infer deletion from directory absence.

Do not build move detection, inode tracking, parser-owned knowledge extraction, or
directory synchronization initially.

These directory identity and removal rules are decided for v2.

Explicit removal remains one minimal operation: `kg remove <source-key>`
permanently deletes one document, its revisions, passages, search rows, and
evidence links. It rebuilds Ladybug but does not withdraw affected knowledge
items, which may become uncited. Reingestion creates a new document identity.
There is no batch removal, restore, tombstone, or cascade-withdrawal subsystem.

## 12. Recommended cut list

The following current capabilities should be explicitly absent from v2 unless
later usage proves a need:

1. Multi-user authorization and policy rotation.
2. Corpus and namespace isolation.
3. Processing plans, jobs, workers, leases, and retries.
4. Durable receipts and response replay.
5. Execution explanation and retained diagnostic systems.
6. Fine-grained resource metering.
7. Query plans, result-set retention, and support inspection.
8. Fixed graph traversal and decision query APIs.
9. Classification claims, reviews, and selection events.
10. Seed sets and seed-backed support.
11. First-class mentions.
12. Separate anchor and passage hierarchies.
13. Schema proposal, evidence-review, approval, and audit workflows.
14. Multiple embedding profiles and contextual projection variants.
15. Public indexing status, pending, cleanup, and attempt APIs.
16. Special decision/action/blocker/conflict storage and commands.
17. Record supersession graphs.
18. Rich source-context windows and revision comparison.
19. Detailed capability discovery.
20. Profile attachment and model-approval workflows.
21. A stable public Python SDK.
22. Historical or withdrawn knowledge in Ladybug.
23. Incremental graph projection.
24. Automatic Markdown knowledge extraction.
25. File move detection and implicit directory synchronization.

## 13. Recommended keep-but-simplify list

1. Exact source text -> one immutable revision row.
2. Historical citations -> passage points directly to its revision unless the
   user explicitly removes that source.
3. Source updates -> same source key creates a new revision.
4. Batch writes -> documents commit independently; ordinary knowledge-command
   failures roll back the attempted change and graph candidate, while rare
   finalization crashes are detected through `knowledge_version`.
5. Idempotency -> source key/hash for documents; none for authored facts.
6. Schema evolution -> additive rows plus a monotonic version.
7. Entity typing -> multiple active evidence-backed memberships.
8. Entity identity -> SQLite ID plus transactional exact resolution.
9. Provenance -> `created_at` and optional `created_by`.
10. Knowledge history -> immutable facts plus uniform withdrawal.
11. Search consistency -> one SQLite read transaction.
12. Search readiness -> complete searchable publication within ingestion.
13. Graph readiness -> fresh complete rebuild replacing the old graph.
14. Query safety -> native read-only enforcement, real cancellation, and row
    limits; failure reopens the graph-query architecture.
15. Limits -> a handful of concrete input/output limits.
16. Agent discoverability -> generated boundary schemas and graph schema.

## 14. Audit conclusion

No major capability-audit questions remain. Exact component choices and limits
are bounded implementation decisions and gates recorded in the product design
and class architecture.
