//! The `glovebox` command: the operations the owner runs by hand.

use std::path::Path;

use clap::{Parser, Subcommand};
use glovebox_shared::{
    db,
    env::{self, Environment},
    error::{DomainError, DomainResult},
    seed::{self, SeedSummary},
};

#[derive(Debug, Parser)]
#[command(
    name = "glovebox",
    about = "Maintenance commands for the glovebox database"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Copy the current environment's database to its `backups` directory.
    Backup,
    /// Delete a named non-live environment's database files.
    Reset {
        /// The environment to delete; `live` is always refused.
        #[arg(long)]
        env: String,
    },
    /// Fill a named non-live environment with made-up demo data.
    Seed {
        /// The environment to fill; `live` is always refused.
        #[arg(long)]
        env: String,
    },
}

/// Runs one command and returns the text to print. `data_dir` is where the
/// named environments' database files live; `lookup` reads environment
/// variables.
///
/// # Errors
///
/// `backup` fails when the database file is missing or empty. `reset` fails
/// for `live` and for an invalid environment name. `seed` fails for those
/// too, and when the environment's database already holds a vehicle.
pub async fn run(
    args: Cli,
    lookup: impl Fn(&str) -> Option<String>,
    data_dir: &Path,
) -> DomainResult<String> {
    match args.command {
        Command::Backup => backup(&env::resolve(lookup)?).await,
        // `GLOVEBOX_DB_PATH` is deliberately not consulted: the files to
        // delete come from `data_dir` and the validated name alone.
        Command::Reset { env } => {
            let removed = env::reset(data_dir, &env)?;
            if removed.is_empty() {
                return Ok(format!("Nothing to delete for environment {env}."));
            }
            let list: Vec<String> = removed.iter().map(|p| p.display().to_string()).collect();
            Ok(format!("Deleted {}", list.join(", ")))
        }
        // Like `reset`, the file comes from `data_dir` and the validated name
        // alone, so `GLOVEBOX_DB_PATH` cannot point the seed at live data.
        Command::Seed { env } => {
            let name = env::test_name(&env)?;
            let path = data_dir.join(format!("glovebox-{name}.db"));
            let conn = db::connect(&path.to_string_lossy()).await?;
            let summary = seed::demo(&conn).await?;
            conn.close().await?;
            Ok(format!(
                "Seeded environment {name} at {}: {}.",
                path.display(),
                describe(&summary)
            ))
        }
    }
}

fn describe(s: &SeedSummary) -> String {
    let count = |n: u64, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
    format!(
        "{}, {}, {}, {}, {}, {} and {}",
        count(s.vehicles, "vehicle", "vehicles"),
        count(s.appointments, "appointment", "appointments"),
        count(s.jobs, "job", "jobs"),
        count(s.needs, "need", "needs"),
        count(s.options, "option", "options"),
        count(s.parts, "part", "parts"),
        count(s.todos, "to-do", "to-dos"),
    )
}

async fn backup(environment: &Environment) -> DomainResult<String> {
    let source = std::path::absolute(&environment.db_path)
        .map_or_else(|_| environment.db_path.clone(), |p| p.display().to_string());
    let Some(copy) = db::backup_now(&environment.db_path).await? else {
        return Err(DomainError::BadRequest(format!(
            "nothing to back up: {source} is missing or empty"
        )));
    };
    Ok(format!("Backed up {source} to {}", copy.display()))
}
