use std::time::{Duration, Instant};
use ascii_core::model::Timeline;

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum PlaybackState {
    Stopped,
    Playing,
}

pub struct PlaybackController {
    state: PlaybackState,
    last_advance: Option<Instant>,
}

impl PlaybackController {
    pub fn new() -> Self {
        Self {
            state: PlaybackState::Stopped,
            last_advance: None,
        }
    }

    #[allow(dead_code)]
    pub fn state(&self) -> PlaybackState {
        self.state
    }

    pub fn is_playing(&self) -> bool {
        self.state == PlaybackState::Playing
    }

    pub fn play(&mut self, now: Instant) {
        self.state = PlaybackState::Playing;
        self.last_advance = Some(now);
    }

    pub fn pause(&mut self) {
        self.state = PlaybackState::Stopped;
        self.last_advance = None;
    }

    pub fn tick(&mut self, now: Instant, tl: &mut Timeline) -> Option<Duration> {
        if self.state != PlaybackState::Playing {
            return None;
        }

        let last = match self.last_advance {
            Some(l) => l,
            None => {
                self.last_advance = Some(now);
                return Some(tl.fps.frame_duration());
            }
        };

        let period = tl.fps.frame_duration();
        let elapsed = now.saturating_duration_since(last);

        if elapsed >= period {
            let n = (elapsed.as_secs_f64() / period.as_secs_f64()).floor() as usize;
            let n = n.max(1);
            let added_duration = Duration::from_secs_f64(n as f64 * period.as_secs_f64());
            self.last_advance = Some(last + added_duration);

            let len = tl.len();
            let selected = tl.selected_index();

            if !tl.looping && (selected + n >= len) {
                let _ = tl.select(len.saturating_sub(1));
                self.state = PlaybackState::Stopped;
                None
            } else {
                let target = if tl.looping {
                    (selected + n) % len
                } else {
                    (selected + n).min(len.saturating_sub(1))
                };
                let _ = tl.select(target);
                let current_last = self.last_advance.unwrap_or(now);
                let rem = period.saturating_sub(now.saturating_duration_since(current_last));
                Some(rem)
            }
        } else {
            Some(period.saturating_sub(elapsed))
        }
    }
}

impl Default for PlaybackController {
    fn default() -> Self {
        Self::new()
    }
}
