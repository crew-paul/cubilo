//! Startup failures must remain readable after a double-clicked process exits.
use std::{
    fs::{self, File},
    io::Write,
    path::PathBuf,
    sync::Mutex,
};

struct FileLog(Mutex<File>);
impl log::Log for FileLog {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Info
    }
    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            if let Ok(mut file) = self.0.lock() {
                let _ = writeln!(
                    file,
                    "{} [{}] {}",
                    record.level(),
                    record.target(),
                    record.args()
                );
                let _ = file.flush();
            }
        }
    }
    fn flush(&self) {
        if let Ok(mut file) = self.0.lock() {
            let _ = file.flush();
        }
    }
}
fn install_log() -> Option<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(base) = directories::BaseDirs::new() {
        dirs.push(base.data_local_dir().join("Cubilo"));
    }
    dirs.push(std::env::temp_dir().join("Cubilo"));
    for dir in dirs {
        if fs::create_dir_all(&dir).is_err() {
            continue;
        }
        let path = dir.join("startup.log");
        if let Ok(file) = File::create(&path) {
            if log::set_boxed_logger(Box::new(FileLog(Mutex::new(file)))).is_ok() {
                log::set_max_level(log::LevelFilter::Info);
                return Some(path);
            }
        }
    }
    None
}
pub fn run(smoke: bool) -> Result<(), String> {
    let path = install_log();
    log::info!(
        "Cubilo startup: {} {}, graphics={:?}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        std::env::var("WGPU_BACKEND")
    );
    let old_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::error!("{info}\n{}", std::backtrace::Backtrace::force_capture());
        old_hook(info);
    }));
    let result = std::panic::catch_unwind(|| crate::gui::run(smoke));
    let error = match result {
        Ok(Ok(())) => {
            log::info!("Cubilo exited normally");
            return Ok(());
        }
        Ok(Err(error)) => error.to_string(),
        Err(payload) => payload
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_else(|| "그래픽 초기화 중 예기치 않은 오류가 발생했습니다".into()),
    };
    log::error!("Startup failed: {error}");
    let message = format!(
        "큐빌로를 시작하지 못했습니다.\n\n{error}\n\n오류 로그: {}",
        path.as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "로그를 저장할 수 없습니다".into())
    );
    eprintln!("{message}");
    if cfg!(windows) && !smoke {
        rfd::MessageDialog::new()
            .set_title("큐빌로 시작 오류")
            .set_description(&message)
            .set_level(rfd::MessageLevel::Error)
            .show();
    }
    Err(error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use log::Log;
    #[test]
    fn startup_error_is_flushed_to_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("startup.log");
        let logger = FileLog(Mutex::new(File::create(&path).unwrap()));
        logger.log(
            &log::Record::builder()
                .args(format_args!("adapter creation failed"))
                .level(log::Level::Error)
                .target("wgpu")
                .build(),
        );
        assert!(fs::read_to_string(path)
            .unwrap()
            .contains("ERROR [wgpu] adapter creation failed"));
    }
}
