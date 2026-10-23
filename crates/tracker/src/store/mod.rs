//! SQLite store (plan section 4 "Store"): connection setup, `user_version` migrations, the single
//! writer's operations (`writer.rs`) and the read-side queries behind `Reader` (`queries.rs`, and
//! `export.rs` for `export_data`).

mod export;
mod queries;
mod writer;

use std::path::Path;
use std::time::Duration;

use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ToSqlOutput, ValueRef};
use rusqlite::{Connection, ToSql};
use tracker_api::{ApiError, AppIdentity, Appearance, Category, icon_path};

pub use queries::Reader;
pub(crate) use writer::Writer;

/// `<data_dir>/tracker.db`, beside `icons/` and `logs/`.
const DB_FILE: &str = "tracker.db";

/// One embedded SQL file per schema version; `user_version` records how many have run.
const MIGRATIONS: &[&str] = &[
    include_str!("schema_v1.sql"),
    include_str!("schema_v2.sql"),
    include_str!("schema_v3.sql"),
    include_str!("schema_v4.sql"),
    include_str!("schema_v5.sql"),
    include_str!("schema_v6.sql"),
    include_str!("schema_v7.sql"),
];

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// Bad input from the UI. The message is shown to the user as is.
    #[error("{0}")]
    Invalid(String),
    #[error("date error: {0}")]
    Time(#[from] jiff::Error),
    #[error("couldn't start the tracker: {0}")]
    Io(#[from] std::io::Error),
    #[error("couldn't write the export: {0}")]
    Json(#[from] serde_json::Error),
}

impl From<StoreError> for ApiError {
    fn from(e: StoreError) -> Self {
        ApiError {
            message: e.to_string(),
        }
    }
}

fn connect(data_dir: &Path) -> Result<Connection, StoreError> {
    let conn = Connection::open(data_dir.join(DB_FILE))?;
    // Writer and reader briefly contend on WAL checkpoints; wait instead of failing.
    conn.busy_timeout(Duration::from_millis(2000))?;
    Ok(conn)
}

/// The writer's connection, migrated to the latest schema.
fn connect_writer(data_dir: &Path) -> Result<Connection, StoreError> {
    let mut conn = connect(data_dir)?;
    // journal_mode answers with the resulting mode, so it needs the checking variant.
    conn.pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get::<_, String>(0))?;
    conn.pragma_update(None, "synchronous", "FULL")?;
    conn.pragma_update(None, "foreign_keys", true)?;
    migrate(&mut conn)?;
    Ok(conn)
}

/// A read-only connection for queries; WAL lets it read while the writer writes.
fn connect_reader(data_dir: &Path) -> Result<Connection, StoreError> {
    let conn = connect(data_dir)?;
    conn.pragma_update(None, "query_only", true)?;
    // 256 KiB instead of the 2 MiB default: queries are small and the process runs all day.
    conn.pragma_update(None, "cache_size", -256)?;
    Ok(conn)
}

fn migrate(conn: &mut Connection) -> Result<(), StoreError> {
    let version: u32 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version as usize > MIGRATIONS.len() {
        return Err(StoreError::Invalid(
            "This data was written by a newer version of the app.".into(),
        ));
    }
    for (done, sql) in (version..).zip(MIGRATIONS.iter().skip(version as usize)) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", done + 1)?;
        tx.commit()?;
    }
    Ok(())
}

/// `Category` in its stored text form. A newtype because the orphan rule forbids implementing
/// rusqlite's traits on `Category` directly.
struct SqlCategory(Category);

impl ToSql for SqlCategory {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(match self.0 {
            Category::Productive => "productive",
            Category::Entertainment => "entertainment",
        }
        .into())
    }
}

impl FromSql for SqlCategory {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        match value.as_str()? {
            "productive" => Ok(Self(Category::Productive)),
            "entertainment" => Ok(Self(Category::Entertainment)),
            _ => Err(FromSqlError::InvalidType),
        }
    }
}

/// `Appearance` in its stored text form, for the same orphan-rule reason as `SqlCategory`.
struct SqlAppearance(Appearance);

impl ToSql for SqlAppearance {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(match self.0 {
            Appearance::Glass => "glass",
            Appearance::Solid => "solid",
        }
        .into())
    }
}

impl FromSql for SqlAppearance {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        match value.as_str()? {
            "glass" => Ok(Self(Appearance::Glass)),
            "solid" => Ok(Self(Appearance::Solid)),
            _ => Err(FromSqlError::InvalidType),
        }
    }
}

/// The app's icon path only if the catalog has extracted it, so the UI goes straight to the
/// monogram otherwise.
pub(crate) fn existing_icon(data_dir: &Path, identity: &AppIdentity) -> Option<String> {
    let path = icon_path(data_dir, identity);
    path.is_file().then(|| path.to_string_lossy().into_owned())
}

/// `(kind, key)` columns of the `app` table.
fn identity_columns(identity: &AppIdentity) -> (&'static str, &str) {
    match identity {
        AppIdentity::Exe { key } => ("exe", key),
        AppIdentity::Aumid { key } => ("aumid", key),
    }
}

fn identity_from_columns(kind: &str, key: String) -> rusqlite::Result<AppIdentity> {
    match kind {
        "exe" => Ok(AppIdentity::Exe { key }),
        "aumid" => Ok(AppIdentity::Aumid { key }),
        _ => Err(FromSqlError::InvalidType.into()),
    }
}
