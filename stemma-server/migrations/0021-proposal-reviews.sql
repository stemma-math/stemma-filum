-- A review can move to another group member. Keep the reason and the kind
-- of decision beside the immutable proposal for the line's history.
ALTER TABLE proposals ADD COLUMN reviewer TEXT REFERENCES users (id);
ALTER TABLE proposals ADD COLUMN decision TEXT;
ALTER TABLE proposals ADD COLUMN note TEXT;

CREATE INDEX proposals_by_reviewer ON proposals (reviewer, state, id);
