use chrono::{Local, NaiveTime, Timelike};
use serde::Deserialize;
use slint::{ComponentHandle, PhysicalPosition, PhysicalSize};
use std::{
    cell::RefCell,
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    rc::Rc,
    thread,
    time::{Duration, Instant},
};

slint::include_modules!();

#[derive(Debug, Deserialize)]
struct Config {
    #[serde(default)]
    settings: AppSettings,
    reminders: Vec<ReminderConfig>,
}

#[derive(Debug, Deserialize, Clone)]
struct AppSettings {
    /// 是否写入当前用户的开机启动项
    #[serde(default)]
    start_on_boot: bool,
    /// 弹窗显示后，是否允许按 Esc 立即关闭
    #[serde(default = "default_close_on_escape")]
    close_on_escape: bool,
    /// 是否使用全屏提醒
    #[serde(default)]
    fullscreen_reminder: bool,
    /// Slint 渲染器：software 更稳定省内存；femtovg 使用 OpenGL
    #[serde(default = "default_renderer")]
    renderer: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            start_on_boot: false,
            close_on_escape: true,
            fullscreen_reminder: false,
            renderer: default_renderer(),
        }
    }
}

fn default_close_on_escape() -> bool {
    true
}

fn default_renderer() -> String {
    "software".to_string()
}

#[derive(Debug, Deserialize, Clone)]
struct ReminderConfig {
    title: String,
    message: String,
    /// 每天固定时间，格式 HH:MM，例如 "09:30"
    at: Option<String>,
    /// 每隔多少分钟提醒一次
    every_minutes: Option<u64>,
}

#[derive(Debug, Clone)]
struct ReminderEvent {
    id: usize,
    title: String,
    message: String,
    trigger_text: String,
}

#[derive(Debug, Clone)]
struct ReminderRuntime {
    config: ReminderConfig,
    last_daily_date: Option<String>,
    next_interval_at: Option<Instant>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = load_config("reminders.toml")?;
    apply_renderer_setting(&config.settings.renderer);
    apply_start_on_boot_setting(config.settings.start_on_boot)?;

    let ui = ReminderWindow::new()?;
    ui.set_close_on_escape(config.settings.close_on_escape);
    ui.set_fullscreen_reminder(config.settings.fullscreen_reminder);

    let (screen_w, screen_h) = primary_screen_size_physical().unwrap_or((1920, 1080));
    if config.settings.fullscreen_reminder {
        ui.set_window_w(screen_w as f32);
        ui.set_window_h(screen_h as f32);
    } else {
        ui.set_window_w(720.0);
        ui.set_window_h(420.0);
    }

    // 首次计算并传入卡片几何，后续每次显示时会由 apply_reminder_window_size 刷新
    apply_reminder_window_size(&ui);

    ui.hide()?;

    let tray = AppTray::new()?;
    tray.show()?;

    let ui_weak = ui.as_weak();
    let snoozed: Rc<RefCell<HashMap<usize, Instant>>> = Rc::new(RefCell::new(HashMap::new()));
    let current_event: Rc<RefCell<Option<ReminderEvent>>> = Rc::new(RefCell::new(None));

    {
        let ui_weak = ui_weak.clone();
        let current_event = current_event.clone();
        ui.on_dismissed(move || {
            *current_event.borrow_mut() = None;
            if let Some(ui) = ui_weak.upgrade() {
                defer_hide(ui);
            }
        });
    }

    {
        let ui_weak = ui_weak.clone();
        tray.on_show_reminder(move || {
            show_reminder(
                &ui_weak,
                &ReminderEvent {
                    id: usize::MAX,
                    title: "测试提醒".to_string(),
                    message: "这是从系统托盘触发的测试提醒。".to_string(),
                    trigger_text: "系统托盘".to_string(),
                },
            );
        });
    }

    tray.on_quit(move || {
        let _ = slint::quit_event_loop();
    });

    {
        let ui_weak = ui_weak.clone();
        let snoozed = snoozed.clone();
        let current_event = current_event.clone();
        ui.on_snooze(move || {
            if let Some(event) = current_event.borrow().clone() {
                snoozed
                    .borrow_mut()
                    .insert(event.id, Instant::now() + Duration::from_secs(5 * 60));
            }
            *current_event.borrow_mut() = None;
            if let Some(ui) = ui_weak.upgrade() {
                defer_hide(ui);
            }
        });
    }

    let dev_immediate = is_dev_immediate_mode();
    let (tx, rx) = std::sync::mpsc::channel::<ReminderEvent>();

    if dev_immediate {
        send_immediate_test_reminders(&config.reminders, &tx);
    }

    start_scheduler(config.reminders, tx);

    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(500),
        move || {
            let now = Instant::now();
            snoozed.borrow_mut().retain(|_, at| *at > now);

            // 当前已有提醒窗口时，不打断；用户关闭/稍后提醒后再显示下一个。
            if current_event.borrow().is_some() {
                return;
            }

            while let Ok(event) = rx.try_recv() {
                if let Some(snooze_until) = snoozed.borrow().get(&event.id) {
                    if *snooze_until > now {
                        continue;
                    }
                }

                show_reminder(&ui_weak, &event);
                *current_event.borrow_mut() = Some(event);
                break;
            }
        },
    );

    slint::run_event_loop()?;
    Ok(())
}

fn is_dev_immediate_mode() -> bool {
    std::env::args().any(|arg| arg == "--dev-immediate")
        || std::env::var("SCREEN_REMINDER_DEV_IMMEDIATE").is_ok_and(|value| {
            matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES")
        })
}

fn apply_renderer_setting(renderer: &str) {
    // 必须在创建任何 Slint 窗口/托盘之前设置。
    // software 更适合这个提醒工具：稳定、内存更低、不依赖 OpenGL 驱动。
    if std::env::var_os("SLINT_BACKEND").is_some() {
        return;
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

fn apply_start_on_boot_setting(enabled: bool) -> Result<(), Box<dyn std::error::Error>> {
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

fn send_immediate_test_reminders(
    reminders: &[ReminderConfig],
    tx: &std::sync::mpsc::Sender<ReminderEvent>,
) {
    for (id, reminder) in reminders.iter().enumerate() {
        let _ = tx.send(ReminderEvent {
            id,
            title: format!("[测试] {}", reminder.title),
            message: reminder.message.clone(),
            trigger_text: "开发环境立即模式".to_string(),
        });
    }
}

fn load_config(path: impl AsRef<Path>) -> Result<Config, Box<dyn std::error::Error>> {
    let config_path = resolve_config_path(path.as_ref())?;
    let content = fs::read_to_string(&config_path)?;
    let config: Config = toml::from_str(&content)?;

    if config.reminders.is_empty() {
        return Err("reminders.toml 至少需要一个 [[reminders]]".into());
    }

    for (index, reminder) in config.reminders.iter().enumerate() {
        if reminder.at.is_none() && reminder.every_minutes.is_none() {
            return Err(format!(
                "第 {} 个提醒缺少 at 或 every_minutes",
                index + 1
            )
            .into());
        }
        if let Some(at) = &reminder.at {
            parse_time(at).map_err(|_| format!("第 {} 个提醒的 at 格式错误，应为 HH:MM", index + 1))?;
        }
    }

    Ok(config)
}

fn resolve_config_path(file_name: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let mut candidates = Vec::new();

    if file_name.is_absolute() {
        candidates.push(file_name.to_path_buf());
    } else {
        candidates.push(std::env::current_dir()?.join(file_name));

        if let Some(exe_dir) = std::env::current_exe()?.parent() {
            candidates.push(exe_dir.join(file_name));
        }

        candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(file_name));
    }

    for candidate in &candidates {
        if candidate.exists() {
            return Ok(candidate.clone());
        }
    }

    let fallback = candidates
        .first()
        .cloned()
        .unwrap_or_else(|| PathBuf::from(file_name));

    create_default_config(&fallback)?;
    Ok(fallback)
}

fn create_default_config(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::write(
        path,
        r#"# 屏幕提醒配置
# 支持两种定时：
# 1. at = "HH:MM"       每天固定时间触发
# 2. every_minutes = 30  每隔 N 分钟触发一次

[settings]
# true：程序启动时自动写入当前用户的开机启动项；false：自动移除开机启动项
start_on_boot = false
# true：提醒窗口弹出后，按 Esc 立即关闭窗口；false：Esc 不关闭
close_on_escape = true
# true：全屏提醒；false：普通置顶窗口提醒
fullscreen_reminder = false
# 渲染器：software 更稳定省内存；femtovg 使用 OpenGL
renderer = "software"

[[reminders]]
title = "喝水提醒"
message = "起来活动一下，喝一杯水。"
at = "09:30"

[[reminders]]
title = "休息眼睛"
message = "看看远处，放松眼睛 1 分钟。"
every_minutes = 45
"#,
    )?;

    Ok(())
}

fn start_scheduler(reminders: Vec<ReminderConfig>, tx: std::sync::mpsc::Sender<ReminderEvent>) {
    thread::spawn(move || {
        let mut runtimes: Vec<ReminderRuntime> = reminders
            .into_iter()
            .map(|config| ReminderRuntime {
                next_interval_at: config
                    .every_minutes
                    .map(|minutes| Instant::now() + Duration::from_secs(minutes * 60)),
                config,
                last_daily_date: None,
            })
            .collect();

        loop {
            let now = Local::now();
            let today = now.format("%Y-%m-%d").to_string();
            let now_time = now.time();
            let instant_now = Instant::now();

            for (id, runtime) in runtimes.iter_mut().enumerate() {
                if let Some(at) = &runtime.config.at {
                    if let Ok(target) = parse_time(at) {
                        // 每天在目标分钟内触发一次。
                        if now_time.hour() == target.hour()
                            && now_time.minute() == target.minute()
                            && runtime.last_daily_date.as_deref() != Some(&today)
                        {
                            runtime.last_daily_date = Some(today.clone());
                            let _ = tx.send(ReminderEvent {
                                id,
                                title: runtime.config.title.clone(),
                                message: runtime.config.message.clone(),
                                trigger_text: format!("每天 {}", at),
                            });
                        }
                    }
                }

                if let Some(next_at) = runtime.next_interval_at {
                    if instant_now >= next_at {
                        let minutes = runtime.config.every_minutes.unwrap_or(1);
                        runtime.next_interval_at = Some(instant_now + Duration::from_secs(minutes * 60));
                        let _ = tx.send(ReminderEvent {
                            id,
                            title: runtime.config.title.clone(),
                            message: runtime.config.message.clone(),
                            trigger_text: format!("每 {} 分钟", minutes),
                        });
                    }
                }
            }

            thread::sleep(Duration::from_secs(1));
        }
    });
}

fn parse_time(time: &str) -> Result<NaiveTime, chrono::ParseError> {
    NaiveTime::parse_from_str(time, "%H:%M")
}

/// Slint 的 key-pressed 回调里直接调用 hide() 有时会在 Windows/winit 上
/// 出现"第一次关背景、第二次才关提示内容"的异步问题。
/// 用 invoke_from_event_loop 在下一个事件循环中隐藏窗口，避开该问题。
fn defer_hide(ui: ReminderWindow) {
    let timer = slint::Timer::default();
    let _ = timer.start(
        slint::TimerMode::SingleShot,
        Duration::from_millis(0),
        move || {
            let _ = ui.hide();
        },
    );
}

fn apply_reminder_window_size(ui: &ReminderWindow) {
    if ui.get_fullscreen_reminder() {
        if let Some((w, h)) = primary_screen_size_physical() {
            ui.window().set_position(PhysicalPosition::new(0, 0));
            ui.window().set_size(PhysicalSize::new(w, h));
            ui.set_window_w(w as f32);
            ui.set_window_h(h as f32);

            // 卡片尺寸
            let card_w = ((w as f32) - 64.0).min(780.0);
            let card_h = ((h as f32) - 64.0).min(460.0);
            ui.set_card_x(((w as f32 - card_w) / 2.0).max(0.0));
            ui.set_card_y(((h as f32 - card_h) / 2.0).max(0.0));
            ui.set_card_w(card_w);
            ui.set_card_h(card_h);
        }
    } else {
        ui.window().set_size(slint::LogicalSize::new(720.0, 420.0));
        ui.set_window_w(720.0);
        ui.set_window_h(420.0);

        let card_w = ((720.0f32) - 64.0).min(780.0);
        let card_h = ((420.0f32) - 64.0).min(460.0);
        ui.set_card_x(((720.0 - card_w) / 2.0).max(0.0));
        ui.set_card_y(((420.0 - card_h) / 2.0).max(0.0));
        ui.set_card_w(card_w);
        ui.set_card_h(card_h);
    }
}

fn primary_screen_size_physical() -> Option<(u32, u32)> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
        let w = unsafe { GetSystemMetrics(SM_CXSCREEN) };
        let h = unsafe { GetSystemMetrics(SM_CYSCREEN) };
        if w > 0 && h > 0 {
            return Some((w as u32, h as u32));
        }
    }
    #[cfg(not(windows))]
    {
        let _ = ();
    }
    None
}

fn show_reminder(ui_weak: &slint::Weak<ReminderWindow>, event: &ReminderEvent) {
    if let Some(ui) = ui_weak.upgrade() {
        apply_reminder_window_size(&ui);
        ui.set_reminder_title(event.title.clone().into());
        ui.set_reminder_message(event.message.clone().into());
        ui.set_reminder_time(
            format!(
                "{} 触发于 {}",
                event.trigger_text,
                Local::now().format("%Y-%m-%d %H:%M:%S")
            )
            .into(),
        );
        let _ = ui.show();
    }
}
