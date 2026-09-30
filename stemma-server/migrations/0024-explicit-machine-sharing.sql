-- Serving a group lets the owner use this machine for its work. Other
-- members may launch jobs on it only after its owner shares it explicitly.
ALTER TABLE machine_groups ADD COLUMN shared INTEGER NOT NULL DEFAULT 0 CHECK (shared IN (0, 1));
