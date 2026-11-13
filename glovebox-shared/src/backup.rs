//! Dated copies of the database file, kept beside it in a `backups`
//! directory.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Local};
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

use crate::error::{DomainError, DomainResult};

/// How many of the newest backups [`prune`] keeps.
pub const KEEP: usize = 14;

/// How often [`spawn_daily`] looks for a day that has no backup yet.
const DAILY_CHECK: std::time::Duration = std::time::Duration::from_hours(1);

/// How old a leftover temporary file must be before [`write`] removes it.
const STALE_TEMP: std::time::Duration = std::time::Duration::from_hours(1);

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
    remove_stale_temps(&dir);
    // The copy is written under a name no other process uses and renamed
    // once it is complete, so a file with a backup's name is always whole and
    // two processes backing up in the same second cannot collide.
    let temp = dir.join(format!(
        ".{}-{}-{}.tmp",
        stem(db_path),
        std::process::id(),
        now.timestamp_nanos_opt().unwrap_or_default()
    ));
    // `VACUUM INTO` reads through a transaction, so the copy is consistent
    // even while the write-ahead log holds changes not yet in the main file.
    let copied = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "VACUUM INTO ?",
            [temp.to_string_lossy().into_owned().into()],
        ))
        .await;
    if let Err(e) = copied {
        let _ = std::fs::remove_file(&temp);
        return Err(e.into());
    }
    if target.exists() {
        let _ = std::fs::remove_file(&temp);
        return Ok(target);
    }
    std::fs::rename(&temp, &target).map_err(|e| {
        let _ = std::fs::remove_file(&temp);
        DomainError::Internal(format!("cannot name backup {}: {e}", target.display()))
    })?;
    Ok(target)
}

/// Removes temporary files that an interrupted backup left in `dir`.
fn remove_stale_temps(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for path in entries.filter_map(Result::ok).map(|e| e.path()) {
        let stale = path.extension().is_some_and(|x| x == "tmp")
            && std::fs::metadata(&path)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|age| age > STALE_TEMP);
        if stale {
            let _ = std::fs::remove_file(&path);
        }
    }
}

/// Whether the database behind `db` holds a vehicle. A database with none is
/// a new, seeded one, and copying it every day would only push real backups
/// out of the set that [`prune`] keeps. A database whose vehicles cannot be
/// counted is treated as holding data.
pub async fn worth_keeping(db: &impl ConnectionTrait) -> bool {
    let row = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT COUNT(*) FROM vehicles",
        ))
        .await;
    match row {
        Ok(Some(r)) => r.try_get_by_index::<i64>(0).map_or(true, |n| n > 0),
        _ => true,
    }
}

/// The backups [`prune`] keeps out of `all` (oldest first): the newest
/// [`KEEP`], the newest of every calendar month, and the largest file. The
/// monthly and largest copies mean a run of backups of a damaged or emptied
/// database cannot push out the last good one.
fn kept(all: &[PathBuf]) -> std::collections::HashSet<PathBuf> {
    let mut keep: std::collections::HashSet<PathBuf> =
        all.iter().rev().take(KEEP).cloned().collect();
    let month = |p: &PathBuf| {
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned());
        // The stamp is the last 18 characters: `YYYYMMDD-HHMMSS.db`.
        name.and_then(|n| n.len().checked_sub(18).map(|i| n[i..i + 6].to_owned()))
    };
    let mut newest_of_month = std::collections::HashMap::new();
    for p in all {
        if let Some(m) = month(p) {
            newest_of_month.insert(m, p.clone());
        }
    }
    keep.extend(newest_of_month.into_values());
    let size = |p: &PathBuf| std::fs::metadata(p).map_or(0, |m| m.len());
    // `max_by_key` returns the last of equal maxima, which is the newest.
    if let Some(largest) = all.iter().max_by_key(|p| size(p)) {
        keep.insert(largest.clone());
    }
    keep
}

/// Deletes the backups of `db_path` that the retention rule does not keep:
/// the newest [`KEEP`], the newest of every calendar month, and the largest
/// file all stay. A file that cannot be deleted is left in place.
pub fn prune(db_path: &Path) {
    let all = list(db_path);
    let keep = kept(&all);
    for old in all.iter().filter(|p| !keep.contains(*p)) {
        if let Err(e) = std::fs::remove_file(old) {
            tracing::warn!(path = %old.display(), error = %e, "cannot delete an old backup");
        }
    }
}

/// Backs up the database once on each calendar day that has no backup yet,
/// for as long as the process runs. A server that stays up for weeks is
/// covered by this task, since it opens the database only once.
pub fn spawn_daily<C>(db: C, db_path: PathBuf) -> tokio::task::JoinHandle<()>
where
    C: ConnectionTrait + Send + Sync + 'static,
{
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(DAILY_CHECK);
        loop {
            tick.tick().await;
            daily(&db, &db_path, Local::now()).await;
        }
    })
}

/// Takes the day's backup when `now`'s calendar day has none and the database
/// holds data. Failures are logged.
pub async fn daily(db: &impl ConnectionTrait, db_path: &Path, now: DateTime<Local>) {
    if has_backup_on(db_path, now) || !worth_keeping(db).await {
        return;
    }
    match write(db, db_path, now).await {
        Ok(copy) => {
            tracing::info!(path = %copy.display(), "backed up the database");
            prune(db_path);
        }
        Err(e) => tracing::warn!(error = %e, "could not back up the database"),
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

    #[tokio::test]
    async fn no_temporary_file_is_left_and_a_stale_one_is_removed() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("glovebox.db");
        let conn = db::connect(path.to_str().unwrap()).await.unwrap();
        let dir = dir_for(&path);
        std::fs::create_dir_all(&dir).unwrap();
        let stale = dir.join(".glovebox-1-1.tmp");
        let fresh = dir.join(".glovebox-2-2.tmp");
        std::fs::write(&stale, b"partial").unwrap();
        std::fs::write(&fresh, b"partial").unwrap();
        let two_hours_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(7200);
        std::fs::File::options()
            .write(true)
            .open(&stale)
            .unwrap()
            .set_modified(two_hours_ago)
            .unwrap();

        write(&conn, &path, at(13, 9)).await.unwrap();

        assert!(!stale.exists());
        // A fresh temporary file may belong to a backup still being written.
        assert!(fresh.exists());
        let temps = std::fs::read_dir(&dir)
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .path()
                    .extension()
                    .is_some_and(|x| x == "tmp")
            })
            .count();
        assert_eq!(temps, 1);
        assert_eq!(list(&path).len(), 1);
    }

    #[tokio::test]
    async fn daily_skips_a_database_with_no_vehicle_and_a_day_already_covered() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("glovebox.db");
        let conn = db::connect(path.to_str().unwrap()).await.unwrap();

        daily(&conn, &path, at(13, 9)).await;
        assert!(list(&path).is_empty());

        VehicleFixture::new().insert(&conn).await;
        daily(&conn, &path, at(13, 9)).await;
        daily(&conn, &path, at(13, 15)).await;
        assert_eq!(list(&path).len(), 1);

        daily(&conn, &path, at(14, 9)).await;
        assert_eq!(list(&path).len(), 2);
    }

    #[test]
    fn prune_keeps_the_newest_of_each_month_and_the_largest() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("glovebox.db");
        let dir = dir_for(&path);
        std::fs::create_dir_all(&dir).unwrap();
        // One large copy in August, small ones in September, then more than
        // KEEP small ones in October, as after the live file is replaced by
        // an empty one.
        std::fs::write(dir.join("glovebox-20260810-090000.db"), vec![0; 4096]).unwrap();
        std::fs::write(dir.join("glovebox-20260905-090000.db"), b"x").unwrap();
        std::fs::write(dir.join("glovebox-20260920-090000.db"), b"x").unwrap();
        for day in 1..=KEEP + 3 {
            std::fs::write(dir.join(format!("glovebox-202610{day:02}-090000.db")), b"x").unwrap();
        }

        prune(&path);

        // The largest copy and each month's newest survive beside the newest
        // KEEP.
        assert!(dir.join("glovebox-20260810-090000.db").exists());
        assert!(dir.join("glovebox-20260920-090000.db").exists());
        assert!(!dir.join("glovebox-20260905-090000.db").exists());
        assert!(!dir.join("glovebox-20261001-090000.db").exists());
        assert_eq!(list(&path).len(), KEEP + 2);
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
