//! Named environments: which database file and web port a process uses.

use std::path::{Path, PathBuf};

use crate::error::{DomainError, DomainResult};

/// The name of the environment that holds the owner's real data.
pub const LIVE: &str = "live";

const LIVE_PORT: u16 = 4827;
const OTHER_PORT: u16 = 4828;
const MAX_NAME_LEN: usize = 32;

/// The database and port one process works against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Environment {
    pub name: String,
    pub db_path: String,
    pub web_port: u16,
}

impl Environment {
    pub fn is_live(&self) -> bool {
        self.name == LIVE
    }
}

/// Reads `GLOVEBOX_ENV`, `GLOVEBOX_DB_PATH` and `GLOVEBOX_WEB_PORT` through
/// `lookup`. An absent or `live` environment uses `data/glovebox.db` and port
/// 4827; any other valid name uses `data/glovebox-<name>.db` and port 4828.
/// `GLOVEBOX_DB_PATH` replaces the path only, and `GLOVEBOX_WEB_PORT` replaces
/// the port. Production passes `|k| std::env::var(k).ok()`.
pub fn resolve(lookup: impl Fn(&str) -> Option<String>) -> DomainResult<Environment> {
    let name = match lookup("GLOVEBOX_ENV") {
        None => LIVE.to_owned(),
        Some(raw) if raw == LIVE => LIVE.to_owned(),
        Some(raw) => test_name(&raw)
            .map_err(|_| {
                DomainError::invalid(
                    "GLOVEBOX_ENV",
                    format!(
                        "{raw:?} is not an environment name; use `live` or 1 to 32 characters \
                         from a-z, 0-9 and -"
                    ),
                )
            })?
            .to_owned(),
    };
    let (default_path, default_port) = if name == LIVE {
        ("data/glovebox.db".to_owned(), LIVE_PORT)
    } else {
        (format!("data/glovebox-{name}.db"), OTHER_PORT)
    };
    let web_port = match lookup("GLOVEBOX_WEB_PORT") {
        None => default_port,
        Some(raw) => raw.trim().parse().map_err(|_| {
            DomainError::invalid("GLOVEBOX_WEB_PORT", format!("{raw:?} is not a port number"))
        })?,
    };
    Ok(Environment {
        name,
        db_path: lookup("GLOVEBOX_DB_PATH").unwrap_or(default_path),
        web_port,
    })
}

/// Validates the name of a non-live environment: 1 to 32 characters from
/// `a-z`, `0-9` and `-`, and not `live`.
pub fn test_name(name: &str) -> DomainResult<&str> {
    let well_formed = !name.is_empty()
        && name.len() <= MAX_NAME_LEN
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if !well_formed {
        return Err(DomainError::invalid(
            "env",
            format!(
                "{name:?} is not an environment name; use 1 to 32 characters from a-z, 0-9 and -"
            ),
        ));
    }
    if name == LIVE {
        return Err(DomainError::invalid(
            "env",
            "the live environment cannot be named here",
        ));
    }
    Ok(name)
}

/// Deletes `<data_dir>/glovebox-<name>.db` and its `-wal` and `-shm` files and
/// returns the paths removed. A missing file is skipped. The file name is built
/// from a validated non-live name, so the live database is never a target.
pub fn reset(data_dir: &Path, name: &str) -> DomainResult<Vec<PathBuf>> {
    let name = test_name(name)?;
    let mut removed = Vec::new();
    for suffix in ["", "-wal", "-shm"] {
        let path = data_dir.join(format!("glovebox-{name}.db{suffix}"));
        match std::fs::remove_file(&path) {
            Ok(()) => removed.push(path),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                return Err(DomainError::Internal(format!(
                    "cannot delete {}: {e}",
                    path.display()
                )));
            }
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none(_: &str) -> Option<String> {
        None
    }

    fn with<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |k| {
            pairs
                .iter()
                .find(|(key, _)| *key == k)
                .map(|(_, v)| (*v).to_owned())
        }
    }

    #[test]
    fn default_is_live_database_and_port() {
        let env = resolve(none).unwrap();
        assert_eq!(env.name, "live");
        assert!(env.is_live());
        assert_eq!(env.db_path, "data/glovebox.db");
        assert_eq!(env.web_port, 4827);
    }

    #[test]
    fn named_env_uses_suffixed_file_and_other_port() {
        let env = resolve(with(&[("GLOVEBOX_ENV", "test")])).unwrap();
        assert_eq!(env.name, "test");
        assert!(!env.is_live());
        assert_eq!(env.db_path, "data/glovebox-test.db");
        assert_eq!(env.web_port, 4828);
        let env = resolve(with(&[
            ("GLOVEBOX_ENV", "test"),
            ("GLOVEBOX_WEB_PORT", "5000"),
        ]))
        .unwrap();
        assert_eq!(env.web_port, 5000);
    }

    #[test]
    fn explicit_db_path_wins_and_keeps_env_name() {
        let env = resolve(with(&[
            ("GLOVEBOX_ENV", "test"),
            ("GLOVEBOX_DB_PATH", "/abs/x.db"),
        ]))
        .unwrap();
        assert_eq!(env.name, "test");
        assert_eq!(env.db_path, "/abs/x.db");
        assert_eq!(env.web_port, 4828);
    }

    #[test]
    fn bad_env_name_is_invalid() {
        let long = "a".repeat(33);
        for bad in ["../x", "Test", "", long.as_str()] {
            let err = resolve(with(&[("GLOVEBOX_ENV", bad)])).unwrap_err();
            assert!(
                matches!(&err, DomainError::Invalid { field, .. } if field == "GLOVEBOX_ENV"),
                "{bad:?} gave {err:?}"
            );
        }
    }

    fn touch(dir: &Path, names: &[&str]) {
        for n in names {
            std::fs::write(dir.join(n), b"x").unwrap();
        }
    }

    #[test]
    fn reset_deletes_only_the_named_environments_files() {
        let dir = tempfile::tempdir().unwrap();
        touch(
            dir.path(),
            &[
                "glovebox.db",
                "glovebox.db-wal",
                "glovebox-test.db",
                "glovebox-test.db-wal",
                "glovebox-test.db-shm",
                "glovebox-other.db",
            ],
        );
        std::fs::create_dir(dir.path().join("backups")).unwrap();
        let removed = reset(dir.path(), "test").unwrap();
        assert_eq!(removed.len(), 3);
        for gone in [
            "glovebox-test.db",
            "glovebox-test.db-wal",
            "glovebox-test.db-shm",
        ] {
            assert!(!dir.path().join(gone).exists(), "{gone} should be gone");
        }
        for kept in [
            "glovebox.db",
            "glovebox.db-wal",
            "glovebox-other.db",
            "backups",
        ] {
            assert!(dir.path().join(kept).exists(), "{kept} should remain");
        }
    }

    #[test]
    fn reset_refuses_live_and_bad_names() {
        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), &["glovebox.db", "glovebox-test.db"]);
        for bad in ["live", "", "../glovebox", "x/../y"] {
            let err = reset(dir.path(), bad).unwrap_err();
            assert!(
                matches!(err, DomainError::Invalid { .. }),
                "{bad:?}: {err:?}"
            );
        }
        assert!(dir.path().join("glovebox.db").exists());
        assert!(dir.path().join("glovebox-test.db").exists());
    }

    #[test]
    fn reset_of_a_missing_environment_removes_nothing_and_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        assert!(reset(dir.path(), "test").unwrap().is_empty());
    }
}
