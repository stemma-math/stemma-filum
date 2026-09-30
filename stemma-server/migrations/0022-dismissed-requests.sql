-- Whoever asked for a group puts a declined answer away once they have read
-- it. The request stays in the record.
ALTER TABLE group_requests ADD COLUMN dismissed_at TEXT;
