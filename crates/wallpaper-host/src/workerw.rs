use std::sync::atomic::{AtomicBool, Ordering};

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, FindWindowExW, FindWindowW, GetWindowRect,
    RegisterClassW, SendMessageTimeoutW, SetWindowPos, CS_HREDRAW, CS_VREDRAW, HWND_BOTTOM,
    SMTO_NORMAL, SWP_NOACTIVATE, SWP_NOZORDER, WM_DISPLAYCHANGE, WM_DPICHANGED, WNDCLASSW, WS_CHILD,
    WS_CLIPSIBLINGS, WS_EX_NOACTIVATE, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW,
    WS_EX_TRANSPARENT, WS_VISIBLE,
};
use windows::core::w;

pub static DISPLAY_CHANGED: AtomicBool = AtomicBool::new(false);

#[derive(Debug, thiserror::Error)]
pub enum WorkerWError {
    #[error("Progman not found")]
    NoProgman,
    #[error("WorkerW not found")]
    NoWorkerW,
    #[error("create window failed")]
    CreateWindow,
    #[error("{0}")]
    Windows(#[from] windows::core::Error),
}

pub struct MonitorRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

fn hwnd_ok(result: windows::core::Result<HWND>) -> Option<HWND> {
    result.ok().filter(|hwnd| !hwnd.is_invalid())
}

pub fn spawn_worker_w() -> Result<HWND, WorkerWError> {
    unsafe {
        let progman = hwnd_ok(FindWindowW(w!("Progman"), None)).ok_or(WorkerWError::NoProgman)?;
        let mut result = 0usize;
        let _ = SendMessageTimeoutW(
            progman,
            0x052C,
            WPARAM(0),
            LPARAM(0),
            SMTO_NORMAL,
            1000,
            Some(&mut result),
        );
        let _ = SendMessageTimeoutW(
            progman,
            0x052C,
            WPARAM(0xD),
            LPARAM(0x1),
            SMTO_NORMAL,
            1000,
            Some(&mut result),
        );

        let mut found = HWND::default();
        let _ = windows::Win32::UI::WindowsAndMessaging::EnumWindows(
            Some(enum_worker_w),
            LPARAM(&mut found as *mut HWND as isize),
        );
        if found.is_invalid() {
            found = hwnd_ok(FindWindowExW(Some(progman), None, w!("WorkerW"), None))
                .unwrap_or_default();
        }
        if found.is_invalid() {
            return Err(WorkerWError::NoWorkerW);
        }
        Ok(found)
    }
}

unsafe extern "system" fn enum_worker_w(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
    unsafe {
        if hwnd_ok(FindWindowExW(Some(hwnd), None, w!("SHELLDLL_DefView"), None)).is_some() {
            if let Some(next) = hwnd_ok(FindWindowExW(None, Some(hwnd), w!("WorkerW"), None)) {
                let slot = lparam.0 as *mut HWND;
                *slot = next;
            }
        }
        windows::core::BOOL(1)
    }
}

unsafe extern "system" fn surface_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_DISPLAYCHANGE || msg == WM_DPICHANGED {
        DISPLAY_CHANGED.store(true, Ordering::SeqCst);
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

pub fn register_surface_class() -> Result<(), WorkerWError> {
    unsafe {
        let hinstance = GetModuleHandleW(None)?;
        let class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(surface_wndproc),
            hInstance: hinstance.into(),
            lpszClassName: w!("SkyWallpaperSurface"),
            ..Default::default()
        };
        RegisterClassW(&class);
        Ok(())
    }
}

pub fn create_monitor_window(parent: HWND, rect: &MonitorRect) -> Result<HWND, WorkerWError> {
    unsafe {
        let hinstance = GetModuleHandleW(None)?;
        let hwnd = CreateWindowExW(
            WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TRANSPARENT | WS_EX_NOREDIRECTIONBITMAP,
            w!("SkyWallpaperSurface"),
            w!("SkyWallpaper"),
            WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS,
            rect.x,
            rect.y,
            rect.width as i32,
            rect.height as i32,
            Some(parent),
            None,
            Some(hinstance.into()),
            None,
        )?;
        if hwnd.is_invalid() {
            return Err(WorkerWError::CreateWindow);
        }
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_BOTTOM),
            rect.x,
            rect.y,
            rect.width as i32,
            rect.height as i32,
            SWP_NOACTIVATE | SWP_NOZORDER,
        );
        Ok(hwnd)
    }
}

pub fn destroy_hwnd(hwnd: HWND) {
    unsafe {
        let _ = DestroyWindow(hwnd);
    }
}

pub fn list_monitors_relative_to(parent: HWND) -> Vec<MonitorRect> {
    let mut parent_rect = RECT::default();
    unsafe {
        let _ = GetWindowRect(parent, &mut parent_rect);
    }
    let mut monitors = Vec::new();
    let mut ctx = EnumCtx {
        parent: parent_rect,
        out: &mut monitors,
    };
    unsafe {
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(monitor_enum),
            LPARAM(&mut ctx as *mut EnumCtx as isize),
        );
    }
    if monitors.is_empty() {
        monitors.push(MonitorRect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        });
    }
    monitors
}

struct EnumCtx<'a> {
    parent: RECT,
    out: &'a mut Vec<MonitorRect>,
}

unsafe extern "system" fn monitor_enum(
    monitor: HMONITOR,
    _hdc: HDC,
    _lprc: *mut RECT,
    lparam: LPARAM,
) -> windows::core::BOOL {
    unsafe {
        let ctx = &mut *(lparam.0 as *mut EnumCtx);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(monitor, &mut info).as_bool() {
            let r = info.rcMonitor;
            ctx.out.push(MonitorRect {
                x: r.left - ctx.parent.left,
                y: r.top - ctx.parent.top,
                width: (r.right - r.left).max(1) as u32,
                height: (r.bottom - r.top).max(1) as u32,
            });
        }
        windows::core::BOOL(1)
    }
}

pub fn take_display_changed() -> bool {
    DISPLAY_CHANGED.swap(false, Ordering::SeqCst)
}
