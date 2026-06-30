//! 环境初始化：渲染器选择、开机启动、开发模式检测。

/// 必须在创建 Slint 窗口/托盘之前调用。
pub fn apply_renderer_setting(renderer: &str) {
    if std::env::var_os("SLINT_BACKEND").is_some() {
        return; // 用户已通过环境变量自行指定
    }

    let renderer = renderer.trim().to_lowercase();
    let backend = match renderer.as_str() {
        "femtovg" | "opengl" | "gl" => "winit-femtovg",
        "software" | "sw" | "" => "winit-software",
        other => {
            eprintln!("未知 renderer 配置：{other}，使用 software 渲染器");
            "winit-software"
        }
    };

    std::env::set_var("SLINT_BACKEND", backend);
}

/// 根据配置写入/移除当前用户的 Windows 开机启动项。
pub fn apply_start_on_boot_setting(enabled: bool) -> Result<(), Box<dyn std::error::Error>> {
    set_start_on_boot(enabled)
}

#[cfg(windows)]
fn set_start_on_boot(enabled: bool) -> Result<(), Box<dyn std::error::Error>> {
    use winreg::{enums::HKEY_CURRENT_USER, RegKey};

    const APP_NAME: &str = "ScreenReminder";
    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (run_key, _) = hkcu.create_subkey(RUN_KEY)?;

    if enabled {
        let exe = std::env::current_exe()?;
        let command = format!("\"{}\"", exe.display());
        run_key.set_value(APP_NAME, &command)?;
    } else {
        match run_key.delete_value(APP_NAME) {
            Ok(_) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(err.into()),
        }
    }

    Ok(())
}

#[cfg(not(windows))]
fn set_start_on_boot(enabled: bool) -> Result<(), Box<dyn std::error::Error>> {
    if enabled {
        eprintln!("当前系统暂未实现自动配置开机启动，请手动添加启动项。")
    }
    Ok(())
}

/// 检测是否为开发/立即测试模式。
pub fn is_dev_immediate_mode() -> bool {
    std::env::args().any(|arg| arg == "--dev-immediate")
        || std::env::var("SCREEN_REMINDER_DEV_IMMEDIATE").is_ok_and(|value| {
            matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES")
        })
}
