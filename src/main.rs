#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod config;
mod scheduler;
mod startup;
mod window;

use config::load_config;
use scheduler::{send_immediate_test_reminders, start_scheduler, ReminderEvent};
use slint::ComponentHandle;
use startup::{apply_renderer_setting, apply_start_on_boot_setting, is_dev_immediate_mode};
use std::{
    cell::RefCell,
    collections::HashMap,
    rc::Rc,
    time::{Duration, Instant},
};
use window::{apply_reminder_window_size, defer_hide, primary_screen_size_physical, show_reminder};

slint::include_modules!();

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = load_config("reminders.toml")?;
    apply_renderer_setting(&config.settings.renderer);
    apply_start_on_boot_setting(config.settings.start_on_boot)?;

    // ---- 提醒窗口 ----
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
    apply_reminder_window_size(&ui);
    ui.hide()?;

    // ---- 系统托盘 ----
    let tray = AppTray::new()?;
    tray.show()?;

    // ---- 共享状态 ----
    let ui_weak = ui.as_weak();
    let snoozed: Rc<RefCell<HashMap<usize, Instant>>> = Rc::new(RefCell::new(HashMap::new()));
    let current_event: Rc<RefCell<Option<ReminderEvent>>> = Rc::new(RefCell::new(None));

    // ---- 回调 ----
    {
        let ui_weak = ui_weak.clone();
        let cur = current_event.clone();
        ui.on_dismissed(move || {
            *cur.borrow_mut() = None;
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
        let cur = current_event.clone();
        ui.on_snooze(move || {
            if let Some(event) = cur.borrow().clone() {
                snoozed
                    .borrow_mut()
                    .insert(event.id, Instant::now() + Duration::from_secs(5 * 60));
            }
            *cur.borrow_mut() = None;
            if let Some(ui) = ui_weak.upgrade() {
                defer_hide(ui);
            }
        });
    }

    // ---- 定时调度 ----
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
