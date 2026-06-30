use chrono::{Datelike, Local, NaiveTime, Timelike};
use std::{
    thread,
    time::{Duration, Instant},
};

use crate::{config::ReminderConfig, logging::log_line};

#[derive(Debug, Clone)]
pub struct ReminderEvent {
    pub id: usize,
    pub title: String,
    pub message: String,
    pub trigger_text: String,
}

#[derive(Debug, Clone)]
struct ReminderRuntime {
    config: ReminderConfig,
    last_daily_date: Option<String>,
    next_interval_at: Option<Instant>,
}

/// 在独立线程中启动定时检查，通过 channel 发送提醒事件。
pub fn start_scheduler(
    reminders: Vec<ReminderConfig>,
    tx: std::sync::mpsc::Sender<ReminderEvent>,
) {
    thread::spawn(move || {
        log_line(format!("scheduler thread started: reminders={}", reminders.len()));
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
                if !reminder_is_active(&runtime.config, &now) {
                    continue;
                }

                // 每天固定时间
                if let Some(at) = &runtime.config.at {
                    if let Ok(target) = parse_time(at) {
                        if now_time.hour() == target.hour()
                            && now_time.minute() == target.minute()
                            && runtime.last_daily_date.as_deref() != Some(&today)
                        {
                            runtime.last_daily_date = Some(today.clone());
                            log_line(format!(
                                "scheduler send daily event: id={}, title={}, at={}",
                                id, runtime.config.title, at
                            ));
                            let _ = tx.send(ReminderEvent {
                                id,
                                title: runtime.config.title.clone(),
                                message: runtime.config.message.clone(),
                                trigger_text: format!("每天 {}", at),
                            });
                        }
                    }
                }

                // 间隔提醒
                if let Some(next_at) = runtime.next_interval_at {
                    if instant_now >= next_at {
                        let minutes = runtime.config.every_minutes.unwrap_or(1);
                        runtime.next_interval_at =
                            Some(instant_now + Duration::from_secs(minutes * 60));
                        log_line(format!(
                            "scheduler send interval event: id={}, title={}, every_minutes={}",
                            id, runtime.config.title, minutes
                        ));
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

/// 开发用：启动后立即发送一条测试提醒。
///
/// 注意：不要把配置里的每条提醒都立即发送，否则测试关闭时会出现：
/// 第一次关闭第 1 条，调度器马上显示第 2 条，看起来就像需要按/点两次才关完。
pub fn send_immediate_test_reminders(
    reminders: &[ReminderConfig],
    tx: &std::sync::mpsc::Sender<ReminderEvent>,
) {
    if let Some(reminder) = reminders.first() {
        log_line(format!("dev immediate send event: id=0, title={}", reminder.title));
        let _ = tx.send(ReminderEvent {
            id: 0,
            title: format!("[测试] {}", reminder.title),
            message: reminder.message.clone(),
            trigger_text: "开发环境立即模式".to_string(),
        });
    }
}

fn reminder_is_active(config: &ReminderConfig, now: &chrono::DateTime<Local>) -> bool {
    is_active_weekday(config, now.weekday().number_from_monday()) && is_active_time(config, now.time())
}

fn is_active_time(config: &ReminderConfig, now_time: NaiveTime) -> bool {
    let start = config
        .active_start
        .as_deref()
        .and_then(|value| parse_time(value).ok())
        .unwrap_or_else(|| NaiveTime::from_hms_opt(0, 0, 0).unwrap());
    let end = config
        .active_end
        .as_deref()
        .and_then(|value| parse_time(value).ok())
        .unwrap_or_else(|| NaiveTime::from_hms_opt(23, 59, 59).unwrap());

    if start <= end {
        now_time >= start && now_time <= end
    } else {
        // 支持跨午夜，例如 22:00 - 06:00
        now_time >= start || now_time <= end
    }
}

fn is_active_weekday(config: &ReminderConfig, today: u32) -> bool {
    let start = config
        .weekday_start
        .map(|weekday| weekday.number_from_monday())
        .unwrap_or(1);
    let end = config
        .weekday_end
        .map(|weekday| weekday.number_from_monday())
        .unwrap_or(7);

    if start <= end {
        today >= start && today <= end
    } else {
        // 支持跨周，例如 fri - mon
        today >= start || today <= end
    }
}

fn parse_time(time: &str) -> Result<NaiveTime, chrono::ParseError> {
    NaiveTime::parse_from_str(time, "%H:%M")
}
