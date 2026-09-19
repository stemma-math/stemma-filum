-- Reviewable changes produced by jobs. Their source is operational until a
-- person accepts it; the accepted mathematics is still written and committed
-- on that person's machine, never by the server.
CREATE TABLE proposals (
    id              TEXT PRIMARY KEY,
    job_id          TEXT NOT NULL UNIQUE REFERENCES jobs (id) ON DELETE CASCADE,
    source          TEXT NOT NULL,
    vendor          TEXT NOT NULL,
    model           TEXT NOT NULL,
    prompt_revision TEXT NOT NULL,
    state           TEXT NOT NULL DEFAULT 'awaiting_review',
    decided_by      TEXT REFERENCES users (id),
    decided_at      TEXT
) STRICT;

CREATE INDEX proposals_by_state ON proposals (state, id);
