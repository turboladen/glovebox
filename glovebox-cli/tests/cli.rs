use std::path::Path;

use clap::Parser;
use glovebox_cli::{Cli, run};
use glovebox_shared::{db, error::DomainError, test_support::VehicleFixture};

fn cli(args: &[&str]) -> Cli {
    Cli::try_parse_from(args).unwrap()
}

fn env_for(db_path: &Path) -> impl Fn(&str) -> Option<String> {
    let db_path = db_path.display().to_string();
    move |k| (k == "GLOVEBOX_DB_PATH").then(|| db_path.clone())
}

#[tokio::test]
async fn backup_copies_the_resolved_database() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("glovebox.db");
    let conn = db::connect(path.to_str().unwrap()).await.unwrap();
    VehicleFixture::new().insert(&conn).await;
    conn.close().await.unwrap();

    let text = run(cli(&["glovebox", "backup"]), env_for(&path), dir.path())
        .await
        .unwrap();
    assert!(text.contains(path.to_str().unwrap()), "{text}");
    let copies = std::fs::read_dir(dir.path().join("backups"))
        .unwrap()
        .filter_map(Result::ok)
        .count();
    assert!(copies >= 1, "{text}");
}

#[tokio::test]
async fn backup_of_a_missing_database_is_an_error_and_creates_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("glovebox.db");
    let err = run(cli(&["glovebox", "backup"]), env_for(&path), dir.path())
        .await
        .unwrap_err();
    assert!(err.to_string().contains("nothing to back up"), "{err}");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[tokio::test]
async fn reset_requires_env_and_refuses_live() {
    assert!(Cli::try_parse_from(["glovebox", "reset"]).is_err());
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("glovebox.db"), b"x").unwrap();
    let err = run(
        cli(&["glovebox", "reset", "--env", "live"]),
        |_| None,
        dir.path(),
    )
    .await
    .unwrap_err();
    assert!(matches!(err, DomainError::Invalid { .. }), "{err:?}");
    assert!(dir.path().join("glovebox.db").exists());
}

#[tokio::test]
async fn reset_ignores_db_path_override() {
    let dir = tempfile::tempdir().unwrap();
    let live = dir.path().join("glovebox.db");
    std::fs::write(&live, b"x").unwrap();
    std::fs::write(dir.path().join("glovebox-test.db"), b"x").unwrap();
    run(
        cli(&["glovebox", "reset", "--env", "test"]),
        env_for(&live),
        dir.path(),
    )
    .await
    .unwrap();
    assert!(live.exists());
    assert!(!dir.path().join("glovebox-test.db").exists());
}
