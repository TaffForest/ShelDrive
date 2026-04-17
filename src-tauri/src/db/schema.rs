use rusqlite::Connection;

const SCHEMA_VERSION: i32 = 2;

pub fn initialize(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch("PRAGMA journal_mode=WAL;")?;
    conn.execute_batch("PRAGMA foreign_keys=ON;")?;

    let version: i32 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap_or(0);

    if version < 1 {
        migrate_v1(conn)?;
    }
    if version < 2 {
        migrate_v2(conn)?;
    }

    conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    Ok(())
}

fn migrate_v1(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS directories (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            path       TEXT    NOT NULL UNIQUE,
            created_at TEXT    NOT NULL
        );

        CREATE TABLE IF NOT EXISTS files (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            path        TEXT    NOT NULL UNIQUE,
            cid         TEXT    NOT NULL,
            size_bytes  INTEGER NOT NULL,
            mime_type   TEXT,
            pinned_at   TEXT    NOT NULL,
            modified_at TEXT    NOT NULL,
            metadata    TEXT
        );

        CREATE INDEX IF NOT EXISTS idx_files_cid ON files(cid);
        CREATE INDEX IF NOT EXISTS idx_files_path ON files(path);
        CREATE INDEX IF NOT EXISTS idx_directories_path ON directories(path);

        -- Seed root directory
        INSERT OR IGNORE INTO directories (path, created_at)
        VALUES ('/', datetime('now'));
        ",
    )?;
    Ok(())
}

fn migrate_v2(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "
        -- Per-folder encryption keys
        CREATE TABLE IF NOT EXISTS folder_keys (
            folder_path       TEXT    PRIMARY KEY,
            encrypted_key     TEXT    NOT NULL,
            owner_address     TEXT    NOT NULL,
            created_at        TEXT    NOT NULL,
            rotated_at        TEXT
        );

        -- Shared folder members: each row is a member's encrypted copy of the folder key
        CREATE TABLE IF NOT EXISTS shared_members (
            id                INTEGER PRIMARY KEY AUTOINCREMENT,
            folder_path       TEXT    NOT NULL,
            member_address    TEXT    NOT NULL,
            encrypted_key     TEXT    NOT NULL,
            added_at          TEXT    NOT NULL,
            UNIQUE(folder_path, member_address)
        );

        CREATE INDEX IF NOT EXISTS idx_shared_folder ON shared_members(folder_path);
        CREATE INDEX IF NOT EXISTS idx_shared_member ON shared_members(member_address);
        ",
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initialize_creates_tables() {
        let conn = Connection::open_in_memory().unwrap();
        initialize(&conn).unwrap();

        let count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='files'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);

        let count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='directories'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_v2_tables_exist() {
        let conn = Connection::open_in_memory().unwrap();
        initialize(&conn).unwrap();

        let count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='folder_keys'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);

        let count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='shared_members'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_initialize_is_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        initialize(&conn).unwrap();
        initialize(&conn).unwrap();
    }

    #[test]
    fn test_root_directory_seeded() {
        let conn = Connection::open_in_memory().unwrap();
        initialize(&conn).unwrap();

        let path: String = conn
            .query_row("SELECT path FROM directories WHERE path = '/'", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(path, "/");
    }
}
