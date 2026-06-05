use rusqlite::Connection;

pub fn run_migrations(conn: &Connection) -> rusqlite::Result<()> {
    let version: i64 =
        conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;

    if version < 1 {
        conn.execute_batch(include_str!("m001_initial.sql"))?;
        conn.execute_batch("PRAGMA user_version = 1")?;
    }

    Ok(())
}
