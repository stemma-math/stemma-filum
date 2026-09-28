-- A worker compiles an agent's staged library change before review. Its
-- reconstructible evidence travels with the proposal; a person's later
-- correspondence audit is a separate, durable record.
ALTER TABLE proposals ADD COLUMN formal_review TEXT;

CREATE TABLE formal_audits (
    id               TEXT PRIMARY KEY,
    proposal_id      TEXT NOT NULL REFERENCES proposals (id) ON DELETE CASCADE,
    line_id           TEXT NOT NULL REFERENCES lines (id) ON DELETE CASCADE,
    block_id          TEXT NOT NULL,
    revision          INTEGER NOT NULL,
    block_source      TEXT NOT NULL,
    check_result      TEXT NOT NULL,
    reading           TEXT NOT NULL,
    audited_by        TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    state             TEXT NOT NULL DEFAULT 'audited'
                      CHECK (state IN ('audited', 'audit_withdrawn')),
    withdrawn_reason TEXT,
    audited_at        TEXT NOT NULL DEFAULT (datetime('now')),
    withdrawn_at      TEXT,
    UNIQUE (proposal_id, block_id)
) STRICT;

CREATE INDEX formal_audits_current
    ON formal_audits (line_id, block_id, audited_at DESC);
