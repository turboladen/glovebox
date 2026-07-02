use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[command(name = "glovebox", about = "Car maintenance tracker")]
pub struct AppConfig {
    /// Path to the `SQLite` database file
    #[arg(long, default_value = "data/glovebox.db", env = "GLOVEBOX_DB_PATH")]
    pub db_path: String,

    /// Address to listen on
    #[arg(long, default_value = "0.0.0.0:3003", env = "GLOVEBOX_LISTEN")]
    pub listen: String,

    /// Directory for uploaded files
    #[arg(long, default_value = "data/files", env = "GLOVEBOX_FILES_DIR")]
    pub files_dir: String,
}
