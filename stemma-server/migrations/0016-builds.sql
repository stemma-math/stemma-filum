-- What a stretch of writing came to is built by a machine, never here:
-- the machine that wrote it builds it when it is over, and publishes the
-- result. A stretch is built once something published covers its last
-- revision. One that nothing covers a while after it ended is asked of
-- another machine of the group, as a job, and remembers which.
ALTER TABLE writing_sessions ADD COLUMN built INTEGER NOT NULL DEFAULT 0;
ALTER TABLE writing_sessions ADD COLUMN build_job TEXT REFERENCES jobs (id) ON DELETE SET NULL;

-- Nothing before this is owed a build.
UPDATE writing_sessions SET built = 1;
