use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use nana_ui::runtime::{
    Activate, AppTitleBar, Button, Checkbox, Entity, FrameworkError, LengthSpec, ScrollAxes,
    ScrollView, SettingsCard, SettingsRow, Stack, Text, TextChanged, TextInput, ToggleChanged,
    view::{button, entity_ref, row, text, text_input, widget},
};
use nana_ui::{
    ApplicationState, ApplicationWindow, ButtonKind, RuntimeApplication, RuntimeProgramContext,
    RuntimeProgramUpdate, WindowDescriptor, run_runtime,
};
use nana_ui_core::DisplaySpec;
use nana_ui_platform::WindowId;
use weather::{GeoPlace, lookup_ip, search_places};

use crate::autostart;
use crate::config::{LanguagePref, LocationMode};
use crate::state::AppState;

static SETTINGS_STATE: Mutex<Option<Arc<AppState>>> = Mutex::new(None);

#[derive(Clone)]
enum Message {
    Select(Page),
    Query(String),
    Search,
    SearchFinished {
        request_id: u64,
        result: Result<Vec<GeoPlace>, String>,
    },
    UseIp,
    IpFinished {
        request_id: u64,
        result: Result<GeoPlace, String>,
    },
    Lang(LanguagePref),
    Autostart(bool),
    PauseFullscreen(bool),
    TogglePause,
    RefreshWeather,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    General,
    Behavior,
    About,
}

#[derive(Clone, Copy)]
struct Widgets {
    title_bar: Entity<AppTitleBar>,
    nav_general: Entity<Button>,
    nav_behavior: Entity<Button>,
    nav_about: Entity<Button>,
    general_page: Entity<Stack>,
    behavior_page: Entity<Stack>,
    about_page: Entity<Stack>,
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
    next_request_id: u64,
    active_request_id: Option<u64>,
    widgets: Option<Widgets>,
    page: Page,
}

pub fn run(state: Arc<AppState>) -> anyhow::Result<()> {
    *SETTINGS_STATE.lock().unwrap() = Some(state.clone());
    let title = state.text().settings;
    let result = run_runtime::<RuntimeApplication<Settings>>(
        WindowDescriptor::new(title)
            .initial_size(640.0, 720.0)
            .minimum_size(480.0, 520.0),
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
            next_request_id: 0,
            active_request_id: None,
            state,
            widgets: None,
            page: Page::General,
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
        let (title_bar, widgets) = crate::shell::mount_app_shell(window, tx.settings, || {
            let nav_general = entity_ref::<Button>();
            let nav_behavior = entity_ref::<Button>();
            let nav_about = entity_ref::<Button>();
            let general_page = entity_ref::<Stack>();
            let behavior_page = entity_ref::<Stack>();
            let about_page = entity_ref::<Stack>();
            let query = entity_ref::<TextInput>();
            let search = entity_ref::<Button>();
            let ip = entity_ref::<Button>();
            let place = entity_ref::<Text>();
            let lang_label = entity_ref::<Text>();
            let lang_sys = entity_ref::<Button>();
            let lang_zh = entity_ref::<Button>();
            let lang_en = entity_ref::<Button>();
            let autostart = entity_ref::<Checkbox>();
            let fullscreen = entity_ref::<Checkbox>();
            let pause = entity_ref::<Button>();
            let refresh = entity_ref::<Button>();
            let status_ref = entity_ref::<Text>();
            let about = entity_ref::<Text>();
            let location_controls = widget(SettingsCard::new(tx.location)).children(
                widget(Stack::column(10.0)).children((
                    text(tx.location_hint),
                    text_input()
                        .placeholder(tx.location)
                        .label(tx.location)
                        .entity_ref(query)
                        .on_cx(|_, event: &TextChanged, cx| {
                            cx.dispatch_program(Message::Query(event.value.to_string()));
                        }),
                    row().gap(8.0).children((
                        widget(Button::new(tx.search).kind(ButtonKind::Primary))
                            .entity_ref(search)
                            .on_cx(|_, _: &Activate, cx| cx.dispatch_program(Message::Search)),
                        button(tx.use_ip)
                            .entity_ref(ip)
                            .on_cx(|_, _: &Activate, cx| cx.dispatch_program(Message::UseIp)),
                    )),
                    text(format!(
                        "{}  ({:.2}, {:.2})",
                        cfg.label, cfg.latitude, cfg.longitude
                    ))
                    .entity_ref(place),
                )),
            );
            let language_controls = widget(SettingsCard::new(tx.language)).children(
                widget(Stack::column(10.0)).children((
                    text(tx.language_hint).entity_ref(lang_label),
                    widget(SettingsRow::new(tx.language)).children(
                        row().gap(8.0).children((
                            button(tx.follow_system).entity_ref(lang_sys).on_cx(
                                |_, _: &Activate, cx| {
                                    cx.dispatch_program(Message::Lang(LanguagePref::System))
                                },
                            ),
                            button(tx.chinese)
                                .entity_ref(lang_zh)
                                .on_cx(|_, _: &Activate, cx| {
                                    cx.dispatch_program(Message::Lang(LanguagePref::Zh))
                                }),
                            button(tx.english)
                                .entity_ref(lang_en)
                                .on_cx(|_, _: &Activate, cx| {
                                    cx.dispatch_program(Message::Lang(LanguagePref::En))
                                }),
                        )),
                    ),
                )),
            );
            let behavior_controls = widget(SettingsCard::new(tx.behavior)).children(
                widget(Stack::column(8.0)).children((
                    widget(SettingsRow::new(tx.autostart).hint(tx.runtime_hint)).children(
                        widget(Checkbox::new(tx.autostart, cfg.autostart))
                            .entity_ref(autostart)
                            .on_cx(|_, event: &ToggleChanged, cx| {
                                cx.dispatch_program(Message::Autostart(event.checked))
                            }),
                    ),
                    widget(SettingsRow::new(tx.pause_fullscreen)).children(
                        widget(Checkbox::new(tx.pause_fullscreen, cfg.pause_on_fullscreen))
                            .entity_ref(fullscreen)
                            .on_cx(|_, event: &ToggleChanged, cx| {
                                cx.dispatch_program(Message::PauseFullscreen(event.checked))
                            }),
                    ),
                    row().gap(8.0).children((
                        button(if paused { tx.resume } else { tx.pause })
                            .entity_ref(pause)
                            .on_cx(|_, _: &Activate, cx| cx.dispatch_program(Message::TogglePause)),
                        button(tx.refresh_weather).entity_ref(refresh).on_cx(
                            |_, _: &Activate, cx| cx.dispatch_program(Message::RefreshWeather),
                        ),
                    )),
                    text(if status.is_empty() {
                        tx.running.to_string()
                    } else {
                        status
                    })
                    .entity_ref(status_ref),
                )),
            );
            let about_controls = widget(SettingsCard::new(tx.about_page)).children(
                widget(Stack::column(8.0))
                    .children((text(tx.about).entity_ref(about), text(tx.runtime_hint))),
            );
            let body = widget(Stack::fill_row(16.0)).children((
                widget(
                    Stack::fill_column(8.0)
                        .width(LengthSpec::Px(160.0))
                        .shrink(0.0)
                        .padding(8.0),
                )
                .children((
                    text(tx.settings),
                    widget(Button::new(tx.general).kind(ButtonKind::Primary))
                        .entity_ref(nav_general)
                        .on_cx(|_, _: &Activate, cx| {
                            cx.dispatch_program(Message::Select(Page::General))
                        }),
                    button(tx.behavior)
                        .entity_ref(nav_behavior)
                        .on_cx(|_, _: &Activate, cx| {
                            cx.dispatch_program(Message::Select(Page::Behavior))
                        }),
                    button(tx.about_page)
                        .entity_ref(nav_about)
                        .on_cx(|_, _: &Activate, cx| {
                            cx.dispatch_program(Message::Select(Page::About))
                        }),
                )),
                widget(ScrollView::new(ScrollAxes::Vertical).with_layout(|layout| {
                    layout.flex_grow = Some(1.0);
                    layout.flex_shrink = Some(1.0);
                }))
                .children(
                    widget(Stack::fill_column(16.0)).children((
                        widget(Stack::column(16.0))
                            .entity_ref(general_page)
                            .children((location_controls, language_controls)),
                        widget(Stack::column(16.0).with_layout(|layout| {
                            layout.display = Some(DisplaySpec::None);
                        }))
                        .entity_ref(behavior_page)
                        .children(behavior_controls),
                        widget(Stack::column(16.0).with_layout(|layout| {
                            layout.display = Some(DisplaySpec::None);
                        }))
                        .entity_ref(about_page)
                        .children(about_controls),
                    )),
                ),
            ));
            (
                body,
                (
                    (
                        nav_general,
                        nav_behavior,
                        nav_about,
                        general_page,
                        behavior_page,
                        about_page,
                        query,
                    ),
                    (search, ip, place, lang_label, lang_sys, lang_zh, lang_en),
                    (autostart, fullscreen, pause, refresh, status_ref, about),
                ),
            )
        })?;
        self.widgets = Some(Widgets {
            title_bar,
            nav_general: widgets.0.0,
            nav_behavior: widgets.0.1,
            nav_about: widgets.0.2,
            general_page: widgets.0.3,
            behavior_page: widgets.0.4,
            about_page: widgets.0.5,
            query: widgets.0.6,
            search: widgets.1.0,
            ip: widgets.1.1,
            place: widgets.1.2,
            lang_label: widgets.1.3,
            lang_sys: widgets.1.4,
            lang_zh: widgets.1.5,
            lang_en: widgets.1.6,
            autostart: widgets.2.0,
            fullscreen: widgets.2.1,
            pause: widgets.2.2,
            refresh: widgets.2.3,
            status: widgets.2.4,
            about: widgets.2.5,
        });
        Ok(())
    }

    fn update(
        &mut self,
        message: Message,
        windows: &mut HashMap<WindowId, ApplicationWindow>,
        context: &RuntimeProgramContext<Self::Message>,
    ) -> RuntimeProgramUpdate {
        match message {
            Message::Select(page) => self.page = page,
            Message::Query(value) => self.query = value,
            Message::Search => {
                let request_id = self.start_request();
                let lang = self.state.lang().code();
                let query = self.query.clone();
                let context = context.clone();
                std::thread::spawn(move || {
                    let result = search_places(&query, lang).map_err(|err| err.to_string());
                    context.dispatch(Message::SearchFinished { request_id, result });
                });
            }
            Message::SearchFinished { request_id, result } => {
                if !self.is_current_request(request_id) {
                    return RuntimeProgramUpdate::default();
                }
                match result {
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
            Message::UseIp => {
                let request_id = self.start_request();
                let context = context.clone();
                std::thread::spawn(move || {
                    let result = lookup_ip().map_err(|err| err.to_string());
                    context.dispatch(Message::IpFinished { request_id, result });
                });
            }
            Message::IpFinished { request_id, result } => {
                if !self.is_current_request(request_id) {
                    return RuntimeProgramUpdate::default();
                }
                match result {
                    Ok(place) => {
                        self.state.update_location(
                            place.latitude,
                            place.longitude,
                            place.label.clone(),
                            LocationMode::Ip,
                        );
                        self.state.set_status(place.label);
                    }
                    Err(err) => self.state.set_status(err),
                }
            }
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
            self.sync_pages(window);
        }
        RuntimeProgramUpdate::redraw(context.window_id())
    }
}

impl Settings {
    fn start_request(&mut self) -> u64 {
        self.next_request_id = self.next_request_id.wrapping_add(1);
        self.active_request_id = Some(self.next_request_id);
        self.state.set_status("…");
        self.next_request_id
    }

    fn is_current_request(&self, request_id: u64) -> bool {
        request_is_current(self.active_request_id, request_id)
    }

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
        let _ = cx.update_component(w.nav_general, |b, _| {
            b.label = tx.general.to_string();
            b.kind = if self.page == Page::General {
                ButtonKind::Primary
            } else {
                ButtonKind::Ghost
            };
        });
        let _ = cx.update_component(w.nav_behavior, |b, _| {
            b.label = tx.behavior.to_string();
            b.kind = if self.page == Page::Behavior {
                ButtonKind::Primary
            } else {
                ButtonKind::Ghost
            };
        });
        let _ = cx.update_component(w.nav_about, |b, _| {
            b.label = tx.about_page.to_string();
            b.kind = if self.page == Page::About {
                ButtonKind::Primary
            } else {
                ButtonKind::Ghost
            };
        });
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

    fn sync_pages(&self, window: &mut ApplicationWindow) {
        let Some(w) = self.widgets else {
            return;
        };
        let cx = window.document.context_mut();
        for (page, visible) in [
            (w.general_page, self.page == Page::General),
            (w.behavior_page, self.page == Page::Behavior),
            (w.about_page, self.page == Page::About),
        ] {
            let _ = cx.update_component(page, |stack, _| {
                *stack = stack.clone().with_layout(|layout| {
                    layout.display = (!visible).then_some(DisplaySpec::None);
                });
            });
        }
    }
}

fn request_is_current(active_request_id: Option<u64>, request_id: u64) -> bool {
    active_request_id == Some(request_id)
}

#[cfg(test)]
mod tests {
    use super::request_is_current;

    #[test]
    fn only_active_request_can_apply_result() {
        assert!(request_is_current(Some(7), 7));
        assert!(!request_is_current(Some(8), 7));
        assert!(!request_is_current(None, 7));
    }
}
