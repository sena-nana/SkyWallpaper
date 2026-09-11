use std::sync::Arc;

use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder, TrayIconEvent};

use crate::state::AppState;

pub struct TrayIds {
    pub settings: String,
    pub pause: String,
    pub quit: String,
}

pub struct Tray {
    _icon: TrayIcon,
    pub ids: TrayIds,
    pause_item: MenuItem,
}

pub fn build(state: &AppState) -> anyhow::Result<Tray> {
    let tx = state.text();
    let menu = Menu::new();
    let settings = MenuItem::new(tx.open_settings, true, None);
    let pause = MenuItem::new(
        if state.is_paused() {
            tx.resume
        } else {
            tx.pause
        },
        true,
        None,
    );
    let quit = MenuItem::new(tx.exit, true, None);
    menu.append(&settings)?;
    menu.append(&pause)?;
    menu.append(&quit)?;
    let icon = tray_icon()?;
    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip(tx.app)
        .with_icon(icon)
        .build()?;
    Ok(Tray {
        ids: TrayIds {
            settings: settings.id().0.clone(),
            pause: pause.id().0.clone(),
            quit: quit.id().0.clone(),
        },
        pause_item: pause,
        _icon: tray,
    })
}

impl Tray {
    pub fn sync_pause(&self, state: &AppState) {
        let tx = state.text();
        self.pause_item.set_text(if state.is_paused() {
            tx.resume
        } else {
            tx.pause
        });
    }
}

pub fn poll_menu() -> Option<String> {
    MenuEvent::receiver().try_recv().ok().map(|e| e.id.0)
}

pub fn poll_tray_click() -> bool {
    matches!(
        TrayIconEvent::receiver().try_recv(),
        Ok(TrayIconEvent::DoubleClick { .. } | TrayIconEvent::Click { .. })
    )
}

fn tray_icon() -> anyhow::Result<Icon> {
    let size = 32u32;
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    let cx = 15.5f32;
    let cy = 15.5f32;
    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            let d = (dx * dx + dy * dy).sqrt();
            let i = ((y * size + x) * 4) as usize;
            if d < 10.0 {
                rgba[i] = 255;
                rgba[i + 1] = 196;
                rgba[i + 2] = 72;
                rgba[i + 3] = 255;
            } else if d < 14.0 && ((x + y) % 4 == 0) {
                rgba[i] = 255;
                rgba[i + 1] = 168;
                rgba[i + 2] = 40;
                rgba[i + 3] = 220;
            }
        }
    }
    Ok(Icon::from_rgba(rgba, size, size)?)
}

pub fn open_settings(state: Arc<AppState>) {
    if state
        .settings_open
        .compare_exchange(
            false,
            true,
            std::sync::atomic::Ordering::SeqCst,
            std::sync::atomic::Ordering::SeqCst,
        )
        .is_err()
    {
        return;
    }
    std::thread::Builder::new()
        .name("skywallpaper-settings".into())
        .spawn(move || {
            if let Err(err) = crate::settings::run(state.clone()) {
                state.set_status(err.to_string());
            }
            state
                .settings_open
                .store(false, std::sync::atomic::Ordering::SeqCst);
        })
        .ok();
}
