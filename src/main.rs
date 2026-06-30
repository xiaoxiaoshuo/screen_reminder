#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod config;
mod logging;
mod scheduler;
mod startup;
mod window;

use config::load_config;
use logging::log_line;
use scheduler::{send_immediate_test_reminders, start_scheduler, ReminderEvent};
use startup::{apply_renderer_setting, apply_start_on_boot_setting, is_dev_immediate_mode};
use std::{
    cell::RefCell,
    collections::{HashMap, VecDeque},
    rc::Rc,
    time::{Duration, Instant},
};
use window::{close_window, show_reminder};

slint::include_modules!();

fn main() -> Result<(), Box<dyn std::error::Error>> {
    log_line("app starting");
    let config = load_config("reminders.toml")?;
    log_line(format!(
        "config loaded: reminders={}, fullscreen={}, close_on_escape={}, renderer={}",
        config.reminders.len(),
        config.settings.fullscreen_reminder,
        config.settings.close_on_escape,
        config.settings.renderer
    ));
    apply_renderer_setting(&config.settings.renderer);
    apply_start_on_boot_setting(config.settings.start_on_boot)?;

    let close_on_escape = config.settings.close_on_escape;
    let fullscreen_reminder = config.settings.fullscreen_reminder;

    // ---- 系统托盘 ----
    let tray = AppTray::new()?;
    tray.show()?;

    // ---- 共享状态 ----
    let snoozed: Rc<RefCell<HashMap<usize, Instant>>> = Rc::new(RefCell::new(HashMap::new()));
    let current_event: Rc<RefCell<Option<ReminderEvent>>> = Rc::new(RefCell::new(None));
    let current_window: Rc<RefCell<Option<ReminderWindow>>> = Rc::new(RefCell::new(None));
    let pending_events: Rc<RefCell<VecDeque<ReminderEvent>>> = Rc::new(RefCell::new(VecDeque::new()));

    // ---- 回调 ----
    {
        let cur = current_event.clone();
        let current_window = current_window.clone();
        let snoozed = snoozed.clone();
        tray.on_show_reminder(move || {
            log_line("tray show_reminder clicked");
            let event = ReminderEvent {
                id: usize::MAX,
                title: "测试提醒".to_string(),
                message: "这是从系统托盘触发的测试提醒。".to_string(),
                trigger_text: "系统托盘".to_string(),
            };
            let _ = show_event_in_new_window(
                event,
                close_on_escape,
                fullscreen_reminder,
                cur.clone(),
                current_window.clone(),
                snoozed.clone(),
            );
        });
    }

    tray.on_quit(move || {
        log_line("tray quit clicked");
        let _ = slint::quit_event_loop();
    });

    // ---- 定时调度 ----
    let dev_immediate = is_dev_immediate_mode();
    let (tx, rx) = std::sync::mpsc::channel::<ReminderEvent>();

    if dev_immediate {
        log_line("dev immediate mode enabled");
        send_immediate_test_reminders(&config.reminders, &tx);
    }

    log_line("start scheduler thread");
    start_scheduler(config.reminders, tx);

    let pending_events_for_timer = pending_events.clone();
    let current_window_for_timer = current_window.clone();
    let current_event_for_timer = current_event.clone();
    let snoozed_for_timer = snoozed.clone();
    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(500),
        move || {
            let now = Instant::now();
            snoozed_for_timer.borrow_mut().retain(|_, at| *at > now);

            // 先把调度线程发来的事件收进显式队列。
            // 如果当前窗口还没关，新的提醒不会覆盖当前提醒，而是排队等待。
            while let Ok(event) = rx.try_recv() {
                log_line(format!(
                    "main timer received event: id={}, title={}, trigger={}",
                    event.id, event.title, event.trigger_text
                ));
                let mut pending = pending_events_for_timer.borrow_mut();
                // 相同提醒重复触发时，只保留最新一条，避免用户长时间不关导致队列无限增长。
                if let Some(pos) = pending.iter().position(|item| item.id == event.id) {
                    pending.remove(pos);
                }
                pending.push_back(event);
            }

            if current_event_for_timer.borrow().is_some() {
                return;
            }

            while let Some(event) = pending_events_for_timer.borrow_mut().pop_front() {
                if let Some(snooze_until) = snoozed_for_timer.borrow().get(&event.id) {
                    if *snooze_until > now {
                        continue;
                    }
                }

                log_line(format!("main timer show event: id={}, title={}", event.id, event.title));
                if show_event_in_new_window(
                    event.clone(),
                    close_on_escape,
                    fullscreen_reminder,
                    current_event_for_timer.clone(),
                    current_window_for_timer.clone(),
                    snoozed_for_timer.clone(),
                ) {
                    log_line(format!("main timer show success: id={}, title={}", event.id, event.title));
                    break;
                } else {
                    log_line(format!("main timer show failed: id={}, title={}", event.id, event.title));
                }
            }
        },
    );

    slint::run_event_loop()?;
    Ok(())
}

fn show_event_in_new_window(
    event: ReminderEvent,
    close_on_escape: bool,
    fullscreen_reminder: bool,
    current_event: Rc<RefCell<Option<ReminderEvent>>>,
    current_window: Rc<RefCell<Option<ReminderWindow>>>,
    snoozed: Rc<RefCell<HashMap<usize, Instant>>>,
) -> bool {
    if current_window.borrow().is_some() {
        log_line("show_event_in_new_window skipped: current window already exists");
        return false;
    }

    let ui = match ReminderWindow::new() {
        Ok(ui) => ui,
        Err(err) => {
            log_line(format!("ReminderWindow::new failed: {err}"));
            return false;
        }
    };

    ui.set_close_on_escape(close_on_escape);
    ui.set_fullscreen_reminder(fullscreen_reminder);

    ui.on_debug_log(|message| {
        log_line(format!("ui debug: {message}"));
    });

    {
        let cur = current_event.clone();
        let window_slot = current_window.clone();
        ui.on_dismissed(move || {
            log_line("dismissed callback fired");
            *cur.borrow_mut() = None;
            if let Some(window) = window_slot.borrow_mut().take() {
                log_line("dismissed: close current window");
                close_window(window);
            } else {
                log_line("dismissed: current window already empty");
            }
        });
    }

    {
        let cur = current_event.clone();
        let window_slot = current_window.clone();
        let snoozed = snoozed.clone();
        let event_for_snooze = event.clone();
        ui.on_snooze(move || {
            log_line("snooze callback fired");
            log_line(format!(
                "snooze event: id={}, title={}",
                event_for_snooze.id, event_for_snooze.title
            ));
            snoozed
                .borrow_mut()
                .insert(event_for_snooze.id, Instant::now() + Duration::from_secs(5 * 60));
            *cur.borrow_mut() = None;
            if let Some(window) = window_slot.borrow_mut().take() {
                log_line("snooze: close current window");
                close_window(window);
            } else {
                log_line("snooze: current window already empty");
            }
        });
    }

    if show_reminder(&ui, &event) {
        *current_event.borrow_mut() = Some(event);
        *current_window.borrow_mut() = Some(ui);
        true
    } else {
        false
    }
}
