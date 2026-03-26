use std::{
    fs::OpenOptions,
    io::Write,
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

static LOGGER: OnceLock<Option<Mutex<std::fs::File>>> = OnceLock::new();

pub(crate) fn enabled() -> bool {
    LOGGER.get_or_init(init).is_some()
}

pub(crate) fn log(message: impl AsRef<str>) {
    let Some(file_lock) = LOGGER.get_or_init(init) else {
        return;
    };

    let Ok(mut file) = file_lock.lock() else {
        return;
    };

    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);

    let _ = writeln!(file, "[{}] {}", ts, message.as_ref());
}

fn init() -> Option<Mutex<std::fs::File>> {
    let path = std::env::var("EGUI_TERM_DEBUG_LOG").ok()?;
    if path.trim().is_empty() {
        return None;
    }

    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .ok()?;

    Some(Mutex::new(file))
}
