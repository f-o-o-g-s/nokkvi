//! Best-effort read of the PipeWire graph to NAME the app holding the audio
//! device — the "who" half of the bit-perfect blocker diagnostic (Tier B).
//!
//! When bit-perfect playback is RESAMPLED, the `/proc/asound` probe gives the
//! device *rate* (the "what"); this gives the "who": another
//! `Stream/Output/Audio` node linked to the same sink nokkvi feeds, by its
//! application name (e.g. "Firefox", "Noctalia"). It runs a SHORT-LIVED registry
//! roundtrip (the canonical pw-rs pattern) on its OWN mainloop — never the audio
//! sink's mainloop — so it can't disturb playback. It blocks until the initial
//! enumeration completes, so call it OFF the audio + UI threads (a worker).
//! Fully best-effort: any failure → `None`, and the badge simply omits the name.
//!
//! The topology logic ([`resolve_sink_holder`]) is split from the PipeWire I/O
//! so it can be unit-tested with synthetic graphs; the FFI half can only be
//! verified live against a real graph.

/// A node in the PipeWire graph snapshot — only the props the resolver needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GraphNode {
    pub id: u32,
    pub app_name: Option<String>,
    pub media_class: Option<String>,
    pub node_name: Option<String>,
}

/// A link (output node → input node) in the graph snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct GraphLink {
    pub output_node: u32,
    pub input_node: u32,
}

/// Find the app holding the audio sink that OUR node (matched by `node.name`)
/// feeds: another `Stream/Output/Audio` node linked to the same `Audio/Sink`.
/// Returns its application name (falling back to its node name). Pure — the
/// testable core of the Tier-B diagnostic.
pub(crate) fn resolve_sink_holder(
    nodes: &[GraphNode],
    links: &[GraphLink],
    our_node_name: &str,
) -> Option<String> {
    let our = nodes
        .iter()
        .find(|n| n.node_name.as_deref() == Some(our_node_name))?;
    // The sink our stream links INTO.
    let sink_id = links
        .iter()
        .filter(|l| l.output_node == our.id)
        .find_map(|l| {
            nodes
                .iter()
                .find(|n| n.id == l.input_node && n.media_class.as_deref() == Some("Audio/Sink"))
        })?
        .id;
    // Another playback stream on that same sink → its app name.
    links
        .iter()
        .filter(|l| l.input_node == sink_id)
        .filter_map(|l| nodes.iter().find(|n| n.id == l.output_node))
        .find(|n| n.id != our.id && n.media_class.as_deref() == Some("Stream/Output/Audio"))
        .and_then(|n| n.app_name.clone().or_else(|| n.node_name.clone()))
}

/// Best-effort read of the REAL ALSA device playback clock rates from
/// `/proc/asound` — the device-layer "what" half of the honest bit-perfect
/// indicator (the "who" is [`resolve_sink_holder`]). Returns the distinct rates
/// of every currently-open playback PCM, sorted and deduped; an EMPTY vec when
/// none are open (idle / suspended / a Bluetooth sink — which has no ALSA
/// hardware PCM) or the tree can't be read.
///
/// The client stream rate only reports nokkvi's *request*, which lies when
/// PipeWire resamples a live down-switch; this reads the hardware `hw_params`
/// instead. Returning the full set (not a single guessed rate) lets the caller
/// pick the device's actual rate by preferring the track rate when it's present
/// — so a second app open on another card at a different rate can't mask a
/// genuine bit-perfect match (see the indicator's `resolve_device_rate`).
///
/// Lives in the data crate beside the graph probe — per the rule that
/// `data/src/audio/` owns iced-free device logic — so the UI doesn't scrape
/// `/proc/asound` itself and the two halves of the diagnostic stay co-located
/// and unit-testable.
pub fn active_alsa_playback_rates() -> Vec<u32> {
    let mut rates = std::collections::BTreeSet::new();
    let Ok(cards) = std::fs::read_dir("/proc/asound") else {
        return Vec::new();
    };
    for card in cards.flatten() {
        if !card.file_name().to_string_lossy().starts_with("card") {
            continue;
        }
        let Ok(pcms) = std::fs::read_dir(card.path()) else {
            continue;
        };
        for pcm in pcms.flatten() {
            let pcm_name = pcm.file_name();
            let pcm_name = pcm_name.to_string_lossy();
            // Playback PCMs are named "pcm<N>p" (capture is "...c").
            if !(pcm_name.starts_with("pcm") && pcm_name.ends_with('p')) {
                continue;
            }
            let Ok(subs) = std::fs::read_dir(pcm.path()) else {
                continue;
            };
            for sub in subs.flatten() {
                if !sub.file_name().to_string_lossy().starts_with("sub") {
                    continue;
                }
                let Ok(content) = std::fs::read_to_string(sub.path().join("hw_params")) else {
                    continue;
                };
                // Open PCMs list "rate: <N> (...)"; closed ones just say "closed".
                for line in content.lines() {
                    if let Some(rest) = line.strip_prefix("rate:")
                        && let Some(tok) = rest.split_whitespace().next()
                        && let Ok(r) = tok.parse::<u32>()
                    {
                        rates.insert(r);
                    }
                }
            }
        }
    }
    rates.into_iter().collect()
}

/// Run a short-lived PipeWire registry roundtrip and resolve who is holding the
/// sink nokkvi feeds. Best-effort: returns `None` on any failure. Runs its own
/// blocking mainloop — call it OFF the audio + UI threads.
///
/// NOTE: the FFI here is verifiable only against a live graph; the topology it
/// hands to [`resolve_sink_holder`] is what the unit tests cover.
#[cfg(target_os = "linux")]
pub fn probe_sink_holder(our_node_name: &str) -> Option<String> {
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };

    use pipewire as pw;
    use pw::types::ObjectType;

    let mainloop = pw::main_loop::MainLoopRc::new(None).ok()?;
    let context = pw::context::ContextRc::new(&mainloop, None).ok()?;
    let core = context.connect_rc(None).ok()?;
    let registry = core.get_registry().ok()?;

    let nodes: Rc<RefCell<Vec<GraphNode>>> = Rc::new(RefCell::new(Vec::new()));
    let links: Rc<RefCell<Vec<GraphLink>>> = Rc::new(RefCell::new(Vec::new()));

    let nodes_cb = nodes.clone();
    let links_cb = links.clone();
    let _reg_listener = registry
        .add_listener_local()
        .global(move |global| match global.type_ {
            ObjectType::Node => {
                if let Some(props) = global.props {
                    nodes_cb.borrow_mut().push(GraphNode {
                        id: global.id,
                        app_name: props.get("application.name").map(str::to_owned),
                        media_class: props.get("media.class").map(str::to_owned),
                        node_name: props.get("node.name").map(str::to_owned),
                    });
                }
            }
            ObjectType::Link => {
                if let Some(props) = global.props
                    && let (Some(out), Some(inp)) =
                        (props.get("link.output.node"), props.get("link.input.node"))
                    && let (Ok(o), Ok(i)) = (out.parse::<u32>(), inp.parse::<u32>())
                {
                    links_cb.borrow_mut().push(GraphLink {
                        output_node: o,
                        input_node: i,
                    });
                }
            }
            _ => {}
        })
        .register();

    // Roundtrip: quit the loop once the initial enumeration is done.
    let done = Rc::new(Cell::new(false));
    let done_cb = done.clone();
    let loop_cb = mainloop.clone();
    let pending = core.sync(0).ok()?;
    let _core_listener = core
        .add_listener_local()
        .done(move |id, seq| {
            if id == pw::core::PW_ID_CORE && seq == pending {
                done_cb.set(true);
                loop_cb.quit();
            }
        })
        .register();

    // Hard deadline on the roundtrip. `mainloop.run()` polls with an infinite
    // timeout and only returns once `quit()` is called — so if the daemon never
    // delivers the matching `done` (a restart / socket drop in the window
    // between `core.sync(0)` and its reply), without this the call would block
    // this `spawn_blocking` thread FOREVER. Since the probe re-fires ~1×/s while
    // resampled, accumulated stuck threads would eventually exhaust the blocking
    // pool. The one-shot timer sets `done` AND quits, so the `while !done.get()`
    // exits (a bare `quit()` would let the loop re-enter `run()` and block again)
    // and the thread is always reclaimed within the deadline.
    let done_timeout = done.clone();
    let timeout_loop = mainloop.clone();
    let _timer = mainloop.loop_().add_timer(move |_| {
        done_timeout.set(true);
        timeout_loop.quit();
    });
    let _ = _timer.update_timer(Some(std::time::Duration::from_secs(2)), None);

    while !done.get() {
        mainloop.run();
    }

    let holder = resolve_sink_holder(&nodes.borrow(), &links.borrow(), our_node_name);
    drop(_reg_listener);
    drop(_core_listener);
    holder
}

/// Non-Linux stub: bit-perfect / the PipeWire graph is Linux-only, so there's
/// never a holder to name. Keeps the call site unconditional.
#[cfg(not(target_os = "linux"))]
pub fn probe_sink_holder(_our_node_name: &str) -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: u32, app: Option<&str>, class: &str, name: &str) -> GraphNode {
        GraphNode {
            id,
            app_name: app.map(str::to_owned),
            media_class: Some(class.to_owned()),
            node_name: Some(name.to_owned()),
        }
    }

    #[test]
    fn names_the_other_stream_on_our_sink() {
        // Our node (10) and Firefox (20) both link into the FiiO sink (99).
        let nodes = vec![
            node(10, None, "Stream/Output/Audio", "Nokkvi"),
            node(20, Some("Firefox"), "Stream/Output/Audio", "Firefox"),
            node(99, None, "Audio/Sink", "alsa_output.fiio"),
        ];
        let links = vec![
            GraphLink {
                output_node: 10,
                input_node: 99,
            },
            GraphLink {
                output_node: 20,
                input_node: 99,
            },
        ];
        assert_eq!(
            resolve_sink_holder(&nodes, &links, "Nokkvi").as_deref(),
            Some("Firefox")
        );
    }

    #[test]
    fn none_when_we_are_alone_on_the_sink() {
        let nodes = vec![
            node(10, None, "Stream/Output/Audio", "Nokkvi"),
            node(99, None, "Audio/Sink", "alsa_output.fiio"),
        ];
        let links = vec![GraphLink {
            output_node: 10,
            input_node: 99,
        }];
        assert_eq!(resolve_sink_holder(&nodes, &links, "Nokkvi"), None);
    }

    #[test]
    fn falls_back_to_node_name_when_app_name_missing() {
        let nodes = vec![
            node(10, None, "Stream/Output/Audio", "Nokkvi"),
            node(20, None, "Stream/Output/Audio", "noctalia-sound"),
            node(99, None, "Audio/Sink", "alsa_output.fiio"),
        ];
        let links = vec![
            GraphLink {
                output_node: 10,
                input_node: 99,
            },
            GraphLink {
                output_node: 20,
                input_node: 99,
            },
        ];
        assert_eq!(
            resolve_sink_holder(&nodes, &links, "Nokkvi").as_deref(),
            Some("noctalia-sound")
        );
    }

    #[test]
    fn ignores_streams_on_a_different_sink() {
        // Another stream (20) is on a DIFFERENT sink (98), not ours (99).
        let nodes = vec![
            node(10, None, "Stream/Output/Audio", "Nokkvi"),
            node(20, Some("Spotify"), "Stream/Output/Audio", "Spotify"),
            node(98, None, "Audio/Sink", "alsa_output.builtin"),
            node(99, None, "Audio/Sink", "alsa_output.fiio"),
        ];
        let links = vec![
            GraphLink {
                output_node: 10,
                input_node: 99,
            },
            GraphLink {
                output_node: 20,
                input_node: 98,
            },
        ];
        assert_eq!(resolve_sink_holder(&nodes, &links, "Nokkvi"), None);
    }

    #[test]
    fn none_when_our_node_absent() {
        let nodes = vec![node(99, None, "Audio/Sink", "alsa_output.fiio")];
        assert_eq!(resolve_sink_holder(&nodes, &[], "Nokkvi"), None);
    }

    /// Live smoke test against the REAL PipeWire graph — verifies the FFI
    /// roundtrip connects, enumerates, and returns without hanging/crashing.
    /// Ignored by default (needs a live PipeWire + a running Nokkvi). Run:
    /// `cargo test -p nokkvi-data live_probe_smoke -- --ignored --nocapture`.
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "needs a live PipeWire graph + running Nokkvi"]
    fn live_probe_smoke() {
        let holder = super::probe_sink_holder("Nokkvi");
        println!("live sink holder for 'Nokkvi': {holder:?}");
    }
}
