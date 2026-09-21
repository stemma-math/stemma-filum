-- Each personal repository has its own namespace. Shared lines retain one
-- name per group; personal lines and claims also belong to their owner.
CREATE TABLE scoped_lines (
    id         TEXT PRIMARY KEY,
    group_id   TEXT NOT NULL REFERENCES groups (id) ON DELETE CASCADE,
    side       TEXT NOT NULL CHECK (side IN ('shared', 'private')),
    name       TEXT NOT NULL,
    author     TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    started_at TEXT NOT NULL DEFAULT (datetime('now')),
    writing    TEXT NOT NULL DEFAULT 'anyone',
    shared_at  TEXT
) STRICT;

INSERT INTO scoped_lines SELECT * FROM lines;
DROP TABLE lines;
ALTER TABLE scoped_lines RENAME TO lines;

CREATE UNIQUE INDEX lines_shared_name ON lines (group_id, side, name)
    WHERE side = 'shared';
CREATE UNIQUE INDEX lines_private_name ON lines (group_id, side, name, author)
    WHERE side = 'private';
CREATE INDEX lines_by_group ON lines (group_id, side);

DROP INDEX claims_one_per_line;
CREATE UNIQUE INDEX claims_one_per_shared_line ON claims (group_id, side, line)
    WHERE released_at IS NULL AND side = 'shared';
CREATE UNIQUE INDEX claims_one_per_private_line ON claims (group_id, side, line, user_id)
    WHERE released_at IS NULL AND side = 'private';
