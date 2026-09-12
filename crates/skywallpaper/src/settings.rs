use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use nana_ui::runtime::{
    Activate, AppTitleBar, Button, Checkbox, Entity, FrameworkError, Text, TextChanged, TextInput,
    ToggleChanged,
};
use nana_ui::{
    ApplicationState, ApplicationWindow, ButtonKind, RuntimeApplication, RuntimeProgramContext,
    RuntimeProgramUpdate, RuntimeWindowSettings, run_runtime,
};
use nana_ui_platform::WindowId;
use weather::{lookup_ip, search_places};

use crate::autostart;
use crate::config::{LanguagePref, LocationMode};
use crate::state::AppState;

static SETTINGS_STATE: Mutex<Option<Arc<AppState>>> = Mutex::new(None);

#[derive(Clone)]
enum Message {
    Query(String),
    Search,
    UseIp,
    Lang(LanguagePref),
    Autostart(bool),
    PauseFullscreen(bool),
    TogglePause,
    RefreshWeather,
}

#[derive(Clone, Copy)]
struct Widgets {
    title_bar: Entity<AppTitleBar>,
    location_label: Entity<Text>,
    query: Entity<TextInput>,
    search: Entity<Button>,
    ip: Entity<Button>,
    place: Entity<Text>,
    lang_label: Entity<Text>,
    lang_sys: Entity<Button>,
    lang_zh: Entity<Button>,
    lang_en: Entity<Button>,
    autostart: Entity<Checkbox>,
    fullscreen: Entity<Checkbox>,
    pause: Entity<Button>,
    refresh: Entity<Button>,
    status: Entity<Text>,
    about: Entity<Text>,
}

struct Settings {
    state: Arc<AppState>,
    query: String,
    widgets: Option<Widgets>,
}

pub fn run(state: Arc<AppState>) -> anyhow::Result<()> {
    *SETTINGS_STATE.lock().unwrap() = Some(state.clone());
    let title = state.text().settings;
    let result = run_runtime::<RuntimeApplication<Settings>>(
        RuntimeWindowSettings::new(title)
            .initial_size(480.0, 640.0)
            .minimum_size(360.0, 400.0),
    );
    *SETTINGS_STATE.lock().unwrap() = None;
    result.map_err(|err| anyhow::anyhow!("{err}"))
}

impl ApplicationState for Settings {
    type Message = Message;
    type Error = FrameworkError;

    fn initialize(_: &RuntimeProgramContext<Self::Message>) -> Result<Self, Self::Error> {
        let state = SETTINGS_STATE
            .lock()
            .unwrap()
            .clone()
            .expect("settings state");
        Ok(Self {
            query: String::new(),
            state,
            widgets: None,
        })
    }

    fn build(
        &mut self,
        window: &mut ApplicationWindow,
        _: &RuntimeProgramContext<Self::Message>,
    ) -> Result<(), Self::Error> {
        let tx = self.state.text();
        let cfg = self.state.config.lock().unwrap().clone();
        let paused = self.state.is_paused();
        let status = self.state.status.lock().unwrap().clone();
        self.widgets = Some(crate::shell::mount_app_shell(
            window,
            tx.settings,
            |ui, title_bar| {
                let location_label = ui.child("loc_l", Text::new(tx.location));
                let query = ui.child(
                    "query",
                    TextInput::new("")
                        .placeholder(tx.location)
                        .label(tx.location),
                );
                let (search, ip) = ui.row(8.0, |ui| {
                    let search =
                        ui.child("search", Button::new(tx.search).kind(ButtonKind::Primary));
                    let ip = ui.child("ip", Button::new(tx.use_ip));
                    (search, ip)
                });
                let place = ui.child(
                    "place",
                    Text::new(format!(
                        "{}  ({:.2}, {:.2})",
                        cfg.label, cfg.latitude, cfg.longitude
                    )),
                );
                let lang_label = ui.child("lang_l", Text::new(tx.language));
                let (lang_sys, lang_zh, lang_en) = ui.row(8.0, |ui| {
                    let lang_sys = ui.child("lang_sys", Button::new(tx.follow_system));
                    let lang_zh = ui.child("lang_zh", Button::new(tx.chinese));
                    let lang_en = ui.child("lang_en", Button::new(tx.english));
                    (lang_sys, lang_zh, lang_en)
                });
                let autostart = ui.child("auto", Checkbox::new(tx.autostart, cfg.autostart));
                let fullscreen = ui.child(
                    "fs",
                    Checkbox::new(tx.pause_fullscreen, cfg.pause_on_fullscreen),
                );
                let pause = ui.child(
                    "pause",
                    Button::new(if paused { tx.resume } else { tx.pause }),
                );
                let refresh = ui.child("refresh", Button::new(tx.refresh_weather));
                let status = ui.child(
                    "status",
                    Text::new(if status.is_empty() {
                        tx.running.to_string()
                    } else {
                        status
                    }),
                );
                let about = ui.child("about", Text::new(tx.about));

                ui.on(query, move |_, event: &TextChanged, cx| {
                    cx.dispatch_program(Message::Query(event.value.clone()));
                });
                ui.on(search, move |_, _: &Activate, cx| {
                    cx.dispatch_program(Message::Search);
                });
                ui.on(ip, move |_, _: &Activate, cx| {
                    cx.dispatch_program(Message::UseIp);
                });
                ui.on(lang_sys, move |_, _: &Activate, cx| {
                    cx.dispatch_program(Message::Lang(LanguagePref::System));
                });
                ui.on(lang_zh, move |_, _: &Activate, cx| {
                    cx.dispatch_program(Message::Lang(LanguagePref::Zh));
                });
                ui.on(lang_en, move |_, _: &Activate, cx| {
                    cx.dispatch_program(Message::Lang(LanguagePref::En));
                });
                ui.on(autostart, move |_, event: &ToggleChanged, cx| {
                    cx.dispatch_program(Message::Autostart(event.checked));
                });
                ui.on(fullscreen, move |_, event: &ToggleChanged, cx| {
                    cx.dispatch_program(Message::PauseFullscreen(event.checked));
                });
                ui.on(pause, move |_, _: &Activate, cx| {
                    cx.dispatch_program(Message::TogglePause);
                });
                ui.on(refresh, move |_, _: &Activate, cx| {
                    cx.dispatch_program(Message::RefreshWeather);
                });

                Widgets {
                    title_bar,
                    location_label,
                    query,
                    search,
                    ip,
                    place,
                    lang_label,
                    lang_sys,
                    lang_zh,
                    lang_en,
                    autostart,
                    fullscreen,
                    pause,
                    refresh,
                    status,
                    about,
                }
            },
        )?);
        Ok(())
    }

    fn update(
        &mut self,
        message: Message,
        windows: &mut HashMap<WindowId, ApplicationWindow>,
        context: &RuntimeProgramContext<Self::Message>,
    ) -> RuntimeProgramUpdate {
        match message {
            Message::Query(value) => self.query = value,
            Message::Search => {
                let lang = self.state.lang().code();
                match search_places(&self.query, lang) {
                    Ok(hits) if !hits.is_empty() => {
                        let hit = &hits[0];
                        self.state.update_location(
                            hit.latitude,
                            hit.longitude,
                            hit.label.clone(),
                            LocationMode::Manual,
                        );
                        self.state.set_status(hit.label.clone());
                    }
                    Ok(_) | Err(_) => self.state.set_status("…"),
                }
            }
            Message::UseIp => match lookup_ip() {
                Ok(place) => {
                    self.state.update_location(
                        place.latitude,
                        place.longitude,
                        place.label.clone(),
                        LocationMode::Ip,
                    );
                    self.state.set_status(place.label);
                }
                Err(err) => self.state.set_status(err.to_string()),
            },
            Message::Lang(pref) => self.state.set_language(pref),
            Message::Autostart(on) => {
                let mut cfg = self.state.config.lock().unwrap();
                cfg.autostart = on;
                cfg.save();
                drop(cfg);
                let _ = autostart::set_enabled(on);
            }
            Message::PauseFullscreen(on) => {
                let mut cfg = self.state.config.lock().unwrap();
                cfg.pause_on_fullscreen = on;
                cfg.save();
            }
            Message::TogglePause => self.state.toggle_pause(),
            Message::RefreshWeather => self
                .state
                .refresh_weather
                .store(true, std::sync::atomic::Ordering::Relaxed),
        }
        if let Some(window) = windows.get_mut(&context.window_id()) {
            self.sync_widgets(window);
        }
        RuntimeProgramUpdate::redraw(context.window_id())
    }
}

impl Settings {
    fn sync_widgets(&mut self, window: &mut ApplicationWindow) {
        let Some(w) = self.widgets else {
            return;
        };
        let tx = self.state.text();
        let cfg = self.state.config.lock().unwrap().clone();
        let paused = self.state.is_paused();
        let status = self.state.status.lock().unwrap().clone();
        let cx = window.document.context_mut();
        let _ = cx.update_component(w.title_bar, |bar, _| {
            bar.title = tx.settings.into();
        });
        let _ = cx.assemble_app_title_bar(w.title_bar);
        let _ = cx.update_component(w.location_label, |t, _| t.value = tx.location.to_string());
        let _ = cx.update_component(w.query, |input, _| {
            input.placeholder = tx.location.into();
        });
        let _ = cx.update_component(w.search, |b, _| b.label = tx.search.to_string());
        let _ = cx.update_component(w.ip, |b, _| b.label = tx.use_ip.to_string());
        let _ = cx.update_component(w.place, |t, _| {
            t.value = format!("{}  ({:.2}, {:.2})", cfg.label, cfg.latitude, cfg.longitude);
        });
        let _ = cx.update_component(w.lang_label, |t, _| t.value = tx.language.to_string());
        let _ = cx.update_component(w.lang_sys, |b, _| b.label = tx.follow_system.to_string());
        let _ = cx.update_component(w.lang_zh, |b, _| b.label = tx.chinese.to_string());
        let _ = cx.update_component(w.lang_en, |b, _| b.label = tx.english.to_string());
        let _ = cx.update_component(w.autostart, |c, _| {
            c.label = tx.autostart.to_string();
            c.checked = cfg.autostart;
        });
        let _ = cx.update_component(w.fullscreen, |c, _| {
            c.label = tx.pause_fullscreen.to_string();
            c.checked = cfg.pause_on_fullscreen;
        });
        let _ = cx.update_component(w.pause, |b, _| {
            b.label = if paused {
                tx.resume.to_string()
            } else {
                tx.pause.to_string()
            };
        });
        let _ = cx.update_component(w.refresh, |b, _| b.label = tx.refresh_weather.to_string());
        let _ = cx.update_component(w.status, |t, _| {
            let run = if paused { tx.paused } else { tx.running };
            t.value = if status.is_empty() {
                format!("{} · {}", tx.weather, run)
            } else {
                format!("{} · {} · {}", tx.weather, run, status)
            };
        });
        let _ = cx.update_component(w.about, |t, _| t.value = tx.about.to_string());
    }
}
