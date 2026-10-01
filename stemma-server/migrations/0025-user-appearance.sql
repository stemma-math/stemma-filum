-- One appearance choice per account, shared by browser and desktop.
-- NULL means this account has not made a choice yet, so a desktop with an
-- existing local choice can migrate it on first connection.
ALTER TABLE users ADD COLUMN appearance TEXT
    CHECK (appearance IN ('system', 'light', 'dark'));
