use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[command(name = "glovebox", about = "Car maintenance tracker")]
pub struct AppConfig {
    /// Path to the `SQLite` database file
    #[arg(long, default_value = "data/glovebox.db", env = "GLOVEBOX_DB_PATH")]
    pub db_path: String,
}
