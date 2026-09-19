-- Machines that run work, and the work they are given.
--
-- A machine registers once and then serves whichever groups its owner
-- enables. It is its credential: revoking the machine takes the
-- credential back without signing its owner out of anything else.
--
-- The server computes none of this itself. It keeps the queue, hands a
-- job to the machine that was chosen for it, and records the typed
-- outcome that comes back.

CREATE TABLE machines (
    id            TEXT PRIMARY KEY,
    user_id       TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    token_id      TEXT NOT NULL UNIQUE
                       REFERENCES client_tokens (id) ON DELETE CASCADE,
    label         TEXT NOT NULL,
    registered_at TEXT NOT NULL DEFAULT (datetime('now')),
    last_seen_at  TEXT,
    retired_at    TEXT
) STRICT;

CREATE INDEX machines_by_user ON machines (user_id);

-- What the machine found when it looked, not what anybody declared. An
-- agent it does not have has a row here too, saying so: the owner can
-- install it, and an absence with a reason teaches them that.
CREATE TABLE machine_agents (
    machine_id   TEXT NOT NULL REFERENCES machines (id) ON DELETE CASCADE,
    agent        TEXT NOT NULL,
    version      TEXT,
    availability TEXT NOT NULL,
    seen_at      TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (machine_id, agent)
) STRICT;

-- The groups a machine's owner has turned it on for. Registration is
-- once; this is per group, and it is the owner's to change.
CREATE TABLE machine_groups (
    machine_id TEXT NOT NULL REFERENCES machines (id) ON DELETE CASCADE,
    group_id   TEXT NOT NULL REFERENCES groups (id) ON DELETE CASCADE,
    enabled_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (machine_id, group_id)
) STRICT;

CREATE INDEX machine_groups_by_group ON machine_groups (group_id);

-- One unit of work. The machine and the agent were chosen on purpose, so
-- `chosen_machine` is never rewritten: a job whose machine is asleep
-- waits for it. A job that named nobody may be taken by anything that is
-- free and has the agent.
CREATE TABLE jobs (
    id             TEXT PRIMARY KEY,
    group_id       TEXT NOT NULL REFERENCES groups (id) ON DELETE CASCADE,
    side           TEXT NOT NULL,
    line           TEXT NOT NULL,
    commit_id      TEXT NOT NULL,
    task           TEXT NOT NULL,
    agent          TEXT NOT NULL,
    chosen_machine TEXT REFERENCES machines (id) ON DELETE SET NULL,
    launched_by    TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    launched_at    TEXT NOT NULL DEFAULT (datetime('now')),
    state          TEXT NOT NULL DEFAULT 'queued',
    taken_by       TEXT REFERENCES machines (id) ON DELETE SET NULL,
    taken_at       TEXT,
    finished_at    TEXT,
    outcome        TEXT,
    code           TEXT,
    reason         TEXT,
    -- When the machine's owner dealt with a job that could not be tried.
    -- The outcome stays in the record; what clears is the decision it
    -- asked of them.
    cleared_at     TEXT
) STRICT;

CREATE INDEX jobs_waiting ON jobs (launched_at) WHERE state = 'queued';
CREATE INDEX jobs_by_group ON jobs (group_id, launched_at DESC);
CREATE INDEX jobs_by_machine ON jobs (taken_by, launched_at DESC);
