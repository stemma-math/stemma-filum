-- What changed on the instance, for the machines and windows watching it.
--
-- An event is a notice, not a record: it says what changed and where, and
-- whoever reads it asks for the thing itself. So it carries identifiers
-- and nothing a reader could not already see, and it is thrown away
-- after a while — a reader that was away longer than that starts over.
--
-- Who may read one is written on it. An event with a person on it is
-- theirs alone; otherwise it belongs to everybody in its group.
CREATE TABLE events (
    id       INTEGER PRIMARY KEY AUTOINCREMENT,
    kind     TEXT NOT NULL,
    group_id TEXT REFERENCES groups (id) ON DELETE CASCADE,
    user_id  TEXT REFERENCES users (id) ON DELETE CASCADE,
    subject  TEXT NOT NULL,
    at       TEXT NOT NULL DEFAULT (datetime('now')),
    CHECK (group_id IS NOT NULL OR user_id IS NOT NULL)
) STRICT;

CREATE INDEX events_by_group ON events (group_id, id);
CREATE INDEX events_by_user ON events (user_id, id);
