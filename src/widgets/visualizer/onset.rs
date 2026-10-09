//! Beat-onset hysteresis shared by the beat-driven effects (the Reflection's
//! kick ripples, the Tunnel's lit rings): one event per kick, read off the
//! normalized beat pulse (`VisualizerState::current_beat_pulse`, ~1.0 on a
//! kick in loud and quiet songs alike).

/// A beat pulse above this is an onset...
const TRIGGER: f32 = 0.6;
/// ...and the next one waits for the pulse to fall below this.
const REARM: f32 = 0.35;

pub(crate) struct BeatTrigger {
    armed: bool,
}

impl BeatTrigger {
    pub(crate) fn new() -> Self {
        Self { armed: true }
    }

    /// Re-arm (a track change).
    pub(crate) fn reset(&mut self) {
        self.armed = true;
    }

    /// One tick of the beat pulse: true on the tick an onset fires.
    pub(crate) fn fire(&mut self, beat: f32) -> bool {
        if beat < REARM {
            self.armed = true;
        }
        if self.armed && beat > TRIGGER {
            self.armed = false;
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fires_once_per_kick_and_rearms_when_the_pulse_falls() {
        let mut t = BeatTrigger::new();
        assert!(!t.fire(0.2), "a quiet pulse is no onset");
        assert!(t.fire(0.9), "the kick fires");
        assert!(!t.fire(0.8), "still high: the same kick");
        assert!(!t.fire(0.5), "between the levels: not re-armed yet");
        assert!(!t.fire(0.7), "so a bump here is not a new onset");
        assert!(!t.fire(0.3), "below the re-arm level");
        assert!(t.fire(0.9), "the next kick fires");
    }

    #[test]
    fn reset_rearms() {
        let mut t = BeatTrigger::new();
        assert!(t.fire(0.9));
        t.reset();
        assert!(
            t.fire(0.9),
            "a reset trigger fires on a pulse that is still high"
        );
    }
}
