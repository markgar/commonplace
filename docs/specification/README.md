# Commonplace design specification

Commonplace is a local, evidence-backed knowledge system for personal document
collections. It combines exact source retention, hybrid text retrieval, explicit
knowledge authoring, and graph queries in one command-line application.

This directory is the canonical design document set:

- [Product design](product.md) defines user-visible behavior and product scope.
- [Software architecture](architecture.md) defines component boundaries,
  dependencies, workflows, and failure handling.
- [Persistence design](persistence.md) defines durable state and database
  invariants.
- [Implementation and acceptance](implementation.md) defines delivery order,
  technology gates, and release acceptance.

The documents are intended to be read together. When requirements overlap, the
product design owns behavior, the architecture owns code boundaries, the
persistence design owns durable representation, and the implementation document
owns sequencing and acceptance.

## System summary

The primary product loop is:

```text
ingest sources
    -> retrieve exact passages
    -> interpret evidence
    -> author cited knowledge
    -> query the knowledge graph
    -> verify claims against sources
```

Commonplace has two complementary query paths:

1. **Hybrid search** finds relevant passages using lexical retrieval, vector
   retrieval, deterministic fusion, and local reranking.
2. **Graph query** executes read-only Cypher over explicitly authored entities,
   types, relationships, and literal facts.

Both paths return canonical identifiers that resolve to exact evidence in
SQLite.

## Core invariants

The following rules apply throughout the design:

1. SQLite is the authoritative durable store.
2. Source revisions and passages preserve exact UTF-8 text.
3. Every citation resolves to a canonical passage and source revision.
4. Authored knowledge changes only through explicit record, withdraw, and source
   removal operations.
5. Grafeo contains a complete, derived projection of active knowledge and can
   be rebuilt from SQLite.
6. Graph reads fail when the graph version does not match SQLite.
7. Ingestion is batch-first, synchronous, bounded in memory, and independently
   transactional per document.
8. Knowledge authoring and withdrawal roll back with graph-build failures and
   use version checks to detect the narrow graph-activation/SQLite-commit crash
   window.
9. The CLI JSON interface is the stable programmatic contract.
10. Local source text is never sent to a remote inference service.

## Product boundaries

Commonplace is designed for one person operating one local knowledge base. It
does not provide a hosted service, collaboration, user authorization, automatic
knowledge extraction, natural-language query routing, or background job
orchestration.

The design favors direct implementations and small replaceable boundaries.
Features are included when they enable ingestion, retrieval, evidence
verification, knowledge authoring, graph projection, or graph querying.
