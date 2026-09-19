-- The lines the instance knows were started, and who writes them.
--
-- A line is its directory in a repository, and nothing here replaces
-- that: the title, the question and the mathematics all live in the
-- source where their author can see and edit them. What is kept here is
-- what git cannot answer — who started it, and whether they kept the
-- writing to themselves when they shared it.

CREATE TABLE lines (
    id         TEXT PRIMARY KEY,
    group_id   TEXT NOT NULL REFERENCES groups (id) ON DELETE CASCADE,
    side       TEXT NOT NULL,
    name       TEXT NOT NULL,
    author     TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    started_at TEXT NOT NULL DEFAULT (datetime('now')),
    -- `author` or `anyone`. A line nobody shared is its author's alone
    -- whatever this says, because nobody else can reach the repository.
    writing    TEXT NOT NULL DEFAULT 'anyone',
    shared_at  TEXT,
    UNIQUE (group_id, side, name)
) STRICT;

CREATE INDEX lines_by_group ON lines (group_id, side);
