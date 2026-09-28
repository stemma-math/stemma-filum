-- A person or a group can have a picture.
--
-- A picture is small and belongs to one account or one group, so it lives
-- beside them rather than in a store of its own, and goes when they go.
-- Somebody without one is shown by their initials, as before.
CREATE TABLE user_pictures (
    user_id    TEXT PRIMARY KEY REFERENCES users (id) ON DELETE CASCADE,
    media_type TEXT NOT NULL,
    bytes      BLOB NOT NULL,
    changed_at TEXT NOT NULL DEFAULT (datetime('now'))
) STRICT;

CREATE TABLE group_pictures (
    group_id   TEXT PRIMARY KEY REFERENCES groups (id) ON DELETE CASCADE,
    media_type TEXT NOT NULL,
    bytes      BLOB NOT NULL,
    changed_at TEXT NOT NULL DEFAULT (datetime('now'))
) STRICT;
