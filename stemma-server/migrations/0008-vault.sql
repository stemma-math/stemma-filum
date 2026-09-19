-- The vault is immutable mathematical content plus append-only human records.
CREATE TABLE vault_entries (
    id              TEXT PRIMARY KEY,
    group_id        TEXT NOT NULL REFERENCES groups (id) ON DELETE CASCADE,
    siglum          TEXT NOT NULL,
    name            TEXT NOT NULL,
    prose           TEXT NOT NULL,
    formalization   TEXT NOT NULL,
    correspondence  TEXT NOT NULL DEFAULT 'not_audited',
    admitted_by     TEXT NOT NULL REFERENCES users (id),
    admitted_at     TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE (group_id, siglum)
) STRICT;

CREATE TABLE bridges (
    id              TEXT PRIMARY KEY,
    group_id        TEXT NOT NULL REFERENCES groups (id) ON DELETE CASCADE,
    left_entry      TEXT NOT NULL REFERENCES vault_entries (id),
    right_entry     TEXT NOT NULL REFERENCES vault_entries (id),
    kind            TEXT NOT NULL CHECK (kind IN ('predicate', 'structure')),
    theorem         TEXT NOT NULL,
    origin          TEXT NOT NULL CHECK (origin IN ('direct', 'derived')),
    first_parent    TEXT REFERENCES bridges (id),
    second_parent   TEXT REFERENCES bridges (id),
    correspondence  TEXT NOT NULL DEFAULT 'not_audited',
    created_by      TEXT NOT NULL REFERENCES users (id),
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    CHECK ((origin = 'direct' AND first_parent IS NULL AND second_parent IS NULL)
        OR (origin = 'derived' AND first_parent IS NOT NULL AND second_parent IS NOT NULL)),
    CHECK (left_entry <> right_entry),
    UNIQUE (group_id, left_entry, right_entry, kind)
) STRICT;

CREATE TABLE audits (
    id          TEXT PRIMARY KEY,
    target_kind TEXT NOT NULL CHECK (target_kind IN ('entry', 'bridge')),
    target_id   TEXT NOT NULL,
    action      TEXT NOT NULL CHECK (action IN ('attested', 'withdrawn')),
    by_user     TEXT NOT NULL REFERENCES users (id),
    note        TEXT NOT NULL,
    recorded_at TEXT NOT NULL DEFAULT (datetime('now'))
) STRICT;

CREATE INDEX audits_by_target ON audits (target_kind, target_id, recorded_at);

CREATE TABLE audit_obligations (
    id          TEXT PRIMARY KEY,
    group_id    TEXT NOT NULL REFERENCES groups (id) ON DELETE CASCADE,
    bridge_id   TEXT NOT NULL REFERENCES bridges (id),
    statement   TEXT NOT NULL,
    assigned_to TEXT NOT NULL REFERENCES users (id),
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    decided_at  TEXT,
    UNIQUE (bridge_id, statement, assigned_to)
) STRICT;

CREATE TABLE audit_decisions (
    id            TEXT PRIMARY KEY,
    obligation_id TEXT NOT NULL UNIQUE REFERENCES audit_obligations (id),
    decision      TEXT NOT NULL CHECK (decision IN ('stands', 'rewrite', 'waiting_for_audit')),
    by_user       TEXT NOT NULL REFERENCES users (id),
    note          TEXT NOT NULL,
    decided_at    TEXT NOT NULL DEFAULT (datetime('now'))
) STRICT;
