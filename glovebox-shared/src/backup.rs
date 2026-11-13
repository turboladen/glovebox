//! Dated copies of the database file, kept beside it in a `backups`
//! directory.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Local};
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

use crate::error::{DomainError, DomainResult};

/// How many backups [`prune`] keeps.
pub const KEEP: usize = 14;

/// The directory that holds the backups of the database at `db_path`.
pub fn dir_for(db_path: &Path) -> PathBuf {
    db_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .join("backups")
}

/// Whether `db_path` names a file that already holds data. A missing or empty
/// file has nothing to back up.
pub fn has_data(db_path: &Path) -> bool {
    std::fs::metadata(db_path).is_ok_and(|m| m.is_file() && m.len() > 0)
}

fn stem(db_path: &Path) -> String {
    db_path.file_stem().map_or_else(
        || "glovebox".to_owned(),
        |s| s.to_string_lossy().into_owned(),
    )
}

/// Whether `name` is `<prefix><YYYYMMDD>-<HHMMSS>.db`. The exact shape keeps
/// one database's backups apart from those of a database whose name extends
/// it, such as `glovebox-test`.
fn is_backup_name(name: &str, prefix: &str) -> bool {
    let Some(stamp) = name
        .strip_prefix(prefix)
        .and_then(|rest| rest.strip_suffix(".db"))
    else {
        return false;
    };
    stamp.len() == 15
        && stamp.bytes().enumerate().all(|(i, b)| {
            if i == 8 {
                b == b'-'
            } else {
                b.is_ascii_digit()
            }
        })
}

/// The backups of `db_path`, oldest first. The timestamp in each name sorts
/// in time order.
pub fn list(db_path: &Path) -> Vec<PathBuf> {
    let prefix = format!("{}-", stem(db_path));
    let Ok(entries) = std::fs::read_dir(dir_for(db_path)) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| is_backup_name(&n.to_string_lossy(), &prefix))
        })
        .collect();
    found.sort();
    found
}

/// Whether a backup of `db_path` was taken on the calendar day of `now`.
pub fn has_backup_on(db_path: &Path, now: DateTime<Local>) -> bool {
    let day = format!("{}-{}", stem(db_path), now.format("%Y%m%d"));
    list(db_path).iter().any(|p| {
        p.file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with(&day))
    })
}

/// Writes a consistent copy of the database behind `db` to the backups
/// directory of `db_path` and returns the copy's path. Other connections may
/// keep writing while it runs.
///
/// # Errors
///
/// Fails when the directory cannot be created or `SQLite` cannot write the
/// copy. A copy that another process started in the same second counts as
/// taken and is returned.
pub async fn write(
    db: &impl ConnectionTrait,
    db_path: &Path,
    now: DateTime<Local>,
) -> DomainResult<PathBuf> {
    let dir = dir_for(db_path);
    std::fs::create_dir_all(&dir).map_err(|e| {
        DomainError::Internal(format!(
            "cannot create backup directory {}: {e}",
            dir.display()
        ))
    })?;
    let target = dir.join(format!(
        "{}-{}.db",
        stem(db_path),
        now.format("%Y%m%d-%H%M%S")
    ));
    if target.exists() {
        return Ok(target);
    }
    // `VACUUM INTO` reads through a transaction, so the copy is consistent
    // even while the write-ahead log holds changes not yet in the main file.
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "VACUUM INTO ?",
        [target.to_string_lossy().into_owned().into()],
    ))
    .await?;
    Ok(target)
}

/// Deletes the oldest backups of `db_path` until [`KEEP`] remain. A file that
/// cannot be deleted is left in place.
pub fn prune(db_path: &Path) {
    let all = list(db_path);
    let extra = all.len().saturating_sub(KEEP);
    for old in &all[..extra] {
        if let Err(e) = std::fs::remove_file(old) {
            tracing::warn!(path = %old.display(), error = %e, "cannot delete an old backup");
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use sea_orm::{Database, EntityTrait, PaginatorTrait};

    use super::*;
    use crate::{db, entities::vehicle, test_support::VehicleFixture};

    fn at(day: u32, hour: u32) -> DateTime<Local> {
        Local
            .with_ymd_and_hms(2026, 11, day, hour, 0, 0)
            .single()
            .unwrap()
    }

    #[test]
    fn backups_live_in_a_directory_beside_the_database() {
        assert_eq!(
            dir_for(Path::new("data/glovebox.db")),
            Path::new("data/backups")
        );
        assert_eq!(dir_for(Path::new("glovebox.db")), Path::new("./backups"));
    }

    #[tokio::test]
    async fn a_backup_is_a_readable_copy_with_the_same_rows() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("glovebox.db");
        let conn = db::connect(path.to_str().unwrap()).await.unwrap();
        VehicleFixture::new().insert(&conn).await;

        let copy = write(&conn, &path, at(13, 9)).await.unwrap();

        assert_eq!(copy, tmp.path().join("backups/glovebox-20261113-090000.db"));
        let restored = Database::connect(db::connect_options(Some(copy)))
            .await
            .unwrap();
        assert_eq!(vehicle::Entity::find().count(&restored).await.unwrap(), 1);
    }

    #[tokio::test]
    async fn a_backup_in_the_same_second_is_not_written_twice() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("glovebox.db");
        let conn = db::connect(path.to_str().unwrap()).await.unwrap();

        let first = write(&conn, &path, at(13, 9)).await.unwrap();
        let second = write(&conn, &path, at(13, 9)).await.unwrap();

        assert_eq!(first, second);
        assert_eq!(list(&path).len(), 1);
    }

    #[tokio::test]
    async fn has_backup_on_looks_at_the_calendar_day() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("glovebox.db");
        let conn = db::connect(path.to_str().unwrap()).await.unwrap();
        write(&conn, &path, at(13, 9)).await.unwrap();

        assert!(has_backup_on(&path, at(13, 23)));
        assert!(!has_backup_on(&path, at(14, 0)));
    }

    #[test]
    fn prune_keeps_the_newest_and_ignores_other_files() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("glovebox.db");
        let dir = dir_for(&path);
        std::fs::create_dir_all(&dir).unwrap();
        for day in 1..=KEEP + 3 {
            std::fs::write(dir.join(format!("glovebox-202610{day:02}-090000.db")), b"x").unwrap();
        }
        std::fs::write(dir.join("glovebox-test-20261001-090000.db"), b"x").unwrap();
        std::fs::write(dir.join("notes.txt"), b"x").unwrap();

        prune(&path);

        let left = list(&path);
        assert!(dir.join("notes.txt").exists());
        assert!(dir.join("glovebox-test-20261001-090000.db").exists());
        assert!(!dir.join("glovebox-20261001-090000.db").exists());
        assert!(
            dir.join(format!("glovebox-202610{:02}-090000.db", KEEP + 3))
                .exists()
        );
        assert_eq!(left.len(), KEEP);
    }

    #[test]
    fn an_empty_or_missing_file_has_no_data() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("glovebox.db");
        assert!(!has_data(&path));
        std::fs::write(&path, b"").unwrap();
        assert!(!has_data(&path));
        std::fs::write(&path, b"x").unwrap();
        assert!(has_data(&path));
    }
}
