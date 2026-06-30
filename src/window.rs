use crate::ReminderWindow;
use slint::{ComponentHandle, PhysicalPosition, PhysicalSize};

/// 延迟隐藏窗口 —— 避免在 key-pressed 回调里直接 hide() 导致 Windows/winit
/// 上需要按两次 Esc 的问题。
pub fn defer_hide(ui: ReminderWindow) {
    let weak = ui.as_weak();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = weak.upgrade() {
            let _ = ui.hide();
        }
    });
}

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
    #[cfg(windows)]
    {
        if let Some(hwnd) = hwnd_for(ui) {
            let dpi = unsafe { windows_sys::Win32::UI::HiDpi::GetDpiForWindow(hwnd) };
            if dpi > 0 {
                return (dpi as f32 / 96.0).max(1.0);
            }
        }
    }

    ui.window().scale_factor().max(1.0)
}

/// 显示一次提醒弹窗。
pub fn show_reminder(
    ui_weak: &slint::Weak<ReminderWindow>,
    event: &crate::scheduler::ReminderEvent,
) {
    if let Some(ui) = ui_weak.upgrade() {
        apply_reminder_window_size(&ui);
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

        let _ = ui.show();

        // show() 后原生 HWND 才一定存在。再次同步尺寸并强制置顶，
        // 避免 Windows/winit 某些情况下只依赖 always-on-top 不稳定。
        apply_reminder_window_size(&ui);
        force_window_topmost(&ui);
        ui.window().request_redraw();
    }
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
