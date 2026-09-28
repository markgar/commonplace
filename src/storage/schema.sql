CREATE TABLE store_state (
    singleton         INTEGER PRIMARY KEY CHECK (singleton = 1),
    format            TEXT NOT NULL CHECK (format = 'commonplace-store/2'),
    schema_version    INTEGER NOT NULL DEFAULT 0 CHECK (schema_version >= 0),
    knowledge_version INTEGER NOT NULL DEFAULT 0 CHECK (knowledge_version >= 0),
    created_at        TEXT NOT NULL
) STRICT;

CREATE TABLE documents (
    document_id      INTEGER PRIMARY KEY AUTOINCREMENT,
    source_key       TEXT NOT NULL UNIQUE CHECK (length(source_key) > 0),
    created_at       TEXT NOT NULL,
    last_ingested_at TEXT NOT NULL
) STRICT;

CREATE TABLE document_revisions (
    revision_id      INTEGER PRIMARY KEY AUTOINCREMENT,
    document_id      INTEGER NOT NULL
                     REFERENCES documents(document_id) ON DELETE CASCADE,
    revision_number  INTEGER NOT NULL CHECK (revision_number > 0),
    revision_digest  TEXT NOT NULL CHECK (length(revision_digest) = 64),
    text             TEXT NOT NULL,
    title            TEXT,
    source_type      TEXT NOT NULL CHECK (length(source_type) > 0),
    occurred_at      TEXT,
    metadata_json    TEXT NOT NULL CHECK (
                         json_valid(metadata_json)
                         AND json_type(metadata_json) = 'object'
                     ),
    created_at       TEXT NOT NULL,
    UNIQUE (document_id, revision_number)
) STRICT;

CREATE TABLE passages (
    passage_id   INTEGER PRIMARY KEY AUTOINCREMENT,
    revision_id  INTEGER NOT NULL
                 REFERENCES document_revisions(revision_id) ON DELETE CASCADE,
    ordinal      INTEGER NOT NULL CHECK (ordinal >= 0),
    start_byte   INTEGER NOT NULL CHECK (start_byte >= 0),
    end_byte     INTEGER NOT NULL CHECK (end_byte >= start_byte),
    text         TEXT NOT NULL,
    UNIQUE (revision_id, ordinal),
    UNIQUE (revision_id, start_byte, end_byte)
) STRICT;

CREATE VIRTUAL TABLE passage_fts USING fts5(
    text,
    content = '',
    contentless_delete = 1,
    tokenize = 'unicode61'
);

CREATE VIRTUAL TABLE passage_vectors USING vec0(
    passage_id INTEGER PRIMARY KEY,
    embedding  FLOAT[384]
);

CREATE TABLE entity_types (
    entity_type_id     INTEGER PRIMARY KEY AUTOINCREMENT,
    name               TEXT NOT NULL UNIQUE CHECK (length(name) > 0),
    description        TEXT,
    introduced_version INTEGER NOT NULL CHECK (introduced_version > 0)
) STRICT;

CREATE TABLE identifier_schemes (
    identifier_scheme_id INTEGER PRIMARY KEY AUTOINCREMENT,
    name                 TEXT NOT NULL UNIQUE CHECK (length(name) > 0),
    description          TEXT,
    introduced_version   INTEGER NOT NULL CHECK (introduced_version > 0)
) STRICT;

CREATE TABLE predicates (
    predicate_id       INTEGER PRIMARY KEY AUTOINCREMENT,
    name               TEXT NOT NULL UNIQUE CHECK (length(name) > 0),
    object_kind        TEXT NOT NULL CHECK (
                           object_kind IN (
                               'entity', 'string', 'integer',
                               'boolean', 'timestamp'
                           )
                       ),
    description        TEXT,
    introduced_version INTEGER NOT NULL CHECK (introduced_version > 0)
) STRICT;

CREATE TABLE predicate_entity_types (
    predicate_id   INTEGER NOT NULL
                   REFERENCES predicates(predicate_id) ON DELETE RESTRICT,
    role           TEXT NOT NULL CHECK (role IN ('subject', 'object')),
    entity_type_id INTEGER NOT NULL
                   REFERENCES entity_types(entity_type_id) ON DELETE RESTRICT,
    PRIMARY KEY (predicate_id, role, entity_type_id)
) STRICT;

CREATE TABLE entities (
    entity_id      INTEGER PRIMARY KEY AUTOINCREMENT,
    canonical_name TEXT NOT NULL CHECK (length(canonical_name) > 0),
    created_at     TEXT NOT NULL,
    created_by     TEXT
) STRICT;

CREATE TABLE entity_aliases (
    entity_id  INTEGER NOT NULL
               REFERENCES entities(entity_id) ON DELETE RESTRICT,
    alias      TEXT NOT NULL CHECK (length(alias) > 0),
    created_at TEXT NOT NULL,
    created_by TEXT,
    PRIMARY KEY (entity_id, alias)
) STRICT;

CREATE TABLE entity_identifiers (
    entity_id            INTEGER NOT NULL
                         REFERENCES entities(entity_id) ON DELETE RESTRICT,
    identifier_scheme_id INTEGER NOT NULL
                         REFERENCES identifier_schemes(identifier_scheme_id)
                         ON DELETE RESTRICT,
    value                TEXT NOT NULL CHECK (length(value) > 0),
    created_at            TEXT NOT NULL,
    created_by            TEXT,
    PRIMARY KEY (identifier_scheme_id, value)
) STRICT;

CREATE TABLE knowledge_items (
    knowledge_item_id INTEGER PRIMARY KEY AUTOINCREMENT,
    kind              TEXT NOT NULL CHECK (
                          kind IN ('type_membership', 'fact')
                      ),
    schema_version    INTEGER NOT NULL CHECK (schema_version > 0),
    created_at        TEXT NOT NULL,
    created_by        TEXT,
    withdrawn_at      TEXT,
    withdrawn_by      TEXT,
    CHECK (withdrawn_at IS NOT NULL OR withdrawn_by IS NULL)
) STRICT;

CREATE TABLE entity_type_memberships (
    knowledge_item_id INTEGER PRIMARY KEY
                      REFERENCES knowledge_items(knowledge_item_id)
                      ON DELETE CASCADE,
    entity_id         INTEGER NOT NULL
                      REFERENCES entities(entity_id) ON DELETE RESTRICT,
    entity_type_id    INTEGER NOT NULL
                      REFERENCES entity_types(entity_type_id) ON DELETE RESTRICT
) STRICT;

CREATE TABLE facts (
    knowledge_item_id INTEGER PRIMARY KEY
                      REFERENCES knowledge_items(knowledge_item_id)
                      ON DELETE CASCADE,
    subject_entity_id INTEGER NOT NULL
                      REFERENCES entities(entity_id) ON DELETE RESTRICT,
    predicate_id      INTEGER NOT NULL
                      REFERENCES predicates(predicate_id) ON DELETE RESTRICT,
    object_entity_id  INTEGER
                      REFERENCES entities(entity_id) ON DELETE RESTRICT,
    literal_json      TEXT CHECK (
                          literal_json IS NULL OR json_valid(literal_json)
                      ),
    CHECK (
        (object_entity_id IS NOT NULL AND literal_json IS NULL)
        OR
        (object_entity_id IS NULL AND literal_json IS NOT NULL)
    )
) STRICT;

CREATE TABLE knowledge_item_evidence (
    knowledge_item_id INTEGER NOT NULL
                      REFERENCES knowledge_items(knowledge_item_id)
                      ON DELETE CASCADE,
    passage_id        INTEGER NOT NULL
                      REFERENCES passages(passage_id) ON DELETE CASCADE,
    PRIMARY KEY (knowledge_item_id, passage_id)
) STRICT;

CREATE INDEX entity_name_idx
    ON entities(canonical_name, entity_id);

CREATE INDEX entity_alias_value_idx
    ON entity_aliases(alias, entity_id);

CREATE INDEX entity_identifier_entity_idx
    ON entity_identifiers(entity_id, identifier_scheme_id, value);

CREATE INDEX knowledge_item_active_idx
    ON knowledge_items(knowledge_item_id)
    WHERE withdrawn_at IS NULL;

CREATE INDEX membership_entity_idx
    ON entity_type_memberships(entity_id, entity_type_id, knowledge_item_id);

CREATE INDEX membership_type_idx
    ON entity_type_memberships(entity_type_id, entity_id, knowledge_item_id);

CREATE INDEX fact_subject_idx
    ON facts(subject_entity_id, predicate_id, knowledge_item_id);

CREATE INDEX fact_object_idx
    ON facts(object_entity_id, predicate_id, knowledge_item_id)
    WHERE object_entity_id IS NOT NULL;

CREATE INDEX evidence_passage_idx
    ON knowledge_item_evidence(passage_id, knowledge_item_id);
