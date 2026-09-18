-- Accounts and the grants that let somebody operate the instance.
--
-- An account is global to the server and is never scoped to a group: a
-- researcher collaborates with several groups and is one person. The group
-- is a scope chosen after entering, not part of the login.

CREATE TABLE users (
    id            TEXT PRIMARY KEY,
    slug          TEXT NOT NULL UNIQUE,
    email         TEXT NOT NULL UNIQUE,
    display_name  TEXT NOT NULL,
    password_hash TEXT NOT NULL,
    created_at    TEXT NOT NULL DEFAULT (datetime('now')),
    suspended_at  TEXT
) STRICT;

-- Operating the instance is a grant with a history, never a column on a
-- profile. Revoking sets `revoked_at` and keeps the row, so the record of
-- who could do what, and when, survives.
CREATE TABLE instance_grants (
    id         TEXT PRIMARY KEY,
    user_id    TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    kind       TEXT NOT NULL,
    granted_at TEXT NOT NULL DEFAULT (datetime('now')),
    granted_by TEXT REFERENCES users (id),
    revoked_at TEXT,
    revoked_by TEXT REFERENCES users (id)
) STRICT;

CREATE INDEX instance_grants_by_user ON instance_grants (user_id);

-- One live grant of a kind per account. Revoked ones are outside the index,
-- so the same grant can be given again later and both acts are recorded.
CREATE UNIQUE INDEX instance_grants_live
    ON instance_grants (user_id, kind)
    WHERE revoked_at IS NULL;
