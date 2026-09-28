//! The pictures people and groups show themselves by.
//!
//! A picture is checked for what it is before it is kept: the bytes must
//! open as one of the kinds a browser shows everywhere, whatever the upload
//! said it was, and it must be small. It is served back as exactly the
//! kind that was found.

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};
use stemma_core::id::{GroupId, UserId};

/// The largest picture kept, in bytes.
pub const LARGEST: usize = 1024 * 1024;

/// A picture as it is kept and served.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picture {
    /// What the bytes are, as a media type.
    pub media_type: &'static str,
    /// The image itself.
    pub bytes: Vec<u8>,
    /// When it was last changed, which is also what makes its address new.
    pub changed_at: String,
}

/// Why an upload is not kept.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Refused {
    /// Nothing was chosen.
    #[error("Choose a picture to upload.")]
    Empty,
    /// It is over [`LARGEST`].
    #[error("The picture is larger than 1 MB. Choose a smaller one.")]
    TooLarge,
    /// It is not a PNG, a JPEG or a WebP image.
    #[error("The picture must be a PNG, JPEG or WebP image.")]
    NotAPicture,
}

/// What the bytes are, when they are a picture this instance keeps.
///
/// Read from the bytes, never from the name or the type the browser sent:
/// those are what the uploader says, and this is what it is.
pub fn kind_of(bytes: &[u8]) -> Option<&'static str> {
    match bytes {
        [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, ..] => Some("image/png"),
        [0xff, 0xd8, 0xff, ..] => Some("image/jpeg"),
        [
            b'R',
            b'I',
            b'F',
            b'F',
            _,
            _,
            _,
            _,
            b'W',
            b'E',
            b'B',
            b'P',
            ..,
        ] => Some("image/webp"),
        _ => None,
    }
}

/// Checks an upload and says what it is.
pub fn check(bytes: &[u8]) -> Result<&'static str, Refused> {
    if bytes.is_empty() {
        return Err(Refused::Empty);
    }
    if bytes.len() > LARGEST {
        return Err(Refused::TooLarge);
    }

    kind_of(bytes).ok_or(Refused::NotAPicture)
}

/// Whose picture it is: the table it is in and the key it is under.
enum Owner {
    User(String),
    Group(String),
}

impl Owner {
    fn table(&self) -> (&'static str, &'static str) {
        match self {
            Self::User(_) => ("user_pictures", "user_id"),
            Self::Group(_) => ("group_pictures", "group_id"),
        }
    }

    fn key(&self) -> &str {
        match self {
            Self::User(key) | Self::Group(key) => key,
        }
    }
}

fn read(connection: &Connection, owner: &Owner) -> Result<Option<Picture>> {
    let (table, column) = owner.table();

    connection
        .query_row(
            &format!("SELECT bytes, changed_at FROM {table} WHERE {column} = ?1"),
            [owner.key()],
            |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .context("cannot read the picture")?
        .map(|(bytes, changed_at)| {
            // The kind is read again from the bytes rather than trusted
            // from the column, so what is served is what is there.
            let media_type = kind_of(&bytes).context("a kept picture is not a picture")?;

            Ok(Picture {
                media_type,
                bytes,
                changed_at,
            })
        })
        .transpose()
}

fn when(connection: &Connection, owner: &Owner) -> Result<Option<String>> {
    let (table, column) = owner.table();

    connection
        .query_row(
            &format!("SELECT changed_at FROM {table} WHERE {column} = ?1"),
            [owner.key()],
            |row| row.get(0),
        )
        .optional()
        .context("cannot read the picture")
}

fn keep(connection: &Connection, owner: &Owner, bytes: &[u8]) -> Result<Result<(), Refused>> {
    let media_type = match check(bytes) {
        Ok(media_type) => media_type,
        Err(refused) => return Ok(Err(refused)),
    };
    let (table, column) = owner.table();

    connection
        .execute(
            &format!(
                "INSERT INTO {table} ({column}, media_type, bytes) VALUES (?1, ?2, ?3)
                 ON CONFLICT ({column}) DO UPDATE
                    SET media_type = excluded.media_type,
                        bytes = excluded.bytes,
                        changed_at = datetime('now')"
            ),
            params![owner.key(), media_type, bytes],
        )
        .context("cannot keep the picture")?;

    Ok(Ok(()))
}

fn remove(connection: &Connection, owner: &Owner) -> Result<()> {
    let (table, column) = owner.table();

    connection
        .execute(
            &format!("DELETE FROM {table} WHERE {column} = ?1"),
            [owner.key()],
        )
        .context("cannot remove the picture")?;

    Ok(())
}

/// Somebody's picture, if they have one.
pub fn of_user(connection: &Connection, user: &UserId) -> Result<Option<Picture>> {
    read(connection, &Owner::User(user.to_string()))
}

/// When somebody's picture last changed, if they have one.
pub fn user_changed(connection: &Connection, user: &UserId) -> Result<Option<String>> {
    when(connection, &Owner::User(user.to_string()))
}

/// Keeps somebody's picture, replacing the one they had.
pub fn set_user(
    connection: &Connection,
    user: &UserId,
    bytes: &[u8],
) -> Result<Result<(), Refused>> {
    keep(connection, &Owner::User(user.to_string()), bytes)
}

/// Takes somebody's picture away; their initials show again.
pub fn remove_user(connection: &Connection, user: &UserId) -> Result<()> {
    remove(connection, &Owner::User(user.to_string()))
}

/// A group's picture, if it has one.
pub fn of_group(connection: &Connection, group: &GroupId) -> Result<Option<Picture>> {
    read(connection, &Owner::Group(group.to_string()))
}

/// When a group's picture last changed, if it has one.
pub fn group_changed(connection: &Connection, group: &GroupId) -> Result<Option<String>> {
    when(connection, &Owner::Group(group.to_string()))
}

/// Keeps a group's picture, replacing the one it had.
pub fn set_group(
    connection: &Connection,
    group: &GroupId,
    bytes: &[u8],
) -> Result<Result<(), Refused>> {
    keep(connection, &Owner::Group(group.to_string()), bytes)
}

/// Takes a group's picture away; its initials show again.
pub fn remove_group(connection: &Connection, group: &GroupId) -> Result<()> {
    remove(connection, &Owner::Group(group.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::test_database;
    use crate::store::{groups, users};
    use stemma_core::id::Slug;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n rest of the image";
    const JPEG: &[u8] = b"\xff\xd8\xff\xe0 rest of the image";
    const WEBP: &[u8] = b"RIFF\x10\0\0\0WEBPVP8 rest";

    fn ana(connection: &Connection) -> UserId {
        users::create(
            connection,
            &Slug::parse("ana").unwrap(),
            "ana@example.org",
            "Ana",
            "hash",
        )
        .unwrap()
        .unwrap()
        .id
    }

    #[test]
    fn a_picture_is_known_by_its_bytes() {
        assert_eq!(kind_of(PNG), Some("image/png"));
        assert_eq!(kind_of(JPEG), Some("image/jpeg"));
        assert_eq!(kind_of(WEBP), Some("image/webp"));
        assert_eq!(kind_of(b"<svg xmlns='http://www.w3.org/2000/svg'/>"), None);
        assert_eq!(kind_of(b"GIF89a"), None);
    }

    #[test]
    fn an_empty_a_large_or_a_strange_upload_is_refused() {
        assert_eq!(check(b""), Err(Refused::Empty));
        assert_eq!(check(b"<html>"), Err(Refused::NotAPicture));

        let mut large = PNG.to_vec();
        large.resize(LARGEST + 1, 0);
        assert_eq!(check(&large), Err(Refused::TooLarge));
    }

    #[test]
    fn a_picture_is_kept_replaced_and_taken_away() {
        let database = test_database();
        let connection = database.connection();
        let user = ana(connection);

        assert_eq!(of_user(connection, &user).unwrap(), None);

        set_user(connection, &user, PNG).unwrap().unwrap();
        assert_eq!(
            of_user(connection, &user).unwrap().unwrap().media_type,
            "image/png"
        );

        set_user(connection, &user, JPEG).unwrap().unwrap();
        let kept = of_user(connection, &user).unwrap().unwrap();
        assert_eq!(
            (kept.media_type, kept.bytes.as_slice()),
            ("image/jpeg", JPEG)
        );

        assert_eq!(
            set_user(connection, &user, b"not a picture").unwrap(),
            Err(Refused::NotAPicture)
        );
        assert_eq!(of_user(connection, &user).unwrap().unwrap().bytes, JPEG);

        remove_user(connection, &user).unwrap();
        assert_eq!(of_user(connection, &user).unwrap(), None);
        assert_eq!(user_changed(connection, &user).unwrap(), None);
    }

    #[test]
    fn a_group_has_a_picture_of_its_own() {
        let mut database = test_database();
        let user = ana(database.connection());
        let group = groups::create(
            database.connection_mut(),
            &Slug::parse("algebra").unwrap(),
            "Algebra",
            &user,
        )
        .unwrap();
        let connection = database.connection();

        set_group(connection, &group.id, WEBP).unwrap().unwrap();

        assert_eq!(
            of_group(connection, &group.id).unwrap().unwrap().media_type,
            "image/webp"
        );
        assert_eq!(of_user(connection, &user).unwrap(), None);
        assert!(group_changed(connection, &group.id).unwrap().is_some());
    }
}
