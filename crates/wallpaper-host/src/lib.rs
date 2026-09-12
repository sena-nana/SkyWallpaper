mod engine;
mod fullscreen;
mod power;
mod workerw;

pub use engine::{EngineError, WallpaperEngine};
pub use fullscreen::{foreground_is_fullscreen, work_areas_occluded};
pub use power::on_battery;
pub use workerw::{WorkerWError, take_display_changed};

pub fn target_fps(paused: bool, battery: bool, fullscreen: bool, occluded: bool) -> u32 {
    if paused || fullscreen {
        0
    } else if occluded {
        1
    } else if battery {
        15
    } else {
        30
    }
}

#[cfg(test)]
mod tests {
    use super::target_fps;

    #[test]
    fn fps_pauses_for_fullscreen_and_manual_pause() {
        assert_eq!(target_fps(false, false, false, false), 30);
        assert_eq!(target_fps(false, true, false, false), 15);
        assert_eq!(target_fps(true, false, false, false), 0);
        assert_eq!(target_fps(false, false, true, false), 0);
        assert_eq!(target_fps(true, true, true, false), 0);
        assert_eq!(target_fps(false, false, false, true), 1);
        assert_eq!(target_fps(false, true, false, true), 1);
        assert_eq!(target_fps(true, false, false, true), 0);
    }
}
