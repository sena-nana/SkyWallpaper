use crate::config::LanguagePref;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Zh,
    En,
}

impl Lang {
    pub fn resolve(pref: LanguagePref) -> Self {
        match pref {
            LanguagePref::Zh => Lang::Zh,
            LanguagePref::En => Lang::En,
            LanguagePref::System => system_lang(),
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Lang::Zh => "zh",
            Lang::En => "en",
        }
    }
}

fn system_lang() -> Lang {
    #[cfg(windows)]
    {
        use windows::Win32::Globalization::GetUserDefaultUILanguage;
        let langid = unsafe { GetUserDefaultUILanguage() };
        let primary = langid & 0x3FF;
        if primary == 0x04 {
            return Lang::Zh;
        }
    }
    Lang::En
}

#[derive(Debug, Clone, Copy)]
pub struct Text {
    pub app: &'static str,
    pub settings: &'static str,
    pub location: &'static str,
    pub search: &'static str,
    pub use_ip: &'static str,
    pub language: &'static str,
    pub follow_system: &'static str,
    pub chinese: &'static str,
    pub english: &'static str,
    pub autostart: &'static str,
    pub pause_fullscreen: &'static str,
    pub pause: &'static str,
    pub resume: &'static str,
    pub refresh_weather: &'static str,
    pub exit: &'static str,
    pub open_settings: &'static str,
    pub weather: &'static str,
    pub running: &'static str,
    pub paused: &'static str,
    pub about: &'static str,
}

pub fn t(lang: Lang) -> Text {
    match lang {
        Lang::Zh => Text {
            app: "天空壁纸",
            settings: "天空壁纸设置",
            location: "地点",
            search: "搜索",
            use_ip: "使用 IP 粗定位",
            language: "语言",
            follow_system: "跟随系统",
            chinese: "中文",
            english: "English",
            autostart: "开机自启",
            pause_fullscreen: "全屏游戏时暂停",
            pause: "暂停壁纸",
            resume: "继续壁纸",
            refresh_weather: "刷新天气",
            exit: "退出",
            open_settings: "设置",
            weather: "天气",
            running: "运行中",
            paused: "已暂停",
            about: "天色随本地时间变化。天气来自 Open-Meteo，失败时回退晴天。",
        },
        Lang::En => Text {
            app: "SkyWallpaper",
            settings: "SkyWallpaper Settings",
            location: "Location",
            search: "Search",
            use_ip: "Use IP location",
            language: "Language",
            follow_system: "System",
            chinese: "中文",
            english: "English",
            autostart: "Start with Windows",
            pause_fullscreen: "Pause on fullscreen",
            pause: "Pause wallpaper",
            resume: "Resume wallpaper",
            refresh_weather: "Refresh weather",
            exit: "Quit",
            open_settings: "Settings",
            weather: "Weather",
            running: "Running",
            paused: "Paused",
            about: "Sky color follows local time. Weather from Open-Meteo; failures fall back to clear sky.",
        },
    }
}
