pub mod index;
pub mod schema;

use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::Mutex;

pub struct Database {
    pub conn: Mutex<Connection>,
}

impl Database {
    /// Open (or create) the ShelDrive index database.
    /// Default location: ~/.sheldrive/index.db
    pub fn open() -> rusqlite::Result<Self> {
        let db_path = Self::default_path();

        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                rusqlite::Error::InvalidPath(
                    format!("Failed to create directory {}: {}", parent.display(), e).into(),
                )
            })?;
        }

        let conn = Connection::open(&db_path)?;
        schema::initialize(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Open an in-memory database (for testing).
    pub fn open_in_memory() -> rusqlite::Result<Self> {
        let conn = Connection::open_in_memory()?;
        schema::initialize(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn default_path() -> PathBuf {
        let home = dirs::home_dir().expect("Could not determine home directory");
        home.join(".sheldrive").join("index.db")
    }
}
