-- A group says what it is for.
--
-- The reason somebody gave when they asked for the group is that statement
-- already, so an approved request carries its reason across rather than
-- asking the same question twice. Groups made before this have none, and a
-- group without a description is an ordinary group.
ALTER TABLE groups ADD COLUMN description TEXT;
