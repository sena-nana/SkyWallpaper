//! Session-only `--debug` knobs for the sky preview.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Local, TimeZone, Timelike, Utc};
use nana_ui::runtime::{
    Activate, Button, Checkbox, Entity, FrameworkError, List, RangeChanged, RangeField, Text,
    ToggleChanged,
};
use nana_ui::{
    ApplicationState, ApplicationWindow, ButtonKind, RuntimeApplication, RuntimeProgramContext,
    RuntimeProgramUpdate, RuntimeWindowSettings, run_runtime,
};
use nana_ui_platform::WindowId;
use serde::{Deserialize, Serialize};
use sky_core::{PrecipKind, SkyView, SkyWeather, WeatherCode, enu_from_alt_az};

use crate::config::LanguagePref;
use crate::i18n::Lang;

/// Uniform scalars that affect the picture, excluding `resolution` (2).
pub const TUNABLE_COUNT: usize = 12;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct DebugParams {
    pub latitude: f64,
    pub longitude: f64,
    pub live_clock: bool,
    pub hour: f32,
    pub override_sun: bool,
    pub sun_alt: f32,
    pub sun_az: f32,
    pub anim_paused: bool,
    pub cloud_cover: f32,
    pub precip: f32,
    pub snow: bool,
    pub fog: f32,
    pub thunder: f32,
    pub cam_pitch: f32,
    pub cam_yaw: f32,
    pub exposure: f32,
}

impl DebugParams {
    pub fn from_live(latitude: f64, longitude: f64, weather: SkyWeather) -> Self {
        let view = SkyView::now(latitude, longitude, weather);
        Self {
            latitude,
            longitude,
            live_clock: true,
            hour: local_hour(Local::now()),
            override_sun: false,
            sun_alt: view.sun.altitude_deg as f32,
            sun_az: view.sun.azimuth_deg as f32,
            anim_paused: false,
            cloud_cover: weather.cloud_cover,
            precip: weather.precip,
            snow: weather.precip_kind == PrecipKind::Snow,
            fog: weather.fog,
            thunder: if weather.thunder { 0.7 } else { 0.0 },
            cam_pitch: view.cam_pitch_deg,
            cam_yaw: view.cam_yaw_deg,
            exposure: view.exposure,
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
            utc_for_local_hour(self.hour)
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
        let mut view = SkyView::at(self.latitude, self.longitude, utc, weather);
        if self.override_sun {
            let alt = f64::from(self.sun_alt);
            let az = f64::from(self.sun_az);
            view.sun.altitude_deg = alt;
            view.sun.azimuth_deg = az;
            view.sun.dir_enu = enu_from_alt_az(alt, az);
        }
        view.cam_pitch_deg = self.cam_pitch;
        view.cam_yaw_deg = self.cam_yaw;
        view.exposure = self.exposure;
        view
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
            "{TUNABLE_COUNT} knobs  hour={:.1}  cloud={:.0}%  precip={:.0}%",
            self.hour,
            self.cloud_cover * 100.0,
            self.precip * 100.0
        )
    }

    fn knob(&self, k: Knob) -> f32 {
        match k {
            Knob::Hour => self.hour,
            Knob::Alt => self.sun_alt,
            Knob::Az => self.sun_az,
            Knob::Cloud => self.cloud_cover,
            Knob::Precip => self.precip,
            Knob::Fog => self.fog,
            Knob::Thunder => self.thunder,
            Knob::Pitch => self.cam_pitch,
            Knob::Yaw => self.cam_yaw,
            Knob::Exposure => self.exposure,
        }
    }

    fn set_knob(&mut self, k: Knob, value: f32) {
        match k {
            Knob::Hour => {
                self.live_clock = false;
                self.hour = value;
            }
            Knob::Alt => {
                self.override_sun = true;
                self.sun_alt = value;
            }
            Knob::Az => {
                self.override_sun = true;
                self.sun_az = value;
            }
            Knob::Cloud => self.cloud_cover = value,
            Knob::Precip => self.precip = value,
            Knob::Fog => self.fog = value,
            Knob::Thunder => self.thunder = value,
            Knob::Pitch => self.cam_pitch = value,
            Knob::Yaw => self.cam_yaw = value,
            Knob::Exposure => self.exposure = value,
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

fn utc_for_local_hour(hour: f32) -> DateTime<Utc> {
    let total = (hour.clamp(0.0, 24.0) * 3600.0).round() as i64;
    let total = total.clamp(0, 24 * 3600 - 1);
    let h = (total / 3600) as u32;
    let m = ((total % 3600) / 60) as u32;
    let s = (total % 60) as u32;
    let now = Local::now();
    let naive = now
        .date_naive()
        .and_hms_opt(h, m, s)
        .unwrap_or_else(|| now.date_naive().and_hms_opt(23, 59, 59).expect("hms"));
    match now.timezone().from_local_datetime(&naive) {
        chrono::LocalResult::Single(dt) | chrono::LocalResult::Ambiguous(dt, _) => {
            dt.with_timezone(&Utc)
        }
        chrono::LocalResult::None => Utc::now(),
    }
}

#[derive(Clone, Copy)]
enum Knob {
    Hour,
    Alt,
    Az,
    Cloud,
    Precip,
    Fog,
    Thunder,
    Pitch,
    Yaw,
    Exposure,
}

impl Knob {
    const ALL: [Knob; 10] = [
        Knob::Hour,
        Knob::Alt,
        Knob::Az,
        Knob::Cloud,
        Knob::Precip,
        Knob::Fog,
        Knob::Thunder,
        Knob::Pitch,
        Knob::Yaw,
        Knob::Exposure,
    ];

    fn spec(self, zh: bool) -> (&'static str, f32, f32, f32, &'static str) {
        let (id, min, max, step, en, z) = match self {
            Knob::Hour => ("hour", 0.0, 24.0, 0.05, "Hour", "时刻"),
            Knob::Alt => ("alt", -90.0, 90.0, 0.5, "Sun alt", "太阳高度"),
            Knob::Az => ("az", 0.0, 360.0, 1.0, "Sun az", "太阳方位"),
            Knob::Cloud => ("cloud", 0.0, 1.0, 0.01, "Cloud", "云量"),
            Knob::Precip => ("precip", 0.0, 1.0, 0.01, "Precip", "降水"),
            Knob::Fog => ("fog", 0.0, 1.0, 0.01, "Fog", "雾"),
            Knob::Thunder => ("thunder", 0.0, 1.0, 0.01, "Thunder", "雷电"),
            Knob::Pitch => ("pitch", -15.0, 80.0, 0.5, "Pitch", "俯仰"),
            Knob::Yaw => ("yaw", 0.0, 360.0, 1.0, "Yaw", "方位"),
            Knob::Exposure => ("exp", 0.2, 3.0, 0.01, "Exposure", "曝光"),
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
    OverrideSun(bool),
    Snow(bool),
    Preset(&'static str),
    Reset,
}

#[derive(Clone, Copy)]
struct Widgets {
    summary: Entity<Text>,
    knobs: [Entity<RangeField>; 10],
    live: Entity<Checkbox>,
    pause: Entity<Checkbox>,
    override_sun: Entity<Checkbox>,
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
    let zh = Lang::resolve(LanguagePref::System) == Lang::Zh;
    let title = if zh {
        "天空壁纸 Debug"
    } else {
        "SkyWallpaper Debug"
    };
    let result = run_runtime::<RuntimeApplication<DebugPanel>>(
        RuntimeWindowSettings::new(title).initial_size(480.0, 800.0),
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
        let document = window.document.document();
        let widgets = window.document.context_mut().build(document, |ui| {
            ui.with("root", List::new(), |ui| {
                let summary = ui.child("sum", Text::new(p.summary()));
                let live = ui.child(
                    "live",
                    Checkbox::new(
                        if zh {
                            "跟随当前时刻"
                        } else {
                            "Live clock"
                        },
                        p.live_clock,
                    ),
                );
                let pause = ui.child(
                    "pause",
                    Checkbox::new(if zh { "暂停动画" } else { "Pause anim" }, p.anim_paused),
                );
                let override_sun = ui.child(
                    "osun",
                    Checkbox::new(
                        if zh {
                            "手动太阳位置"
                        } else {
                            "Override sun"
                        },
                        p.override_sun,
                    ),
                );
                let snow = ui.child(
                    "snow",
                    Checkbox::new(if zh { "雪" } else { "Snow" }, p.snow),
                );

                let mut knobs: [Option<Entity<RangeField>>; 10] = [None; 10];
                for (i, knob) in Knob::ALL.into_iter().enumerate() {
                    let (id, min, max, step, label) = knob.spec(zh);
                    let field = ui.child(id, slider(p.knob(knob), min, max, step, label));
                    ui.on(field, move |_, event: &RangeChanged, cx| {
                        cx.dispatch_program(Message::Knob(knob, event.value));
                    });
                    knobs[i] = Some(field);
                }

                ui.with("presets", List::new(), |ui| {
                    for (name, z, e) in PRESETS {
                        let btn = ui.child(*name, Button::new(if zh { *z } else { *e }));
                        ui.on(btn, move |_, _: &Activate, cx| {
                            cx.dispatch_program(Message::Preset(name));
                        });
                    }
                });
                let reset = ui.child(
                    "reset",
                    Button::new(if zh { "重置" } else { "Reset" }).kind(ButtonKind::Primary),
                );

                ui.on(live, move |_, event: &ToggleChanged, cx| {
                    cx.dispatch_program(Message::Live(event.checked));
                });
                ui.on(pause, move |_, event: &ToggleChanged, cx| {
                    cx.dispatch_program(Message::Pause(event.checked));
                });
                ui.on(override_sun, move |_, event: &ToggleChanged, cx| {
                    cx.dispatch_program(Message::OverrideSun(event.checked));
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
                    override_sun,
                    snow,
                }
            })
        })?;
        self.widgets = Some(widgets);
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
                    }
                    p.live_clock = on;
                }
                Message::Pause(on) => p.anim_paused = on,
                Message::OverrideSun(on) => {
                    if on && !p.override_sun {
                        let view = p.build_view();
                        p.sun_alt = view.sun.altitude_deg as f32;
                        p.sun_az = view.sun.azimuth_deg as f32;
                    }
                    p.override_sun = on;
                }
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
            let _ = cx.update_component(w.override_sun, |c, _| c.checked = p.override_sun);
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

fn slider(value: f32, min: f32, max: f32, step: f32, label: &str) -> RangeField {
    let v = f64::from(value.clamp(min, max));
    RangeField::new(v, f64::from(min), f64::from(max), f64::from(step))
        .unwrap_or_else(|_| {
            RangeField::new(
                f64::from(min),
                f64::from(min),
                f64::from(max),
                f64::from(step),
            )
            .expect("range")
        })
        .label(label)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sky_gpu::SkyUniforms;

    fn params() -> DebugParams {
        DebugParams::from_live(39.9042, 116.4074, SkyWeather::clear_fallback())
    }

    #[test]
    fn frozen_hour_changes_sun_altitude() {
        let mut day = params();
        day.live_clock = false;
        day.override_sun = false;
        day.hour = 12.0;
        let mut night = day;
        night.hour = 0.0;
        assert!(
            (day.build_view().sun.altitude_deg - night.build_view().sun.altitude_deg).abs() > 5.0
        );
    }

    #[test]
    fn knobs_reach_uniforms() {
        let mut p = params();
        p.cam_pitch = 42.0;
        p.cloud_cover = 0.8;
        p.override_sun = true;
        p.sun_alt = 45.0;
        p.sun_az = 90.0;
        let view = p.build_view();
        let u = SkyUniforms::from_view(&view, 1280, 720, 1.0, p.thunder);
        assert!((u.cam_pitch - 42.0).abs() < 1e-4);
        assert!((u.cloud_cover - 0.8).abs() < 1e-4);
        let expected = enu_from_alt_az(45.0, 90.0);
        for i in 0..3 {
            assert!((view.sun.dir_enu[i] - expected[i]).abs() < 1e-5);
        }
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
        p.save(&path).unwrap();
        let loaded = DebugParams::load(&path).unwrap();
        assert!((loaded.hour - 6.5).abs() < 1e-4);
        assert!((loaded.precip - p.precip).abs() < 1e-4);
        let _ = fs::remove_dir_all(dir);
    }
}
