use clap::Parser;
use std::path::PathBuf;
use whatsapp_tui::{
    config::{self, Paths},
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
    let args: Vec<_> = std::env::args_os().collect();
    if args.len() == 2 && args[1] == "--media-worker" {
        return whatsapp_tui::media::run_worker()
            .await
            .map_err(|_| AppError::Arguments("Media worker failed"));
    }
    let options = Options::parse();
    let home = config::home_dir().ok_or(AppError::Arguments("Home directory is unavailable"))?;
    #[cfg(unix)]
    let xdg = config::XdgDirs {
        config: std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
        data: std::env::var_os("XDG_DATA_HOME").map(PathBuf::from),
        state: std::env::var_os("XDG_STATE_HOME").map(PathBuf::from),
    };
    #[cfg(unix)]
    let paths = Paths::resolve(&home, &xdg);
    #[cfg(windows)]
    let paths = Paths::resolve_windows(
        &home,
        std::env::var_os("APPDATA").map(PathBuf::from).as_deref(),
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .as_deref(),
    );
    runtime::prepare_with(options, paths, whatsapp_tui::whatsapp::start)
        .await?
        .run()
        .await
}
