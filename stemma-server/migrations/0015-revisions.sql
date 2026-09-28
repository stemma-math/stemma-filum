-- Every line is written through the instance, one revision at a time.
--
-- A revision is a whole line as somebody left it, with the page it
-- typesets to: machines render, the instance never compiles, so a page
-- travels with every revision that changes what a line says. Revisions
-- are numbered per line and never reused, and a write names the revision
-- it was made from, so two machines can never both think they wrote the
-- next one.
--
-- A line keeps its identifier for its whole life, through being shared
-- and through being deleted: a deleted line is a revision that says so,
-- and its row stays with the date it went. Its name is free again.
ALTER TABLE lines ADD COLUMN revision INTEGER NOT NULL DEFAULT 0;
ALTER TABLE lines ADD COLUMN committed INTEGER NOT NULL DEFAULT 0;
ALTER TABLE lines ADD COLUMN deleted_at TEXT;

DROP INDEX lines_shared_name;
DROP INDEX lines_private_name;
CREATE UNIQUE INDEX lines_shared_name ON lines (group_id, side, name)
    WHERE side = 'shared' AND deleted_at IS NULL;
CREATE UNIQUE INDEX lines_private_name ON lines (group_id, side, name, author)
    WHERE side = 'private' AND deleted_at IS NULL;

-- A stretch of one machine writing one line. It ends when the line is let
-- go, when another machine writes it, or after a while with nothing
-- written, and git then gets one commit for all of it.
CREATE TABLE writing_sessions (
    id         TEXT PRIMARY KEY,
    line_id    TEXT NOT NULL REFERENCES lines (id) ON DELETE CASCADE,
    machine_id TEXT REFERENCES machines (id) ON DELETE SET NULL,
    user_id    TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    opened_at  TEXT NOT NULL DEFAULT (datetime('now')),
    last_at    TEXT NOT NULL DEFAULT (datetime('now')),
    closed_at  TEXT,
    commit_id  TEXT
) STRICT;

CREATE UNIQUE INDEX writing_sessions_open ON writing_sessions (line_id)
    WHERE closed_at IS NULL;

CREATE TABLE revisions (
    line_id     TEXT NOT NULL REFERENCES lines (id) ON DELETE CASCADE,
    number      INTEGER NOT NULL,
    kind        TEXT NOT NULL CHECK (kind IN ('started', 'written', 'shared', 'deleted')),
    side        TEXT NOT NULL CHECK (side IN ('shared', 'private')),
    source      TEXT NOT NULL,
    lean_source TEXT,
    page        TEXT NOT NULL,
    author      TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    machine_id  TEXT REFERENCES machines (id) ON DELETE SET NULL,
    session_id  TEXT REFERENCES writing_sessions (id) ON DELETE SET NULL,
    written_at  TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (line_id, number)
) STRICT;

-- A job works against a revision of its line, not a commit: the machine
-- that runs it asks the instance for that revision.
ALTER TABLE jobs RENAME COLUMN commit_id TO revision;

-- And the line itself, by identifier, so the job still finds it after it
-- moved to the group.
ALTER TABLE jobs ADD COLUMN line_id TEXT REFERENCES lines (id) ON DELETE SET NULL;

-- A proposal made against a revision some of whose blocks have changed
-- since is outdated rather than accepted: its reviewer says so, and it
-- stays in the record with that.
-- An accepted proposal is applied by its reviewer's own machine, which
-- merges it into the line and writes it; the revision it became is kept
-- here, and a proposal waiting for this is one whose column is empty.
ALTER TABLE proposals ADD COLUMN applied_revision INTEGER;
