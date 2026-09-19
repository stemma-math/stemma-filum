-- A formalization proposal carries the complete Lean module atomically with
-- the prose source it was reviewed against.
ALTER TABLE proposals ADD COLUMN lean_source TEXT;
