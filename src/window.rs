use crate::{logging::log_line, ReminderWindow};
use slint::{ComponentHandle, PhysicalPosition, PhysicalSize};

/// 根据全屏/窗口模式设置窗口尺寸与卡片几何。
///
/// 注意：Slint UI 里的 `length` 是逻辑像素，不是物理像素。
/// Windows 的 GetSystemMetrics 返回物理像素；如果直接把物理像素传给
/// Slint 属性，在 125%/150% 缩放下内容会明显偏向右下角。
pub fn apply_reminder_window_size(ui: &ReminderWindow) {
    if ui.get_fullscreen_reminder() {
        if let Some((physical_w, physical_h)) = primary_screen_size_physical() {
            ui.window().set_position(PhysicalPosition::new(0, 0));
            ui.window()
                .set_size(PhysicalSize::new(physical_w, physical_h));

            let scale = effective_scale_factor(ui);
            let logical_w = physical_w as f32 / scale;
            let logical_h = physical_h as f32 / scale;
            log_line(format!(
                "apply fullscreen size: physical={}x{}, scale={:.2}, logical={:.1}x{:.1}",
                physical_w, physical_h, scale, logical_w, logical_h
            ));
            apply_content_geometry(ui, logical_w, logical_h);
        }
    } else {
        ui.window()
            .set_size(slint::LogicalSize::new(720.0, 420.0));
        apply_content_geometry(ui, 720.0, 420.0);
    }
}

fn apply_content_geometry(ui: &ReminderWindow, logical_w: f32, logical_h: f32) {
    ui.set_window_w(logical_w);
    ui.set_window_h(logical_h);

    let card_w = (logical_w - 64.0).min(780.0).max(320.0);
    let card_h = (logical_h - 64.0).min(460.0).max(260.0);
    ui.set_card_x(((logical_w - card_w) / 2.0).max(0.0));
    ui.set_card_y(((logical_h - card_h) / 2.0).max(0.0));
    ui.set_card_w(card_w);
    ui.set_card_h(card_h);
}

fn effective_scale_factor(ui: &ReminderWindow) -> f32 {
    let mut scale = ui.window().scale_factor().max(1.0);

    #[cfg(windows)]
    {
        if let Some(hwnd) = hwnd_for(ui) {
            let dpi = unsafe { windows_sys::Win32::UI::HiDpi::GetDpiForWindow(hwnd) };
            if dpi > 0 {
                scale = scale.max(dpi as f32 / 96.0);
            }
        }

        // 新建窗口刚 show 的瞬间，winit/Slint 有时还拿不到正确 HWND DPI，
        // 这时 GetDpiForWindow/scale_factor 可能都是 1.0。
        // 用系统 DPI 做兜底，避免 3840x2160 被误当成逻辑像素导致卡片偏离中心。
        let system_dpi = unsafe { windows_sys::Win32::UI::HiDpi::GetDpiForSystem() };
        if system_dpi > 0 {
            scale = scale.max(system_dpi as f32 / 96.0);
        }
    }

    scale.max(1.0)
}

/// 关闭并释放提醒窗口。
pub fn close_window(ui: ReminderWindow) {
    log_line("close_window requested");
    match ui.hide() {
        Ok(_) => log_line("window.hide success"),
        Err(err) => log_line(format!("window.hide failed: {err}")),
    }
}

/// 显示一次提醒弹窗。返回 true 表示窗口确实 show 成功。
pub fn show_reminder(ui: &ReminderWindow, event: &crate::scheduler::ReminderEvent) -> bool {
    log_line(format!("show_reminder begin: id={}, title={}", event.id, event.title));
    apply_reminder_window_size(ui);
    ui.set_reminder_title(event.title.clone().into());
    ui.set_reminder_message(event.message.clone().into());
    ui.set_reminder_time(
        format!(
            "{} 触发于 {}",
            event.trigger_text,
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
        )
        .into(),
    );

    if let Err(err) = ui.show() {
        log_line(format!("ui.show failed: {err}"));
        eprintln!("显示提醒窗口失败：{err}");
        return false;
    }
    log_line("ui.show success");

    // show() 后原生 HWND 才一定存在。再次同步尺寸并强制置顶，
    // 避免 Windows/winit 某些情况下只依赖 always-on-top 不稳定。
    apply_reminder_window_size(ui);
    force_window_topmost(ui);
    force_window_foreground(ui);
    ui.invoke_focus_reminder();
    ui.window().request_redraw();
    log_line("show_reminder end: topmost/foreground/focus/redraw requested");
    true
}

// ---------------------------------------------------------------------------
// Windows 原生置顶
// ---------------------------------------------------------------------------

#[cfg(windows)]
fn force_window_topmost(ui: &ReminderWindow) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, HWND_TOPMOST, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW,
    };

    let Some(hwnd) = hwnd_for(ui) else {
        return;
    };

    unsafe {
        if ui.get_fullscreen_reminder() {
            if let Some((w, h)) = primary_screen_size_physical() {
                SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, w as i32, h as i32, SWP_SHOWWINDOW);
                return;
            }
        }

        SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
        );
    }
}

#[cfg(not(windows))]
fn force_window_topmost(_ui: &ReminderWindow) {}

#[cfg(windows)]
fn force_window_foreground(ui: &ReminderWindow) {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{SetActiveWindow, SetFocus};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        BringWindowToTop, SetForegroundWindow, ShowWindow, SW_RESTORE,
    };

    let Some(hwnd) = hwnd_for(ui) else {
        return;
    };

    unsafe {
        ShowWindow(hwnd, SW_RESTORE);
        BringWindowToTop(hwnd);
        SetForegroundWindow(hwnd);
        SetActiveWindow(hwnd);
        SetFocus(hwnd);
    }
}

#[cfg(not(windows))]
fn force_window_foreground(_ui: &ReminderWindow) {}

#[cfg(windows)]
fn hwnd_for(ui: &ReminderWindow) -> Option<windows_sys::Win32::Foundation::HWND> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let handle = ui.window().window_handle();
    let raw = handle.window_handle().ok()?.as_raw();

    match raw {
        RawWindowHandle::Win32(handle) => Some(handle.hwnd.get() as _),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// 屏幕尺寸
// ---------------------------------------------------------------------------

pub fn primary_screen_size_physical() -> Option<(u32, u32)> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN,
        };
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
