-- What machines built, so the web can show a line to somebody whose own
-- machine is switched off.
--
-- The artefacts themselves are files outside the repositories, named by
-- the commit they were built from. Everything here can be thrown away
-- and built again, which is what makes them safe to evict.

CREATE TABLE artefacts (
    id         TEXT PRIMARY KEY,
    group_id   TEXT NOT NULL REFERENCES groups (id) ON DELETE CASCADE,
    scope      TEXT NOT NULL,
    kind       TEXT NOT NULL,
    commit_id  TEXT NOT NULL,
    bytes      INTEGER NOT NULL,
    built_by   TEXT NOT NULL REFERENCES users (id),
    built_at   TEXT NOT NULL DEFAULT (datetime('now'))
) STRICT;

-- One artefact of a kind per commit. Building the same commit twice
-- replaces what was there rather than leaving two.
CREATE UNIQUE INDEX artefacts_one
    ON artefacts (group_id, scope, kind, commit_id);

CREATE INDEX artefacts_latest
    ON artefacts (group_id, scope, kind, built_at DESC);
