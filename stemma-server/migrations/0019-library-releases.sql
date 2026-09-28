-- A release is an immutable public projection of one shared-library commit.
-- The source itself lives in the group's public releases.git repository;
-- this row keeps the citation metadata and generated documentation.
CREATE TABLE library_releases (
    group_id       TEXT NOT NULL REFERENCES groups (id) ON DELETE CASCADE,
    version        TEXT NOT NULL,
    source_commit  TEXT NOT NULL,
    release_commit TEXT NOT NULL,
    results        TEXT NOT NULL,
    published_by   TEXT NOT NULL REFERENCES users (id) ON DELETE RESTRICT,
    published_at   TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (group_id, version),
    UNIQUE (group_id, release_commit)
) STRICT;

CREATE INDEX library_releases_by_group
    ON library_releases (group_id, published_at DESC);
