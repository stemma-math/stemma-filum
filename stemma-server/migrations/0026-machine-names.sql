-- A machine's name is its owner's to choose, and a name they chose is
-- kept when the machine registers again.
--
-- A machine also carries a key kept on the computer it runs on, so that
-- signing in again from the same computer finds the same machine instead
-- of making a second one.
ALTER TABLE machines ADD COLUMN named_by_owner INTEGER NOT NULL DEFAULT 0;
ALTER TABLE machines ADD COLUMN device TEXT;

CREATE UNIQUE INDEX machines_by_device ON machines (user_id, device)
    WHERE device IS NOT NULL;
