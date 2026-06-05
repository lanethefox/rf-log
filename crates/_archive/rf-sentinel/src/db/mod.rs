pub mod migrations;
pub use migrations::run_migrations;

use rusqlite::Connection;

/// Open the rf-sentinel SQLite database at the given path.
pub fn open(path: &str) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
    run_migrations(&conn)?;
    Ok(conn)
}
