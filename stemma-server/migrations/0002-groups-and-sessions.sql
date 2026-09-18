-- Groups, the ways into them, and the sessions people hold.

CREATE TABLE groups (
    id          TEXT PRIMARY KEY,
    slug        TEXT NOT NULL UNIQUE,
    name        TEXT NOT NULL,
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    archived_at TEXT
) STRICT;

-- A role is held inside a group. The same person holds different ones in
-- different groups, and holds none at the instance level: that is a grant.
CREATE TABLE memberships (
    group_id  TEXT NOT NULL REFERENCES groups (id) ON DELETE CASCADE,
    user_id   TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    role      TEXT NOT NULL,
    joined_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (group_id, user_id)
) STRICT;

CREATE INDEX memberships_by_user ON memberships (user_id);

-- Groups are requested and approved, not created. The request stays after
-- the decision, with who decided and when.
CREATE TABLE group_requests (
    id           TEXT PRIMARY KEY,
    slug         TEXT NOT NULL,
    name         TEXT NOT NULL,
    reason       TEXT NOT NULL,
    requested_by TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    requested_at TEXT NOT NULL DEFAULT (datetime('now')),
    outcome      TEXT,
    decided_at   TEXT,
    decided_by   TEXT REFERENCES users (id),
    note         TEXT
) STRICT;

CREATE INDEX group_requests_open ON group_requests (requested_at)
    WHERE outcome IS NULL;

-- An invitation is always a link, bound to one group and one role, single
-- use, expiring and revocable. Mail, where it is configured, delivers the
-- same link. Only the hash is kept: the link itself is shown once, to
-- whoever made it.
CREATE TABLE invitations (
    id          TEXT PRIMARY KEY,
    group_id    TEXT NOT NULL REFERENCES groups (id) ON DELETE CASCADE,
    role        TEXT NOT NULL,
    email       TEXT,
    token_hash  TEXT NOT NULL UNIQUE,
    created_by  TEXT NOT NULL REFERENCES users (id),
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    expires_at  TEXT NOT NULL,
    accepted_at TEXT,
    accepted_by TEXT REFERENCES users (id),
    revoked_at  TEXT,
    revoked_by  TEXT REFERENCES users (id)
) STRICT;

CREATE INDEX invitations_by_group ON invitations (group_id);

-- A session is an opaque secret the browser holds and a hash the server
-- keeps. `reauthenticated_at` is what the admin area asks about: holding a
-- session is not the same as having just proved who you are.
CREATE TABLE sessions (
    id                 TEXT PRIMARY KEY,
    user_id            TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    token_hash         TEXT NOT NULL,
    csrf_token         TEXT NOT NULL,
    created_at         TEXT NOT NULL DEFAULT (datetime('now')),
    last_seen_at       TEXT NOT NULL DEFAULT (datetime('now')),
    expires_at         TEXT NOT NULL,
    reauthenticated_at TEXT
) STRICT;

CREATE INDEX sessions_by_user ON sessions (user_id);

-- Without outgoing mail a reset link is the only way back into an account,
-- so it exists whether or not there is mail to send it with.
CREATE TABLE password_resets (
    id         TEXT PRIMARY KEY,
    user_id    TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    token_hash TEXT NOT NULL UNIQUE,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    expires_at TEXT NOT NULL,
    used_at    TEXT
) STRICT;

-- Policy, as opposed to infrastructure: read per request, changed from the
-- admin area while the server runs, and recorded with who changed it.
CREATE TABLE settings (
    key        TEXT PRIMARY KEY,
    value      TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_by TEXT REFERENCES users (id)
) STRICT;
