use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub id: i64,
    pub path: String,
    pub cid: String,
    pub size_bytes: i64,
    pub mime_type: Option<String>,
    pub pinned_at: String,
    pub modified_at: String,
    pub metadata: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirEntry {
    pub id: i64,
    pub path: String,
    pub created_at: String,
}

/// Result type for directory listing — can be a file or subdirectory.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum DirItem {
    File(FileEntry),
    Directory(DirEntry),
}

// ---------------------------------------------------------------------------
// File operations
// ---------------------------------------------------------------------------

pub fn insert_file(conn: &Connection, entry: &FileEntry) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO files (path, cid, size_bytes, mime_type, pinned_at, modified_at, metadata)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            entry.path,
            entry.cid,
            entry.size_bytes,
            entry.mime_type,
            entry.pinned_at,
            entry.modified_at,
            entry.metadata,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn get_file_by_path(conn: &Connection, path: &str) -> rusqlite::Result<FileEntry> {
    conn.query_row(
        "SELECT id, path, cid, size_bytes, mime_type, pinned_at, modified_at, metadata
         FROM files WHERE path = ?1",
        params![path],
        row_to_file_entry,
    )
}

pub fn get_file_by_cid(conn: &Connection, cid: &str) -> rusqlite::Result<FileEntry> {
    conn.query_row(
        "SELECT id, path, cid, size_bytes, mime_type, pinned_at, modified_at, metadata
         FROM files WHERE cid = ?1",
        params![cid],
        row_to_file_entry,
    )
}

pub fn update_file_cid(
    conn: &Connection,
    path: &str,
    new_cid: &str,
    new_size: i64,
    modified_at: &str,
) -> rusqlite::Result<usize> {
    conn.execute(
        "UPDATE files SET cid = ?1, size_bytes = ?2, modified_at = ?3 WHERE path = ?4",
        params![new_cid, new_size, modified_at, path],
    )
}

pub fn delete_file_by_path(conn: &Connection, path: &str) -> rusqlite::Result<usize> {
    conn.execute("DELETE FROM files WHERE path = ?1", params![path])
}

pub fn count_files(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row("SELECT COUNT(*) FROM files", [], |row| row.get(0))
}

// ---------------------------------------------------------------------------
// Directory operations
// ---------------------------------------------------------------------------

pub fn insert_directory(conn: &Connection, path: &str, created_at: &str) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO directories (path, created_at) VALUES (?1, ?2)",
        params![path, created_at],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn get_directory(conn: &Connection, path: &str) -> rusqlite::Result<DirEntry> {
    conn.query_row(
        "SELECT id, path, created_at FROM directories WHERE path = ?1",
        params![path],
        |row| {
            Ok(DirEntry {
                id: row.get(0)?,
                path: row.get(1)?,
                created_at: row.get(2)?,
            })
        },
    )
}

pub fn delete_directory(conn: &Connection, path: &str) -> rusqlite::Result<usize> {
    conn.execute("DELETE FROM directories WHERE path = ?1", params![path])
}

/// List immediate children of a directory path.
/// For path "/", matches files/dirs whose path starts with "/" and has no further "/".
/// For path "/foo", matches children like "/foo/bar" but not "/foo/bar/baz".
pub fn list_directory(conn: &Connection, dir_path: &str) -> rusqlite::Result<Vec<DirItem>> {
    let prefix = if dir_path == "/" {
        "/".to_string()
    } else {
        format!("{}/", dir_path)
    };

    let mut items = Vec::new();

    // Query child files
    let mut file_stmt = conn.prepare(
        "SELECT id, path, cid, size_bytes, mime_type, pinned_at, modified_at, metadata
         FROM files WHERE path LIKE ?1 || '%'",
    )?;

    let file_rows = file_stmt.query_map(params![prefix], row_to_file_entry)?;
    for row in file_rows {
        let entry = row?;
        // Only include direct children (no nested paths beyond one level)
        let relative = &entry.path[prefix.len()..];
        if !relative.contains('/') {
            items.push(DirItem::File(entry));
        }
    }

    // Query child directories
    let mut dir_stmt = conn.prepare(
        "SELECT id, path, created_at
         FROM directories WHERE path LIKE ?1 || '%' AND path != ?2",
    )?;

    let dir_rows = dir_stmt.query_map(params![prefix, dir_path], |row| {
        Ok(DirEntry {
            id: row.get(0)?,
            path: row.get(1)?,
            created_at: row.get(2)?,
        })
    })?;
    for row in dir_rows {
        let entry = row?;
        let relative = &entry.path[prefix.len()..];
        if !relative.contains('/') {
            items.push(DirItem::Directory(entry));
        }
    }

    Ok(items)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn row_to_file_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<FileEntry> {
    Ok(FileEntry {
        id: row.get(0)?,
        path: row.get(1)?,
        cid: row.get(2)?,
        size_bytes: row.get(3)?,
        mime_type: row.get(4)?,
        pinned_at: row.get(5)?,
        modified_at: row.get(6)?,
        metadata: row.get(7)?,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::schema;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        schema::initialize(&conn).unwrap();
        conn
    }

    fn make_file(path: &str, cid: &str, size: i64) -> FileEntry {
        FileEntry {
            id: 0,
            path: path.to_string(),
            cid: cid.to_string(),
            size_bytes: size,
            mime_type: Some("application/octet-stream".to_string()),
            pinned_at: "2026-01-01T00:00:00Z".to_string(),
            modified_at: "2026-01-01T00:00:00Z".to_string(),
            metadata: None,
        }
    }

    #[test]
    fn test_insert_and_get_file() {
        let conn = setup();
        let entry = make_file("/test.txt", "bafyabc123", 1024);

        let id = insert_file(&conn, &entry).unwrap();
        assert!(id > 0);

        let fetched = get_file_by_path(&conn, "/test.txt").unwrap();
        assert_eq!(fetched.cid, "bafyabc123");
        assert_eq!(fetched.size_bytes, 1024);
    }

    #[test]
    fn test_get_file_by_cid() {
        let conn = setup();
        let entry = make_file("/doc.pdf", "bafyxyz789", 2048);
        insert_file(&conn, &entry).unwrap();

        let fetched = get_file_by_cid(&conn, "bafyxyz789").unwrap();
        assert_eq!(fetched.path, "/doc.pdf");
    }

    #[test]
    fn test_update_file_cid() {
        let conn = setup();
        let entry = make_file("/mutable.txt", "bafyold", 100);
        insert_file(&conn, &entry).unwrap();

        let rows = update_file_cid(&conn, "/mutable.txt", "bafynew", 200, "2026-02-01T00:00:00Z")
            .unwrap();
        assert_eq!(rows, 1);

        let fetched = get_file_by_path(&conn, "/mutable.txt").unwrap();
        assert_eq!(fetched.cid, "bafynew");
        assert_eq!(fetched.size_bytes, 200);
    }

    #[test]
    fn test_delete_file() {
        let conn = setup();
        let entry = make_file("/deleteme.txt", "bafydel", 50);
        insert_file(&conn, &entry).unwrap();

        let rows = delete_file_by_path(&conn, "/deleteme.txt").unwrap();
        assert_eq!(rows, 1);

        let result = get_file_by_path(&conn, "/deleteme.txt");
        assert!(result.is_err());
    }

    #[test]
    fn test_count_files() {
        let conn = setup();
        assert_eq!(count_files(&conn).unwrap(), 0);

        insert_file(&conn, &make_file("/a.txt", "cid1", 10)).unwrap();
        insert_file(&conn, &make_file("/b.txt", "cid2", 20)).unwrap();
        assert_eq!(count_files(&conn).unwrap(), 2);
    }

    #[test]
    fn test_duplicate_path_rejected() {
        let conn = setup();
        let entry = make_file("/unique.txt", "cid1", 10);
        insert_file(&conn, &entry).unwrap();

        let result = insert_file(&conn, &entry);
        assert!(result.is_err());
    }

    #[test]
    fn test_insert_and_get_directory() {
        let conn = setup();
        let id = insert_directory(&conn, "/docs", "2026-01-01T00:00:00Z").unwrap();
        assert!(id > 0);

        let dir = get_directory(&conn, "/docs").unwrap();
        assert_eq!(dir.path, "/docs");
    }

    #[test]
    fn test_delete_directory() {
        let conn = setup();
        insert_directory(&conn, "/tmp", "2026-01-01T00:00:00Z").unwrap();

        let rows = delete_directory(&conn, "/tmp").unwrap();
        assert_eq!(rows, 1);

        let result = get_directory(&conn, "/tmp");
        assert!(result.is_err());
    }

    #[test]
    fn test_list_directory_root() {
        let conn = setup();
        insert_directory(&conn, "/docs", "2026-01-01T00:00:00Z").unwrap();
        insert_file(&conn, &make_file("/readme.txt", "cid1", 100)).unwrap();
        insert_file(&conn, &make_file("/docs/spec.md", "cid2", 200)).unwrap();

        let items = list_directory(&conn, "/").unwrap();
        assert_eq!(items.len(), 2); // readme.txt + docs/

        let names: Vec<String> = items
            .iter()
            .map(|i| match i {
                DirItem::File(f) => f.path.clone(),
                DirItem::Directory(d) => d.path.clone(),
            })
            .collect();
        assert!(names.contains(&"/readme.txt".to_string()));
        assert!(names.contains(&"/docs".to_string()));
    }

    #[test]
    fn test_list_directory_nested() {
        let conn = setup();
        insert_directory(&conn, "/docs", "2026-01-01T00:00:00Z").unwrap();
        insert_directory(&conn, "/docs/archive", "2026-01-01T00:00:00Z").unwrap();
        insert_file(&conn, &make_file("/docs/spec.md", "cid1", 100)).unwrap();
        insert_file(&conn, &make_file("/docs/archive/old.md", "cid2", 50)).unwrap();

        let items = list_directory(&conn, "/docs").unwrap();
        assert_eq!(items.len(), 2); // spec.md + archive/

        // Should NOT include /docs/archive/old.md as direct child
        let names: Vec<String> = items
            .iter()
            .map(|i| match i {
                DirItem::File(f) => f.path.clone(),
                DirItem::Directory(d) => d.path.clone(),
            })
            .collect();
        assert!(names.contains(&"/docs/spec.md".to_string()));
        assert!(names.contains(&"/docs/archive".to_string()));
    }

    #[test]
    fn test_list_empty_directory() {
        let conn = setup();
        insert_directory(&conn, "/empty", "2026-01-01T00:00:00Z").unwrap();

        let items = list_directory(&conn, "/empty").unwrap();
        assert!(items.is_empty());
    }
}
