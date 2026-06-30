use serde::{Deserialize, Deserializer};
use std::{
    fs,
    path::{Path, PathBuf},
};

/// 全局配置（reminders.toml 的顶层）
#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub settings: AppSettings,
    pub reminders: Vec<ReminderConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AppSettings {
    /// 是否写入当前用户的开机启动项
    #[serde(default)]
    pub start_on_boot: bool,
    /// 弹窗显示后，是否允许按 Esc 立即关闭
    #[serde(default = "default_close_on_escape")]
    pub close_on_escape: bool,
    /// 是否使用全屏提醒
    #[serde(default)]
    pub fullscreen_reminder: bool,
    /// Slint 渲染器：software 更稳定省内存；femtovg 使用 OpenGL
    #[serde(default = "default_renderer")]
    pub renderer: String,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeekdayConfig {
    Mon,
    Tue,
    Wed,
    Thu,
    Fri,
    Sat,
    Sun,
}

impl WeekdayConfig {
    pub fn number_from_monday(self) -> u32 {
        match self {
            Self::Mon => 1,
            Self::Tue => 2,
            Self::Wed => 3,
            Self::Thu => 4,
            Self::Fri => 5,
            Self::Sat => 6,
            Self::Sun => 7,
        }
    }
}

impl<'de> Deserialize<'de> for WeekdayConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        parse_weekday(&value).ok_or_else(|| {
            serde::de::Error::custom(format!(
                "无效星期值：{value}，支持 mon/tue/.../sun 或 周一/周二/.../周日"
            ))
        })
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct ReminderConfig {
    pub title: String,
    pub message: String,
    /// 每天固定时间，格式 HH:MM，例如 "09:30"
    pub at: Option<String>,
    /// 每隔多少分钟提醒一次
    pub every_minutes: Option<u64>,
    /// 生效开始时间，格式 HH:MM，例如 "09:00"。不填表示不限制。
    pub active_start: Option<String>,
    /// 生效结束时间，格式 HH:MM，例如 "22:00"。不填表示不限制。
    pub active_end: Option<String>,
    /// 生效开始星期。不填表示不限制。
    pub weekday_start: Option<WeekdayConfig>,
    /// 生效结束星期。不填表示不限制。
    pub weekday_end: Option<WeekdayConfig>,
}

// ---------------------------------------------------------------------------
// 加载与校验
// ---------------------------------------------------------------------------

pub fn load_config(path: impl AsRef<Path>) -> Result<Config, Box<dyn std::error::Error>> {
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
        if let Some(active_start) = &reminder.active_start {
            parse_time(active_start)
                .map_err(|_| format!("第 {} 个提醒的 active_start 格式错误，应为 HH:MM", index + 1))?;
        }
        if let Some(active_end) = &reminder.active_end {
            parse_time(active_end)
                .map_err(|_| format!("第 {} 个提醒的 active_end 格式错误，应为 HH:MM", index + 1))?;
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
# 每条提醒可选限制生效时间段和星期范围：
# active_start = "09:00"
# active_end = "22:00"
# weekday_start = "mon"
# weekday_end = "fri"

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
active_start = "09:00"
active_end = "22:00"
weekday_start = "mon"
weekday_end = "fri"

[[reminders]]
title = "休息眼睛"
message = "看看远处，放松眼睛 1 分钟。"
every_minutes = 45
active_start = "09:00"
active_end = "22:00"
weekday_start = "mon"
weekday_end = "fri"
"#,
    )?;

    Ok(())
}

fn parse_time(time: &str) -> Result<chrono::NaiveTime, chrono::ParseError> {
    chrono::NaiveTime::parse_from_str(time, "%H:%M")
}

fn parse_weekday(value: &str) -> Option<WeekdayConfig> {
    match value.trim().to_lowercase().as_str() {
        "1" | "mon" | "monday" | "周一" | "星期一" | "一" => Some(WeekdayConfig::Mon),
        "2" | "tue" | "tues" | "tuesday" | "周二" | "星期二" | "二" => Some(WeekdayConfig::Tue),
        "3" | "wed" | "wednesday" | "周三" | "星期三" | "三" => Some(WeekdayConfig::Wed),
        "4" | "thu" | "thur" | "thurs" | "thursday" | "周四" | "星期四" | "四" => Some(WeekdayConfig::Thu),
        "5" | "fri" | "friday" | "周五" | "星期五" | "五" => Some(WeekdayConfig::Fri),
        "6" | "sat" | "saturday" | "周六" | "星期六" | "六" => Some(WeekdayConfig::Sat),
        "7" | "sun" | "sunday" | "周日" | "周天" | "星期日" | "星期天" | "日" | "天" => Some(WeekdayConfig::Sun),
        _ => None,
    }
}
