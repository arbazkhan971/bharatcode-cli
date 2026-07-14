//! `bharatcode db`: inspect and (optionally) reclaim space in the session
//! database.
//!
//! The CLI persists every chat session in a single SQLite file at
//! `<data_dir>/sessions/sessions.db` (the same file [`SessionStorage`] opens).
//! Over a long-lived install that file accumulates free pages from deleted /
//! rewritten rows and a growing write-ahead log, so it can occupy far more disk
//! than the live data warrants.
//!
//! `bharatcode db` opens a *fresh*, independent connection to that file and runs
//! a read-only health report: total size (`page_count * page_size`), reclaimable
//! free space (`freelist_count * page_size`), and an `integrity_check`. With the
//! explicit, destructive opt-in `--vacuum` it then runs `VACUUM;` followed by a
//! `wal_checkpoint(TRUNCATE)` to physically shrink the file and fold the WAL
//! back in, reporting how many bytes were reclaimed.
//!
//! This never edits or shares the `SessionStorage` connection pool; it resolves
//! the path through the public data-dir helper and opens its own pool, so it is
//! safe to run as a standalone maintenance command. There is no env gate: the
//! read-only stats are always shown, and `--vacuum` is the only switch that
//! mutates the file.
//!
//! Original BharatCode work; not ported from any third party.

use std::path::PathBuf;

use anyhow::Result;
use bharatcode_core::config::paths::Paths;
use bharatcode_core::session::session_manager::{DB_NAME, SESSIONS_FOLDER};
use console::style;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Pool, Row, Sqlite};

/// Options for the `db` subcommand.
#[derive(Debug, Default, Clone)]
pub struct DbOptions {
    /// Run the destructive `VACUUM` + WAL-truncate reclaim pass.
    ///
    /// Defaults to `false`: without it the command is strictly read-only.
    pub vacuum: bool,
    /// Show the read-only statistics block.
    ///
    /// Stats are shown whenever neither flag is given, so this is mostly a way
    /// to ask for the report explicitly even alongside `--vacuum`.
    pub stats: bool,
}

/// A point-in-time snapshot of the session database's on-disk footprint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DbStats {
    /// `PRAGMA page_size` — bytes per database page.
    pub page_size: i64,
    /// `PRAGMA page_count` — total pages in the main database file.
    pub page_count: i64,
    /// `PRAGMA freelist_count` — currently-unused pages eligible for reuse.
    pub freelist_count: i64,
}

impl DbStats {
    /// Logical size of the main database file in bytes (`page_count * page_size`).
    pub fn size_bytes(&self) -> i64 {
        self.page_size.saturating_mul(self.page_count)
    }

    /// Reclaimable free space in bytes (`freelist_count * page_size`).
    pub fn free_bytes(&self) -> i64 {
        self.page_size.saturating_mul(self.freelist_count)
    }
}

/// Resolve the same `<data_dir>/sessions/sessions.db` path that
/// `SessionStorage::new` derives, without touching `session_manager.rs`.
fn session_db_path() -> PathBuf {
    Paths::data_dir().join(SESSIONS_FOLDER).join(DB_NAME)
}

/// Open a fresh, read-write pool against `path`.
///
/// `create_if_missing(false)` keeps the command honest: it inspects an existing
/// database rather than silently materialising an empty one.
async fn open_pool(path: &std::path::Path) -> Result<Pool<Sqlite>> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(false)
        .busy_timeout(std::time::Duration::from_secs(30));

    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await?;
    Ok(pool)
}

/// Run the three read-only pragmas that make up the size/health report.
async fn collect_stats(pool: &Pool<Sqlite>) -> Result<DbStats> {
    let page_size: i64 = sqlx::query("PRAGMA page_size")
        .fetch_one(pool)
        .await?
        .try_get(0)?;
    let page_count: i64 = sqlx::query("PRAGMA page_count")
        .fetch_one(pool)
        .await?
        .try_get(0)?;
    let freelist_count: i64 = sqlx::query("PRAGMA freelist_count")
        .fetch_one(pool)
        .await?
        .try_get(0)?;

    Ok(DbStats {
        page_size,
        page_count,
        freelist_count,
    })
}

/// Run `PRAGMA integrity_check` and return `true` when SQLite reports `ok`.
async fn integrity_ok(pool: &Pool<Sqlite>) -> Result<bool> {
    let result: String = sqlx::query("PRAGMA integrity_check")
        .fetch_one(pool)
        .await?
        .try_get(0)?;
    Ok(result.eq_ignore_ascii_case("ok"))
}

/// Reclaim space: `VACUUM;` then fold the WAL back in with a truncating
/// checkpoint. Both are no-ops on an already-compact file.
async fn reclaim(pool: &Pool<Sqlite>) -> Result<()> {
    sqlx::query("VACUUM").execute(pool).await?;
    // TRUNCATE checkpoints the WAL and shrinks the `-wal` file back to zero so
    // the freed pages actually leave the disk footprint.
    sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
        .execute(pool)
        .await?;
    Ok(())
}

/// Render a byte count as a short human string (e.g. `1.5 MiB`).
fn human_bytes(bytes: i64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[0])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// Look up a user-facing label through the i18n layer, falling back to the
/// English `default` when the active locale table has no entry for `key`.
fn label(key: &str, default: &str) -> String {
    let translated = crate::tr!(key);
    if translated == key {
        default.to_string()
    } else {
        translated
    }
}

/// Entry point for `bharatcode db`.
pub async fn handle_db(opts: DbOptions) -> Result<()> {
    let path = session_db_path();

    if !path.exists() {
        println!(
            "{} {}",
            style(label("db.no_database", "No session database at")).dim(),
            path.display()
        );
        println!(
            "{}",
            style(label(
                "db.no_database.hint",
                "Nothing to inspect yet — start a session to create one.",
            ))
            .dim()
        );
        return Ok(());
    }

    let pool = open_pool(&path).await?;

    let before = collect_stats(&pool).await?;
    let integrity = integrity_ok(&pool).await?;

    println!(
        "{} {}",
        style(label("db.path", "Session database:")).bold(),
        path.display()
    );
    println!(
        "  {:<14} {} ({} pages x {} B)",
        label("db.size", "Size:"),
        human_bytes(before.size_bytes()),
        before.page_count,
        before.page_size,
    );
    println!(
        "  {:<14} {} ({} pages)",
        label("db.free", "Reclaimable:"),
        human_bytes(before.free_bytes()),
        before.freelist_count,
    );
    let integrity_label = if integrity {
        style(label("db.integrity.ok", "ok")).green().to_string()
    } else {
        style(label("db.integrity.fail", "FAILED"))
            .red()
            .to_string()
    };
    println!(
        "  {:<14} {}",
        label("db.integrity", "Integrity:"),
        integrity_label
    );

    if opts.vacuum {
        println!(
            "\n{}",
            style(label("db.vacuum.running", "Vacuuming…")).color256(208)
        );
        reclaim(&pool).await?;
        let after = collect_stats(&pool).await?;
        let reclaimed = (before.size_bytes() - after.size_bytes()).max(0);
        println!(
            "  {:<14} {}",
            label("db.size.after", "New size:"),
            human_bytes(after.size_bytes())
        );
        println!(
            "  {:<14} {}",
            label("db.reclaimed", "Reclaimed:"),
            style(human_bytes(reclaimed)).green()
        );
    } else {
        let _ = opts.stats; // stats are always printed above
        if before.free_bytes() > 0 {
            println!(
                "\n{}",
                style(label(
                    "db.vacuum.hint",
                    "Run `bharatcode db --vacuum` to reclaim free space.",
                ))
                .dim()
            );
        }
    }

    pool.close().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqliteConnectOptions;
    use sqlx::sqlite::SqlitePoolOptions;
    use tempfile::tempdir;

    async fn create_pool(path: &std::path::Path) -> Pool<Sqlite> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal);
        SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .expect("open temp sqlite")
    }

    #[test]
    fn db_options_default_has_vacuum_off() {
        let opts = DbOptions::default();
        assert!(!opts.vacuum, "vacuum must default to false");
        assert!(!opts.stats);
    }

    #[test]
    fn human_bytes_scales_units() {
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(1024), "1.0 KiB");
        assert_eq!(human_bytes(1024 * 1024), "1.0 MiB");
    }

    #[test]
    fn stats_derive_size_and_free_bytes() {
        let stats = DbStats {
            page_size: 4096,
            page_count: 10,
            freelist_count: 3,
        };
        assert_eq!(stats.size_bytes(), 40960);
        assert_eq!(stats.free_bytes(), 12288);
    }

    #[tokio::test]
    async fn stats_report_positive_page_size_and_ok_integrity() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("t.db");
        let pool = create_pool(&db_path).await;
        sqlx::query("CREATE TABLE t (id INTEGER PRIMARY KEY, blob TEXT)")
            .execute(&pool)
            .await
            .expect("create table");

        let stats = collect_stats(&pool).await.expect("collect stats");
        assert!(stats.page_size > 0, "page size must be positive");
        assert!(stats.page_count > 0, "page count must be positive");

        assert!(integrity_ok(&pool).await.expect("integrity"));
        pool.close().await;
    }

    #[tokio::test]
    async fn vacuum_does_not_increase_size_after_churn() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("t.db");
        let pool = create_pool(&db_path).await;
        sqlx::query("CREATE TABLE t (id INTEGER PRIMARY KEY, blob TEXT)")
            .execute(&pool)
            .await
            .expect("create table");

        // Insert a bunch of rows to grow the file, then delete them so the
        // pages land on the freelist.
        let filler = "x".repeat(2000);
        for i in 0..500i64 {
            sqlx::query("INSERT INTO t (id, blob) VALUES (?, ?)")
                .bind(i)
                .bind(&filler)
                .execute(&pool)
                .await
                .expect("insert");
        }
        sqlx::query("DELETE FROM t")
            .execute(&pool)
            .await
            .expect("delete");

        let before = collect_stats(&pool).await.expect("before");
        assert!(
            before.freelist_count > 0,
            "expected free pages after delete, got {}",
            before.freelist_count
        );

        reclaim(&pool).await.expect("reclaim");

        let after = collect_stats(&pool).await.expect("after");
        assert!(
            after.size_bytes() <= before.size_bytes(),
            "vacuum must not increase size: before={} after={}",
            before.size_bytes(),
            after.size_bytes()
        );
        // Freelist should be reclaimed by VACUUM.
        assert!(
            after.freelist_count <= before.freelist_count,
            "freelist should shrink after vacuum"
        );
        pool.close().await;
    }
}
