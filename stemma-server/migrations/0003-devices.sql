-- Signing in on a machine that has no browser.
--
-- The program on the machine asks for a code, a person types that code
-- into a browser they are already signed in on, and the machine then
-- collects a credential of its own. It works over SSH, which a redirect
-- back to localhost does not.

CREATE TABLE client_tokens (
    id           TEXT PRIMARY KEY,
    user_id      TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    label        TEXT NOT NULL,
    token_hash   TEXT NOT NULL UNIQUE,
    created_at   TEXT NOT NULL DEFAULT (datetime('now')),
    last_seen_at TEXT,
    revoked_at   TEXT
) STRICT;

CREATE INDEX client_tokens_by_user ON client_tokens (user_id);

-- One attempt to sign a machine in. The code a person reads is kept as
-- written, because they have to be able to find their own attempt among
-- several; the code the machine holds is kept only as a hash.
CREATE TABLE device_grants (
    id               TEXT PRIMARY KEY,
    device_code_hash TEXT NOT NULL UNIQUE,
    user_code        TEXT NOT NULL UNIQUE,
    label            TEXT NOT NULL,
    created_at       TEXT NOT NULL DEFAULT (datetime('now')),
    expires_at       TEXT NOT NULL,
    approved_at      TEXT,
    approved_by      TEXT REFERENCES users (id),
    denied_at        TEXT,
    token_id         TEXT REFERENCES client_tokens (id)
) STRICT;
