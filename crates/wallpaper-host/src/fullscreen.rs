use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GWL_EXSTYLE, GetClassNameW, GetForegroundWindow, GetWindowLongPtrW, GetWindowRect,
    IsIconic, IsWindowVisible, WINDOW_EX_STYLE, WS_EX_TOOLWINDOW,
};

pub fn foreground_is_fullscreen() -> bool {
    unsafe {
        let hwnd = GetForegroundWindow();
        if skip_window(hwnd) {
            return false;
        }
        let mut wr = RECT::default();
        if GetWindowRect(hwnd, &mut wr).is_err() {
            return false;
        }
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut mi).as_bool() {
            return false;
        }
        covers(wr, mi.rcMonitor)
    }
}

/// True when a single visible top-level window covers that work area.
pub fn work_areas_occluded(areas: &[RECT]) -> Vec<bool> {
    if areas.is_empty() {
        return Vec::new();
    }
    let mut hidden = vec![false; areas.len()];
    let mut ctx = OccCtx {
        areas,
        hidden: &mut hidden,
    };
    unsafe {
        let _ = EnumWindows(Some(enum_cover), LPARAM(&mut ctx as *mut OccCtx as isize));
    }
    hidden
}

struct OccCtx<'a> {
    areas: &'a [RECT],
    hidden: &'a mut [bool],
}

unsafe extern "system" fn enum_cover(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
    unsafe {
        let ctx = &mut *(lparam.0 as *mut OccCtx);
        if skip_window(hwnd) {
            return windows::core::BOOL(1);
        }
        let mut wr = RECT::default();
        if GetWindowRect(hwnd, &mut wr).is_err() {
            return windows::core::BOOL(1);
        }
        for (i, area) in ctx.areas.iter().enumerate() {
            if !ctx.hidden[i] && covers(wr, *area) {
                ctx.hidden[i] = true;
            }
        }
        if ctx.hidden.iter().all(|h| *h) {
            return windows::core::BOOL(0);
        }
        windows::core::BOOL(1)
    }
}

fn skip_window(hwnd: HWND) -> bool {
    unsafe {
        if hwnd.is_invalid() {
            return true;
        }
        if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
            return true;
        }
        if is_cloaked(hwnd) {
            return true;
        }
        let ex = WINDOW_EX_STYLE(GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32);
        if ex.contains(WS_EX_TOOLWINDOW) {
            return true;
        }
        let mut class = [0u16; 64];
        let n = GetClassNameW(hwnd, &mut class);
        let class = String::from_utf16_lossy(&class[..n as usize]);
        is_desktop_class(&class)
    }
}

fn is_desktop_class(class: &str) -> bool {
    matches!(
        class,
        "Progman"
            | "WorkerW"
            | "Shell_TrayWnd"
            | "Shell_SecondaryTrayWnd"
            | "SkyWallpaperSurface"
            | "NotifyIconOverflowWindow"
    )
}

fn is_cloaked(hwnd: HWND) -> bool {
    let mut cloaked: u32 = 0;
    unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            &mut cloaked as *mut u32 as *mut core::ffi::c_void,
            std::mem::size_of::<u32>() as u32,
        )
        .is_ok()
            && cloaked != 0
    }
}

fn covers(window: RECT, monitor: RECT) -> bool {
    let slop = 4;
    window.left <= monitor.left + slop
        && window.top <= monitor.top + slop
        && window.right >= monitor.right - slop
        && window.bottom >= monitor.bottom - slop
}
