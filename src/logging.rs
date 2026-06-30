use std::{
    fs::OpenOptions,
    io::Write,
    path::PathBuf,
};

pub fn log_line(message: impl AsRef<str>) {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
    let line = format!("[{now}] {}\n", message.as_ref());

    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path())
    {
        let _ = file.write_all(line.as_bytes());
        let _ = file.flush();
    }
}

fn log_path() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            return dir.join("screen_reminder.log");
        }
    }

    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("screen_reminder.log")
}
