-- Per-job workspaces, scoped answers, launch selection and cancellation.
ALTER TABLE machine_agents ADD COLUMN models TEXT NOT NULL DEFAULT '[]';
ALTER TABLE machine_agents ADD COLUMN efforts TEXT NOT NULL DEFAULT '[]';

ALTER TABLE jobs ADD COLUMN model TEXT;
ALTER TABLE jobs ADD COLUMN effort TEXT;
ALTER TABLE jobs ADD COLUMN request TEXT;
ALTER TABLE jobs ADD COLUMN grants TEXT;
ALTER TABLE jobs ADD COLUMN workspace TEXT;
ALTER TABLE jobs ADD COLUMN progress TEXT;
ALTER TABLE jobs ADD COLUMN cancel_requested_at TEXT;

ALTER TABLE proposals ADD COLUMN operations TEXT NOT NULL DEFAULT '[]';
ALTER TABLE proposals ADD COLUMN explanation TEXT NOT NULL DEFAULT '';
ALTER TABLE proposals ADD COLUMN effort TEXT;
