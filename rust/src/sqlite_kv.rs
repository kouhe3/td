//! `td::SqliteKeyValue` (tddb/td/db/SqliteKeyValue.h) on top of `rusqlite`.
//!
//! Statement for statement the same SQL as the C++ class, because what is judged
//! is the *observable* behaviour of those statements, not their text:
//!
//!   * `get_all()` and `get_by_prefix("")` both run `SELECT k, v FROM kv` -- a
//!     full table scan, i.e. **rowid** order. `REPLACE INTO` deletes and
//!     re-inserts, so overwriting a key moves it to the end of that order.
//!   * `get_by_range` and `get_by_prefix(p)` run `?1 <= k AND k < ?2`, which
//!     walks the `k BLOB PRIMARY KEY` index, i.e. **key** order.
//!   * `get_by_prefix` strips the prefix from the keys it hands over;
//!     `get_by_range` does not.
//!   * an empty `from` selects the all-rows statement, so `get_by_range("", till)`
//!     ignores `till`; an empty `till` selects the `?1 <= k` rare statement.
//!   * `next_prefix` bumps the last non-0xFF byte; an all-0xFF prefix has no
//!     successor and therefore takes the rare form, which happens to be exact
//!     (no key sorts above 0xFF...FF except those starting with it).
//!     `erase_by_prefix("")` deletes the whole table.
//!   * a zero-length blob is what `bind_blob` produces for an empty `Slice`
//!     (`Slice()` carries `""`, never nullptr), so nothing is ever NULL.
//!   * `get_all()` walks its result with `CHECK(!key.empty())`: one zero-length
//!     key aborts the process. `get_by_prefix("", cb)` leaves that to the caller.
//!   * `commit` without a matching `begin` is `"No matching begin for commit"`;
//!     the counter lives on the shared `RawSqliteDb`, so nested begins collapse
//!     into one SQL `BEGIN`.
//!
//! Not implemented (out of this lane's judgement; docs/facts.md F196): the
//! SQLCipher paths (`PRAGMA key`, `cipher_compatibility`), `SqliteDb::clone`,
//! `trace`, `explain`, `get_pragma*` and `user_version` -- none of them is
//! reachable through `SqliteKeyValue`.

use std::path::Path;
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};

pub struct SqliteKeyValue {
    conn: Option<Connection>,
    begin_cnt: usize,
}

/// `SqliteKeyValue::next_prefix`: increment the last byte that is not 0xFF.
pub fn next_prefix(prefix: &[u8]) -> Vec<u8> {
    let mut next = prefix.to_vec();
    let mut pos = next.len();
    while pos > 0 {
        pos -= 1;
        let value = next[pos].wrapping_add(1);
        next[pos] = value;
        if value != 0 {
            return next;
        }
    }
    Vec::new()
}

/// `detail::RawSqliteDb::with_db_path`: the four files a database can own.
pub fn db_paths(path: &str) -> [String; 4] {
    [
        path.to_string(),
        format!("{path}-journal"),
        format!("{path}-wal"),
        format!("{path}-shm"),
    ]
}

/// `detail::RawSqliteDb::destroy`: unlink all four, then complain about any that
/// survived (except `-shm`, which SQLite may recreate).
pub fn destroy(path: &str) -> Result<(), String> {
    let mut error = None;
    for candidate in db_paths(path) {
        match std::fs::remove_file(&candidate) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => {}
        }
        if !candidate.ends_with("-shm") && Path::new(&candidate).exists() {
            error = Some(format!("Failed to delete file \"{candidate}\""));
        }
    }
    match error {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

fn blob_at(row: &rusqlite::Row, index: usize) -> Vec<u8> {
    use rusqlite::types::ValueRef;
    match row.get_ref(index) {
        Ok(ValueRef::Blob(bytes)) => bytes.to_vec(),
        Ok(ValueRef::Null) => Vec::new(),
        Ok(ValueRef::Text(bytes)) => bytes.to_vec(),
        Ok(other) => panic!("unexpected sqlite value type in a BLOB column: {other:?}"),
        Err(e) => panic!("cannot read column {index}: {e}"),
    }
}

impl SqliteKeyValue {
    /// `SqliteDb::open_with_key(path, allow_creation, DbKey::empty())` followed by
    /// `SqliteKeyValue::init_with_connection(db, table_name)`.
    pub fn open(path: &str, allow_creation: bool, table_name: &str) -> Result<Self, String> {
        let exists = Path::new(path).exists();
        if !exists {
            if !allow_creation {
                return Err(
                    "Database disappeared during execution and can't be recreated".to_string()
                );
            }
            destroy(path)?;
        }
        let mut flags = OpenFlags::SQLITE_OPEN_READ_WRITE;
        if allow_creation {
            flags |= OpenFlags::SQLITE_OPEN_CREATE;
        }
        let conn = Connection::open_with_flags(path, flags).map_err(|e| e.to_string())?;
        conn.busy_timeout(Duration::from_millis(5000))
            .map_err(|e| e.to_string())?;
        let mut kv = SqliteKeyValue {
            conn: Some(conn),
            begin_cnt: 0,
        };
        kv.init(table_name)?;
        Ok(kv)
    }

    fn connection(&self) -> &Connection {
        self.conn.as_ref().expect("the key-value is not open")
    }

    fn init(&mut self, table_name: &str) -> Result<(), String> {
        let sql = format!("CREATE TABLE IF NOT EXISTS {table_name} (k BLOB PRIMARY KEY, v BLOB)");
        self.connection().execute(&sql, []).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self.conn.is_none()
    }

    /// `close()` is `*this = SqliteKeyValue()`, i.e. the connection is dropped.
    pub fn close(&mut self) {
        self.conn = None;
        self.begin_cnt = 0;
    }

    pub fn set(&mut self, key: &[u8], value: &[u8]) {
        let conn = self.conn.as_mut().expect("the key-value is not open");
        conn.execute(
            "REPLACE INTO kv (k, v) VALUES (?1, ?2)",
            rusqlite::params![key, value],
        )
        .expect("Failed to set");
    }

    /// `get()` returns `""` for a missing key *and* for an empty value.
    pub fn get(&self, key: &[u8]) -> Vec<u8> {
        let conn = self.connection();
        let mut stmt = conn
            .prepare("SELECT v FROM kv WHERE k = ?1")
            .expect("prepare get");
        let mut rows = stmt.query(rusqlite::params![key]).expect("query get");
        match rows.next().expect("step get") {
            Some(row) => blob_at(row, 0),
            None => Vec::new(),
        }
    }

    pub fn erase(&mut self, key: &[u8]) {
        let conn = self.conn.as_mut().expect("the key-value is not open");
        conn.execute("DELETE FROM kv WHERE k = ?1", rusqlite::params![key])
            .expect("Failed to erase");
    }

    pub fn erase_batch(&mut self, keys: &[Vec<u8>]) {
        for key in keys {
            self.erase(key);
        }
    }

    fn get_by_range_impl<F>(&self, from: &[u8], till: &[u8], strip_key_prefix: bool, mut callback: F)
    where
        F: FnMut(&[u8], &[u8]) -> bool,
    {
        let conn = self.connection();
        // Three distinct statements, exactly as `init_with_connection` prepares
        // them: the empty `from` one has no WHERE clause at all.
        let (sql, binds): (&str, usize) = if from.is_empty() {
            ("SELECT k, v FROM kv", 0)
        } else if till.is_empty() {
            ("SELECT k, v FROM kv WHERE ?1 <= k", 1)
        } else {
            ("SELECT k, v FROM kv WHERE ?1 <= k AND k < ?2", 2)
        };
        let mut stmt = conn.prepare(sql).expect("prepare range");
        let mut rows = match binds {
            0 => stmt.query([]).expect("query range"),
            1 => stmt.query(rusqlite::params![from]).expect("query range"),
            _ => stmt.query(rusqlite::params![from, till]).expect("query range"),
        };
        while let Some(row) = rows.next().expect("step range") {
            let key = blob_at(row, 0);
            let value = blob_at(row, 1);
            // `Slice::remove_prefix` is a CHECK in C++; slicing panics here for
            // the same reason (only reachable if SQLite returned a key outside
            // the range it was asked for).
            let key: &[u8] = if strip_key_prefix {
                &key[from.len()..]
            } else {
                &key
            };
            if !callback(key, &value) {
                return;
            }
        }
    }

    pub fn get_by_prefix<F>(&self, prefix: &[u8], callback: F)
    where
        F: FnMut(&[u8], &[u8]) -> bool,
    {
        let next = if prefix.is_empty() {
            Vec::new()
        } else {
            next_prefix(prefix)
        };
        self.get_by_range_impl(prefix, &next, true, callback);
    }

    pub fn get_by_range<F>(&self, from: &[u8], till: &[u8], callback: F)
    where
        F: FnMut(&[u8], &[u8]) -> bool,
    {
        self.get_by_range_impl(from, till, false, callback);
    }

    /// `get_all()`: `get_by_prefix("")` with `CHECK(!key.empty())` on every row.
    pub fn get_all(&self) -> Vec<(Vec<u8>, Vec<u8>)> {
        let mut result = Vec::new();
        self.get_by_prefix(b"", |key, value| {
            assert!(
                !key.is_empty(),
                "CHECK(!key.empty()) failed: a zero-length key is in the table"
            );
            result.push((key.to_vec(), value.to_vec()));
            true
        });
        result
    }

    pub fn erase_by_prefix(&mut self, prefix: &[u8]) {
        let next = next_prefix(prefix);
        let conn = self.conn.as_mut().expect("the key-value is not open");
        if next.is_empty() {
            conn.execute("DELETE FROM kv WHERE ?1 <= k", rusqlite::params![prefix])
                .expect("Failed to erase_by_prefix");
        } else {
            conn.execute(
                "DELETE FROM kv WHERE ?1 <= k AND k < ?2",
                rusqlite::params![prefix, next],
            )
            .expect("Failed to erase_by_prefix");
        }
    }

    /// `SqliteKeyValue::drop()`: no-op when already closed.
    pub fn drop_table(&mut self) -> Result<(), String> {
        if self.is_empty() {
            return Ok(());
        }
        let conn = self.connection();
        conn.execute("DROP TABLE IF EXISTS kv", [])
            .map_err(|e| e.to_string())?;
        self.close();
        Ok(())
    }

    /// `RawSqliteDb::on_begin`: only the outermost begin runs SQL.
    fn on_begin(&mut self) -> bool {
        self.begin_cnt += 1;
        self.begin_cnt == 1
    }

    /// `RawSqliteDb::on_commit`: an unmatched commit is an error.
    fn on_commit(&mut self) -> Result<bool, String> {
        if self.begin_cnt == 0 {
            return Err("No matching begin for commit".to_string());
        }
        self.begin_cnt -= 1;
        Ok(self.begin_cnt == 0)
    }

    fn exec(&mut self, sql: &str) -> Result<(), String> {
        let conn = self.conn.as_mut().expect("the key-value is not open");
        conn.execute(sql, []).map(|_| ()).map_err(|e| e.to_string())
    }

    pub fn begin_read_transaction(&mut self) -> Result<(), String> {
        if self.on_begin() {
            self.exec("BEGIN")
        } else {
            Ok(())
        }
    }

    pub fn begin_write_transaction(&mut self) -> Result<(), String> {
        if self.on_begin() {
            self.exec("BEGIN IMMEDIATE")
        } else {
            Ok(())
        }
    }

    pub fn commit_transaction(&mut self) -> Result<(), String> {
        if self.on_commit()? {
            self.exec("COMMIT")
        } else {
            Ok(())
        }
    }
}