//! Session-only `--debug` knobs for the sky preview.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Duration, Local, NaiveDateTime, TimeZone, Timelike, Utc};
use nana_ui::runtime::{
    Activate, Button, Checkbox, Entity, FrameworkError, RangeChanged, RangeField, Text,
    ToggleChanged,
};
use nana_ui::{
    ApplicationState, ApplicationWindow, ButtonKind, RuntimeApplication, RuntimeProgramContext,
    RuntimeProgramUpdate, RuntimeWindowSettings, run_runtime,
};
use nana_ui_platform::WindowId;
use serde::{Deserialize, Serialize};
use sky_core::{PrecipKind, SkyView, SkyWeather, WeatherCode, date_for_season, season_from_utc};

use crate::config::LanguagePref;
use crate::i18n::Lang;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct DebugParams {
    pub latitude: f64,
    pub longitude: f64,
    pub live_clock: bool,
    pub hour: f32,
    #[serde(default)]
    pub season: f32,
    pub anim_paused: bool,
    pub cloud_cover: f32,
    pub precip: f32,
    pub snow: bool,
    pub fog: f32,
    pub thunder: f32,
}

impl DebugParams {
    pub fn from_live(latitude: f64, longitude: f64, weather: SkyWeather) -> Self {
        Self {
            latitude,
            longitude,
            live_clock: true,
            hour: local_hour(Local::now()),
            season: season_from_utc(Utc::now(), latitude),
            anim_paused: false,
            cloud_cover: weather.cloud_cover,
            precip: weather.precip,
            snow: weather.precip_kind == PrecipKind::Snow,
            fog: weather.fog,
            thunder: if weather.thunder { 0.7 } else { 0.0 },
        }
    }

    pub fn apply_preset(&mut self, name: &str) {
        let w = weather_preset(name);
        self.cloud_cover = w.cloud_cover;
        self.precip = w.precip;
        self.snow = w.precip_kind == PrecipKind::Snow;
        self.fog = w.fog;
        self.thunder = if w.thunder { 0.7 } else { 0.0 };
    }

    pub fn build_view(&self) -> SkyView {
        let utc = if self.live_clock {
            Utc::now()
        } else {
            utc_for_local(self.hour, self.season, self.latitude)
        };
        let weather = SkyWeather {
            code: WeatherCode(0),
            cloud_cover: self.cloud_cover.clamp(0.0, 1.0),
            precip: self.precip.clamp(0.0, 1.0),
            precip_kind: if self.snow {
                PrecipKind::Snow
            } else {
                PrecipKind::Rain
            },
            fog: self.fog.clamp(0.0, 1.0),
            thunder: self.thunder > 0.05,
        };
        SkyView::at(self.latitude, self.longitude, utc, weather)
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let text = toml::to_string(self)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, text)?;
        fs::rename(tmp, path)
    }

    pub fn load(path: &Path) -> Option<Self> {
        toml::from_str(&fs::read_to_string(path).ok()?).ok()
    }

    fn summary(&self) -> String {
        format!(
            "hour={:.1}  season={:.2}  cloud={:.0}%  precip={:.0}%",
            self.hour,
            self.season,
            self.cloud_cover * 100.0,
            self.precip * 100.0
        )
    }

    fn knob(&self, k: Knob) -> f32 {
        match k {
            Knob::Hour => self.hour,
            Knob::Season => self.season,
            Knob::Cloud => self.cloud_cover,
            Knob::Precip => self.precip,
            Knob::Fog => self.fog,
            Knob::Thunder => self.thunder,
        }
    }

    fn set_knob(&mut self, k: Knob, value: f32) {
        match k {
            Knob::Hour => {
                self.live_clock = false;
                self.hour = value;
            }
            Knob::Season => {
                self.live_clock = false;
                self.season = value.rem_euclid(1.0);
            }
            Knob::Cloud => self.cloud_cover = value,
            Knob::Precip => self.precip = value,
            Knob::Fog => self.fog = value,
            Knob::Thunder => self.thunder = value,
        }
    }
}

pub fn weather_preset(name: &str) -> SkyWeather {
    match name {
        "clear" => SkyWeather::clear_fallback(),
        "cloud" | "cloudy" => SkyWeather::from_wmo(3, 85.0, 0.0, 16_000.0),
        "rain" => SkyWeather::from_wmo(63, 95.0, 4.0, 8_000.0),
        "snow" => SkyWeather::from_wmo(73, 95.0, 2.0, 6_000.0),
        "fog" => SkyWeather::from_wmo(45, 80.0, 0.0, 400.0),
        "thunder" | "storm" => SkyWeather::from_wmo(95, 100.0, 5.0, 4_000.0),
        _ => SkyWeather::clear_fallback(),
    }
}

fn local_hour(now: DateTime<Local>) -> f32 {
    now.hour() as f32 + now.minute() as f32 / 60.0 + now.second() as f32 / 3600.0
}

fn utc_for_local(hour: f32, season: f32, latitude: f64) -> DateTime<Utc> {
    let total = (hour.clamp(0.0, 24.0) * 3600.0).round() as i64;
    let total = total.clamp(0, 24 * 3600 - 1);
    let h = (total / 3600) as u32;
    let m = ((total % 3600) / 60) as u32;
    let s = (total % 60) as u32;
    let date = date_for_season(season, latitude);
    let naive = date
        .and_hms_opt(h, m, s)
        .unwrap_or_else(|| date.and_hms_opt(23, 59, 59).expect("hms"));
    utc_from_naive_in_tz(&Local, naive)
}

/// DST spring-forward gaps step forward; never substitute `Utc::now()`.
fn utc_from_naive_in_tz<Tz: TimeZone>(tz: &Tz, naive: NaiveDateTime) -> DateTime<Utc> {
    let mut candidate = naive;
    for _ in 0..4 {
        match tz.from_local_datetime(&candidate) {
            chrono::LocalResult::Single(dt) | chrono::LocalResult::Ambiguous(dt, _) => {
                return dt.with_timezone(&Utc);
            }
            chrono::LocalResult::None => {
                candidate = match candidate.checked_add_signed(Duration::hours(1)) {
                    Some(next) => next,
                    None => break,
                };
            }
        }
    }
    naive.and_utc()
}

#[derive(Clone, Copy)]
enum Knob {
    Hour,
    Season,
    Cloud,
    Precip,
    Fog,
    Thunder,
}

impl Knob {
    const ALL: [Knob; 6] = [
        Knob::Hour,
        Knob::Season,
        Knob::Cloud,
        Knob::Precip,
        Knob::Fog,
        Knob::Thunder,
    ];

    fn spec(self, zh: bool) -> (&'static str, f32, f32, f32, &'static str) {
        let (id, min, max, step, en, z) = match self {
            Knob::Hour => ("hour", 0.0, 24.0, 0.05, "Time", "时间"),
            Knob::Season => ("season", 0.0, 1.0, 0.01, "Season", "季节"),
            Knob::Cloud => ("cloud", 0.0, 1.0, 0.01, "Cloud", "云量"),
            Knob::Precip => ("precip", 0.0, 1.0, 0.01, "Precip", "降水"),
            Knob::Fog => ("fog", 0.0, 1.0, 0.01, "Fog", "雾"),
            Knob::Thunder => ("thunder", 0.0, 1.0, 0.01, "Thunder", "雷电"),
        };
        (id, min, max, step, if zh { z } else { en })
    }
}

const PRESETS: &[(&str, &str, &str)] = &[
    ("clear", "晴", "Clear"),
    ("cloud", "云", "Cloud"),
    ("rain", "雨", "Rain"),
    ("snow", "雪", "Snow"),
    ("fog", "雾", "Fog"),
    ("thunder", "雷", "Storm"),
];

#[derive(Clone)]
struct Launch {
    params: Arc<Mutex<DebugParams>>,
    persist: Option<PathBuf>,
}

static LAUNCH: Mutex<Option<Launch>> = Mutex::new(None);

#[derive(Clone, Copy)]
enum Message {
    Knob(Knob, f64),
    Live(bool),
    Pause(bool),
    Snow(bool),
    Preset(&'static str),
    Reset,
}

#[derive(Clone, Copy)]
struct Widgets {
    summary: Entity<Text>,
    knobs: [Entity<RangeField>; 6],
    live: Entity<Checkbox>,
    pause: Entity<Checkbox>,
    snow: Entity<Checkbox>,
}

struct DebugPanel {
    params: Arc<Mutex<DebugParams>>,
    persist: Option<PathBuf>,
    initial: DebugParams,
    widgets: Option<Widgets>,
}

pub fn run(params: Arc<Mutex<DebugParams>>, persist: Option<PathBuf>) -> anyhow::Result<()> {
    *LAUNCH.lock().unwrap() = Some(Launch { params, persist });
    let title = panel_title();
    let result = run_runtime::<RuntimeApplication<DebugPanel>>(
        RuntimeWindowSettings::new(title)
            .initial_size(480.0, 640.0)
            .minimum_size(360.0, 480.0),
    );
    *LAUNCH.lock().unwrap() = None;
    result.map_err(|err| anyhow::anyhow!("{err}"))
}

impl ApplicationState for DebugPanel {
    type Message = Message;
    type Error = FrameworkError;

    fn initialize(_: &RuntimeProgramContext<Self::Message>) -> Result<Self, Self::Error> {
        let launch = LAUNCH.lock().unwrap().clone().expect("debug state");
        let initial = *launch.params.lock().unwrap();
        Ok(Self {
            params: launch.params,
            persist: launch.persist,
            initial,
            widgets: None,
        })
    }

    fn build(
        &mut self,
        window: &mut ApplicationWindow,
        _: &RuntimeProgramContext<Self::Message>,
    ) -> Result<(), Self::Error> {
        let zh = Lang::resolve(LanguagePref::System) == Lang::Zh;
        let p = *self.params.lock().unwrap();
        self.widgets = Some(crate::shell::mount_app_shell(
            window,
            panel_title(),
            |ui, _| {
                let summary = ui.child("sum", Text::new(p.summary()));
                let live = ui.child(
                    "live",
                    Checkbox::new(t(zh, "跟随当前时间", "Live clock"), p.live_clock),
                );
                let pause = ui.child(
                    "pause",
                    Checkbox::new(t(zh, "暂停动画", "Pause anim"), p.anim_paused),
                );
                let snow = ui.child("snow", Checkbox::new(t(zh, "雪", "Snow"), p.snow));

                let mut knobs: [Option<Entity<RangeField>>; 6] = [None; 6];
                for (i, knob) in Knob::ALL.into_iter().enumerate() {
                    let (id, min, max, step, label) = knob.spec(zh);
                    let field = ui.child(id, slider(p.knob(knob), min, max, step, label));
                    ui.on(field, move |_, event: &RangeChanged, cx| {
                        cx.dispatch_program(Message::Knob(knob, event.value));
                    });
                    knobs[i] = Some(field);
                }

                ui.row(8.0, |ui| {
                    for (name, z, e) in PRESETS {
                        let btn = ui.child(*name, Button::new(t(zh, z, e)));
                        ui.on(btn, move |_, _: &Activate, cx| {
                            cx.dispatch_program(Message::Preset(name));
                        });
                    }
                });
                let reset = ui.child(
                    "reset",
                    Button::new(t(zh, "重置", "Reset")).kind(ButtonKind::Primary),
                );

                ui.on(live, move |_, event: &ToggleChanged, cx| {
                    cx.dispatch_program(Message::Live(event.checked));
                });
                ui.on(pause, move |_, event: &ToggleChanged, cx| {
                    cx.dispatch_program(Message::Pause(event.checked));
                });
                ui.on(snow, move |_, event: &ToggleChanged, cx| {
                    cx.dispatch_program(Message::Snow(event.checked));
                });
                ui.on(reset, move |_, _: &Activate, cx| {
                    cx.dispatch_program(Message::Reset);
                });

                Widgets {
                    summary,
                    knobs: knobs.map(|k| k.expect("knob")),
                    live,
                    pause,
                    snow,
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
        {
            let mut p = self.params.lock().unwrap();
            match message {
                Message::Knob(k, value) => p.set_knob(k, value as f32),
                Message::Live(on) => {
                    if !on && p.live_clock {
                        p.hour = local_hour(Local::now());
                        p.season = season_from_utc(Utc::now(), p.latitude);
                    }
                    p.live_clock = on;
                }
                Message::Pause(on) => p.anim_paused = on,
                Message::Snow(on) => p.snow = on,
                Message::Preset(name) => p.apply_preset(name),
                Message::Reset => *p = self.initial,
            }
            if let Some(path) = &self.persist
                && let Err(err) = p.save(path)
            {
                eprintln!("debug params save: {err}");
            }
        }
        if let Some(window) = windows.get_mut(&context.window_id())
            && let Some(w) = self.widgets
        {
            let p = *self.params.lock().unwrap();
            let cx = window.document.context_mut();
            let _ = cx.update_component(w.summary, |t, _| t.value = p.summary());
            let _ = cx.update_component(w.live, |c, _| c.checked = p.live_clock);
            let _ = cx.update_component(w.pause, |c, _| c.checked = p.anim_paused);
            let _ = cx.update_component(w.snow, |c, _| c.checked = p.snow);
            for (i, knob) in Knob::ALL.into_iter().enumerate() {
                let _ = cx.update_component(w.knobs[i], |r, _| {
                    r.value = f64::from(p.knob(knob)).clamp(r.minimum, r.maximum);
                });
            }
        }
        RuntimeProgramUpdate::redraw(context.window_id())
    }
}

fn panel_title() -> &'static str {
    if Lang::resolve(LanguagePref::System) == Lang::Zh {
        "天空壁纸 Debug"
    } else {
        "SkyWallpaper Debug"
    }
}

fn t(zh: bool, z: &'static str, e: &'static str) -> &'static str {
    if zh { z } else { e }
}

fn slider(value: f32, min: f32, max: f32, step: f32, label: &str) -> RangeField {
    let v = f64::from(value.clamp(min, max));
    RangeField::new(v, f64::from(min), f64::from(max), f64::from(step)).label(label)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sky_gpu::SkyUniforms;

    const BEIJING: (f64, f64) = (39.9042, 116.4074);

    fn params() -> DebugParams {
        DebugParams::from_live(BEIJING.0, BEIJING.1, SkyWeather::clear_fallback())
    }

    /// 12:00 in China Standard Time (UTC+8, no DST) on the season's civil date.
    fn beijing_noon_utc(season: f32) -> DateTime<Utc> {
        let cst = chrono::FixedOffset::east_opt(8 * 3600).expect("UTC+8");
        let naive = date_for_season(season, BEIJING.0)
            .and_hms_opt(12, 0, 0)
            .expect("noon");
        cst.from_local_datetime(&naive)
            .single()
            .expect("CST has no DST gap")
            .with_timezone(&Utc)
    }

    #[test]
    fn frozen_hour_changes_sun_altitude() {
        let mut day = params();
        day.live_clock = false;
        day.season = 0.5;
        day.hour = 12.0;
        let mut night = day;
        night.hour = 0.0;
        assert!(
            (day.build_view().sun.altitude_deg - night.build_view().sun.altitude_deg).abs() > 5.0
        );
    }

    #[test]
    fn season_changes_beijing_noon_altitude() {
        let weather = SkyWeather::clear_fallback();
        let summer = SkyView::at(BEIJING.0, BEIJING.1, beijing_noon_utc(0.5), weather);
        let winter = SkyView::at(BEIJING.0, BEIJING.1, beijing_noon_utc(0.0), weather);
        assert!(
            summer.sun.altitude_deg > winter.sun.altitude_deg + 20.0,
            "summer {:.1} winter {:.1}",
            summer.sun.altitude_deg,
            winter.sun.altitude_deg
        );
        assert!((summer.season - 0.5).abs() < 0.03);
        assert!(winter.season < 0.03 || winter.season > 0.97);
    }

    #[test]
    fn knobs_reach_uniforms() {
        let mut p = params();
        p.live_clock = false;
        p.cloud_cover = 0.8;
        p.season = 0.5;
        p.hour = 12.0;
        let view = p.build_view();
        let u = SkyUniforms::from_view(&view, 1280, 720, 1.0, p.thunder);
        assert!((u.cloud_cover - 0.8).abs() < 1e-4);
        assert!(
            (u.season - 0.5).abs() < 0.03,
            "season knob should reach uniforms: {}",
            u.season
        );
        assert_eq!(u.sun_dir[2], 0.0);
    }

    fn season_wrap_dist(a: f32, b: f32) -> f32 {
        let d = (a - b).abs();
        d.min(1.0 - d)
    }

    #[test]
    fn frozen_season_survives_local_utc_roundtrip() {
        for s in [0.0, 0.25, 0.5, 0.75] {
            let utc = utc_for_local(12.0, s, BEIJING.0);
            let got = season_from_utc(utc, BEIJING.0);
            assert!(
                season_wrap_dist(got, s) < 0.02,
                "season {s} round-tripped to {got}"
            );
        }
    }

    #[test]
    fn dst_gap_stays_near_requested_civil_time() {
        use chrono::NaiveDate;
        use chrono_tz::Europe::London;
        let naive = NaiveDate::from_ymd_opt(2025, 3, 30)
            .expect("date")
            .and_hms_opt(1, 30, 0)
            .expect("01:30");
        assert!(
            matches!(
                London.from_local_datetime(&naive),
                chrono::LocalResult::None
            ),
            "2025-03-30 01:30 must be a London DST gap"
        );
        let utc = utc_from_naive_in_tz(&London, naive);
        let got = utc.with_timezone(&London).naive_local();
        let delta_min = (got - naive).num_minutes().abs();
        assert!(
            delta_min <= 60,
            "DST gap must step about 1h: requested {naive}, got {got} ({delta_min} min)"
        );
    }

    #[test]
    fn rain_preset_and_roundtrip() {
        let mut p = params();
        p.apply_preset("rain");
        assert!(p.precip > 0.2 && p.cloud_cover > 0.5 && !p.snow);
        let dir =
            std::env::temp_dir().join(format!("skywallpaper-debug-test-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("params.toml");
        p.hour = 6.5;
        p.season = 0.25;
        p.save(&path).unwrap();
        let loaded = DebugParams::load(&path).unwrap();
        assert!((loaded.hour - 6.5).abs() < 1e-4);
        assert!((loaded.season - 0.25).abs() < 1e-4);
        assert!((loaded.precip - p.precip).abs() < 1e-4);
        let _ = fs::remove_dir_all(dir);
    }
}
