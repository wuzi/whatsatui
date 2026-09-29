use clap::Parser;
use std::path::PathBuf;
use whatsapp_tui::{
    config::{Paths, XdgDirs},
    runtime::{self, AppError, Options},
};
#[tokio::main]
async fn main() {
    if let Err(error) = start().await {
        eprintln!("whatsapp-tui: {error}");
        std::process::exit(1);
    }
}
async fn start() -> Result<(), AppError> {
    let options = Options::parse();
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .ok_or(AppError::Arguments("HOME must name an absolute directory"))?;
    let xdg = XdgDirs {
        config: std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
        data: std::env::var_os("XDG_DATA_HOME").map(PathBuf::from),
        state: std::env::var_os("XDG_STATE_HOME").map(PathBuf::from),
    };
    runtime::prepare_with(
        options,
        Paths::resolve(&home, &xdg),
        whatsapp_tui::whatsapp::start,
    )
    .await?
    .run()
    .await
}
