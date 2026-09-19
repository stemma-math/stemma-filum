-- Who is writing a line, and from where.
--
-- A claim belongs to a machine and not to an account, because only
-- people's machines write mathematics: two versions of a line are one
-- person on two devices, never two people, and that is what makes a rule
-- enough where a negotiation would otherwise be needed.
--
-- Nothing here holds a line for ever. A claim lapses on its own once the
-- machine stops saying it is still writing, so a shut laptop lets go
-- without anybody having to remember to.

CREATE TABLE claims (
    id         TEXT PRIMARY KEY,
    group_id   TEXT NOT NULL REFERENCES groups (id) ON DELETE CASCADE,
    side       TEXT NOT NULL,
    line       TEXT NOT NULL,
    machine_id TEXT NOT NULL REFERENCES machines (id) ON DELETE CASCADE,
    user_id    TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    taken_at   TEXT NOT NULL DEFAULT (datetime('now')),
    renewed_at TEXT NOT NULL DEFAULT (datetime('now')),
    -- Being asked for the line back is a condition of the claim, shown
    -- where the claim is shown. It is counted nowhere: the only person
    -- waiting is the one already holding it.
    asked_at   TEXT,
    asked_by   TEXT REFERENCES machines (id) ON DELETE SET NULL,
    released_at TEXT
) STRICT;

CREATE UNIQUE INDEX claims_one_per_line
    ON claims (group_id, side, line) WHERE released_at IS NULL;

CREATE INDEX claims_by_machine ON claims (machine_id) WHERE released_at IS NULL;

-- The other version, for a job that brings two of them together. Every
-- other job works against one commit and has nothing to put here.
ALTER TABLE jobs ADD COLUMN against TEXT;
