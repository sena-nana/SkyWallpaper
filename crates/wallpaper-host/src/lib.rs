mod engine;
mod fullscreen;
mod power;
mod workerw;

pub use engine::{EngineError, WallpaperEngine};
pub use fullscreen::foreground_is_fullscreen;
pub use power::on_battery;
pub use workerw::{take_display_changed, WorkerWError};

pub fn target_fps(paused: bool, battery: bool, fullscreen: bool) -> u32 {
    if paused || fullscreen {
        0
    } else if battery {
        15
    } else {
        30
    }
}
