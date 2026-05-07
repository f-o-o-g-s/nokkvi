//! Tests for surfing-boat overlay update handlers.

// Surfing-Boat Overlay Handler (boat.rs)
// ============================================================================

mod boat_tests {
    use std::time::{Duration, Instant};

    use nokkvi_data::types::player_settings::VisualizationMode;

    use crate::{app_message::Message, test_helpers::*};

    /// Enable the boat toggle in the shared visualizer config.
    fn enable_boat_in_config(app: &crate::Nokkvi, on: bool) {
        let mut cfg = app.visualizer_config.write();
        cfg.lines.boat = on;
    }

    #[test]
    fn boat_visible_only_in_lines_mode() {
        let mut app = test_app();
        enable_boat_in_config(&app, true);

        // Default mode is Bars — boat should stay hidden even with toggle on.
        app.engine.visualization_mode = VisualizationMode::Bars;
        let _ = app.update(Message::BoatTick(Instant::now()));
        assert!(
            !app.boat.visible,
            "boat must be hidden in Bars mode regardless of the boat toggle"
        );

        // Switch to Lines — boat should now be visible.
        app.engine.visualization_mode = VisualizationMode::Lines;
        let _ = app.update(Message::BoatTick(Instant::now()));
        assert!(
            app.boat.visible,
            "boat must be visible in Lines mode when the toggle is on"
        );
    }

    #[test]
    fn boat_hidden_when_visualizer_disabled() {
        let mut app = test_app();
        enable_boat_in_config(&app, true);
        // VisualizationMode::Off is what mounts the visualizer at all (see
        // app_view.rs). When Off, the boat must also be hidden.
        app.engine.visualization_mode = VisualizationMode::Off;
        let _ = app.update(Message::BoatTick(Instant::now()));
        assert!(
            !app.boat.visible,
            "boat must be hidden when the visualizer is fully off"
        );
    }

    #[test]
    fn boat_hidden_when_settings_toggle_off() {
        let mut app = test_app();
        // Lines mode active, but the user's boat toggle is off (the default).
        app.engine.visualization_mode = VisualizationMode::Lines;
        let _ = app.update(Message::BoatTick(Instant::now()));
        assert!(
            !app.boat.visible,
            "boat must respect the user's `lines.boat` toggle"
        );
    }

    #[test]
    fn boat_advances_x_ratio_on_tick() {
        let mut app = test_app();
        enable_boat_in_config(&app, true);
        app.engine.visualization_mode = VisualizationMode::Lines;

        // The boat is purely propelled by music in the new model and
        // `test_app()` has no visualizer / no BPM — so we seed a
        // non-zero `x_velocity` and verify the integrator advances
        // `x_ratio` from it. This pins "the handler actually ticks
        // step()" without depending on the music pipeline.
        app.boat.x_velocity = 0.05;
        app.boat.facing = 1.0;

        let t0 = Instant::now();
        let _ = app.update(Message::BoatTick(t0));
        let x0 = app.boat.x_ratio;

        let t1 = t0 + Duration::from_millis(100);
        let _ = app.update(Message::BoatTick(t1));
        let x1 = app.boat.x_ratio;

        assert_ne!(
            x0, x1,
            "two ticks 100 ms apart in lines mode must move the boat \
             when seeded with non-zero velocity (got x0={x0}, x1={x1})"
        );
    }

    #[test]
    fn boat_state_resumes_after_mode_round_trip() {
        let mut app = test_app();
        enable_boat_in_config(&app, true);
        app.engine.visualization_mode = VisualizationMode::Lines;

        // Tick a couple of times to seat `last_tick`, advance physics,
        // and let the tack countdown decrement.
        let t0 = Instant::now();
        let _ = app.update(Message::BoatTick(t0));
        let _ = app.update(Message::BoatTick(t0 + Duration::from_millis(50)));
        let saved_tack = app.boat.secs_until_next_tack;
        let saved_x = app.boat.x_ratio;
        assert!(
            saved_tack > 0.0,
            "tack countdown must have been seeded by the first integrating \
             tick (got {saved_tack})"
        );

        // Switch to Bars — boat hides, physics fields preserved.
        app.engine.visualization_mode = VisualizationMode::Bars;
        let _ = app.update(Message::BoatTick(t0 + Duration::from_millis(100)));
        assert!(!app.boat.visible);
        assert_eq!(
            app.boat.secs_until_next_tack, saved_tack,
            "tack countdown must NOT advance while hidden \
             (saved={saved_tack}, now={})",
            app.boat.secs_until_next_tack
        );
        assert_eq!(
            app.boat.x_ratio, saved_x,
            "x_ratio must NOT advance while hidden \
             (saved={saved_x}, now={})",
            app.boat.x_ratio
        );

        // Back to Lines — state resumes from where it left off (the
        // first re-show tick has dt=0 because last_tick was cleared).
        app.engine.visualization_mode = VisualizationMode::Lines;
        let _ = app.update(Message::BoatTick(t0 + Duration::from_millis(150)));
        assert!(app.boat.visible);
        assert_eq!(
            app.boat.secs_until_next_tack, saved_tack,
            "tack countdown preserved across the round trip"
        );
        assert_eq!(
            app.boat.x_ratio, saved_x,
            "x_ratio preserved across the round trip"
        );
    }

    #[test]
    fn boat_clears_last_tick_when_hidden() {
        // Regression: when hidden the handler must drop `last_tick` so the
        // first frame back doesn't see a stale multi-second gap.
        let mut app = test_app();
        enable_boat_in_config(&app, true);
        app.engine.visualization_mode = VisualizationMode::Lines;

        let t0 = Instant::now();
        let _ = app.update(Message::BoatTick(t0));
        assert!(app.boat.last_tick.is_some());

        app.engine.visualization_mode = VisualizationMode::Off;
        let _ = app.update(Message::BoatTick(t0 + Duration::from_secs(5)));
        assert!(
            app.boat.last_tick.is_none(),
            "last_tick must be cleared while hidden so re-show starts with dt=0"
        );
    }

    #[test]
    fn boat_freezes_while_audio_paused() {
        // Audio pause: the visualizer waveform decays to silence (the FFT
        // thread's sample buffer empties), so integrating sail thrust
        // against a flat line still walks the boat across an empty wave.
        // Every dynamic physics field must hold while `playback.paused`.
        let mut app = test_app();
        enable_boat_in_config(&app, true);
        app.engine.visualization_mode = VisualizationMode::Lines;

        // Seed non-default values so an accidental "field stayed at 0
        // because it was already 0" pass can't sneak through.
        app.boat.x_ratio = 0.6;
        app.boat.x_velocity = 0.05;
        app.boat.y_ratio = 0.7;
        app.boat.y_velocity = 0.02;
        app.boat.facing = 1.0;

        // First tick seats `last_tick`; dt=0 keeps the snapshot intact.
        let t0 = Instant::now();
        let _ = app.update(Message::BoatTick(t0));
        let snap = app.boat.clone();

        // Pause and tick after a long gap — under the bug the boat would
        // integrate a half-second of sail thrust against an empty bar buffer.
        app.playback.paused = true;
        let _ = app.update(Message::BoatTick(t0 + Duration::from_millis(500)));

        assert_eq!(
            app.boat.x_ratio, snap.x_ratio,
            "x_ratio must hold while paused"
        );
        assert_eq!(
            app.boat.x_velocity, snap.x_velocity,
            "x_velocity must hold while paused"
        );
        assert_eq!(
            app.boat.y_ratio, snap.y_ratio,
            "y_ratio must hold while paused"
        );
        assert_eq!(
            app.boat.y_velocity, snap.y_velocity,
            "y_velocity must hold while paused"
        );
        assert_eq!(
            app.boat.secs_until_next_tack, snap.secs_until_next_tack,
            "tack countdown must hold while paused"
        );
        assert!(
            app.boat.visible,
            "boat must still render while paused — it just stops moving"
        );
        assert!(
            app.boat.last_tick.is_none(),
            "last_tick must clear so the first tick after resume sees dt=0 \
             (same contract as the hidden branch)"
        );
    }

    #[test]
    fn boat_handler_runs_physics_when_not_playing() {
        // The not-playing path must KEEP ticking physics — the boat
        // smoothly relaxes to the bottom under the silence-override
        // rather than freezing. This guards against a regression where
        // someone copies the pause-fix's early-return and accidentally
        // freezes the boat the moment a track ends.
        //
        // Under the music-only-thrust model the boat doesn't move on
        // silence, so we seed a non-zero `x_velocity` and verify it
        // damps over the not-playing ticks. The handler IS running
        // step() if velocity decays toward zero; if step() were
        // skipped the velocity would persist verbatim.
        let mut app = test_app();
        enable_boat_in_config(&app, true);
        app.engine.visualization_mode = VisualizationMode::Lines;
        app.playback.playing = false;
        app.playback.paused = false;

        app.boat.facing = 1.0;
        app.boat.x_velocity = 0.05;

        let t0 = Instant::now();
        let _ = app.update(Message::BoatTick(t0));
        let v_after_first = app.boat.x_velocity;
        let _ = app.update(Message::BoatTick(t0 + Duration::from_millis(100)));

        assert!(
            app.boat.visible,
            "boat must remain visible while not-playing"
        );
        assert!(
            app.boat.last_tick.is_some(),
            "last_tick must update — physics still ticks when not-playing"
        );
        assert!(
            app.boat.x_velocity < v_after_first,
            "x_velocity must decay between not-playing ticks (the \
             silence override drops bars but does NOT skip step() the \
             way pause does); got v_after_first = {v_after_first}, \
             v_after_second = {}",
            app.boat.x_velocity
        );
    }

    #[test]
    fn boat_resumes_motion_after_unpause() {
        // The pause freeze must not be sticky — once `paused` flips back to
        // false, the next ticks integrate physics again. We seed an initial
        // velocity so the boat has something to move with even without
        // music signals, and verify x_ratio mutates after unpause.
        let mut app = test_app();
        enable_boat_in_config(&app, true);
        app.engine.visualization_mode = VisualizationMode::Lines;
        app.boat.facing = 1.0;
        app.boat.x_velocity = 0.08;

        let t0 = Instant::now();
        let _ = app.update(Message::BoatTick(t0));

        app.playback.paused = true;
        let _ = app.update(Message::BoatTick(t0 + Duration::from_millis(200)));
        let frozen_x = app.boat.x_ratio;

        // Resume. First tick after unpause sees dt=0 (last_tick was cleared);
        // the second tick has a real gap and must mutate position.
        app.playback.paused = false;
        let _ = app.update(Message::BoatTick(t0 + Duration::from_millis(300)));
        let _ = app.update(Message::BoatTick(t0 + Duration::from_millis(400)));

        assert_ne!(
            app.boat.x_ratio, frozen_x,
            "boat must integrate again after unpause (x_ratio still {frozen_x})"
        );
    }
}

// ============================================================================
