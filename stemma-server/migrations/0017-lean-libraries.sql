-- Approved agent changes to a Lake package are kept with the proposal that
-- received the human signature. The server applies them directly to git;
-- machines only mirror and check the result.
ALTER TABLE proposals ADD COLUMN library_change TEXT;
ALTER TABLE proposals ADD COLUMN library_commit TEXT;
