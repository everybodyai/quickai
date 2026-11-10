//! Where an engine that places a model itself puts it (C213). llama.cpp is told
//! where to run a model; Ollama decides it, so the panel learns it from Ollama: the cards it names in its log when it
//! starts (`[devices]` of its document), and while a model runs, how much of it is on them (`[models] loaded`, its
//! `/api/ps`, asked every few seconds). Graphics, This computer and the chips of a run say what it said.

use std::time::{Duration, Instant};

use eframe::egui;

use super::App;
use crate::events::ServerState;

/// When a running engine is asked what it has loaded, after a model loads: soon, to say where it went, then seldom
/// (each question is a line of the engine's log, which says what the engine said word for word).
fn ask_after(asked: u32) -> Duration {
    Duration::from_secs(match asked {
        0 => 1,
        1 => 5,
        2 => 20,
        _ => 60,
    })
}

impl App {
    /// A line of the engine's log: a card it names, when its document says how it names them. The first card of a run
    /// says them anew (a card taken out, a driver that stopped working).
    pub(super) fn note_engine_card(&mut self, line: &str) {
        let Some(spec) = self.engine_doc().and_then(|e| e.devices.clone()) else {
            return;
        };
        let n = if self.cards_said_anew {
            0
        } else {
            self.engine_cards.len()
        };
        let Some(card) = crate::devices::from_log_line(&spec, line, n) else {
            return;
        };
        if self.cards_said_anew {
            self.engine_cards.clear();
            self.cards_said_anew = false;
        }
        if !self
            .engine_cards
            .iter()
            .any(|c| c.id == card.id && c.name == card.name)
        {
            self.engine_cards.push(card);
        }
    }

    /// The engine changed: what another one said of its cards and of what it loaded is not this one's.
    pub(super) fn forget_engine_place(&mut self) {
        self.engine_cards.clear();
        self.cards_said_anew = true;
        self.loaded = None;
        self.loaded_asked = None;
        self.loaded_asks = None;
    }

    /// While the model runs, the engine is asked what it has loaded every few seconds, when its document says how
    /// (`[models] loaded`); the answer is taken when it comes. Stopped, the last answer is forgotten, and the next run's
    /// first card says the cards anew.
    pub(super) fn poll_loaded(&mut self, ctx: &egui::Context) {
        if !self.session.is_active() {
            self.loaded = None;
            self.loaded_asked = None;
            self.loaded_asks = None;
            self.cards_said_anew = true;
            return;
        }
        if self.session.state() != ServerState::Running {
            return;
        }
        // only while a model is loaded (Ollama runs with none until one is chosen); a model loaded anew is asked about soon
        let Some(model) = self.session.model_name().map(str::to_string) else {
            self.loaded = None;
            self.loaded_asks = None;
            return;
        };
        let asked = match &self.loaded_asks {
            Some((name, n)) if *name == model => *n,
            _ => {
                self.loaded = None;
                self.loaded_asked = None;
                self.loaded_asks = Some((model.clone(), 0));
                0
            }
        };
        let Some(call) = self
            .engine_doc()
            .and_then(|e| quickai_core::models::loaded_call(e, &self.host_facts))
            .and_then(Result::ok)
        else {
            return;
        };
        // the answer, when it has come
        if let Some((at, Some(rx))) = &self.loaded_asked {
            let at = *at;
            match rx.try_recv() {
                Ok(Ok(rows)) => {
                    let running = self.session.model_name().map(str::to_string);
                    self.loaded = rows
                        .iter()
                        .find(|row| {
                            running.as_deref().is_some_and(|name| {
                                row.id == name || row.id.strip_suffix(":latest") == Some(name)
                            })
                        })
                        .or(rows.first())
                        .cloned();
                    self.loaded_asked = Some((at, None));
                    self.loaded_asks = Some((model.clone(), asked + 1));
                }
                Ok(Err(_)) | Err(crossbeam_channel::TryRecvError::Disconnected) => {
                    self.loaded_asked = Some((at, None));
                }
                Err(crossbeam_channel::TryRecvError::Empty) => {
                    ctx.request_repaint_after(Duration::from_millis(300));
                    return;
                }
            }
        }
        let wait = ask_after(asked);
        let due = match &self.loaded_asked {
            None => true,
            Some((at, rx)) => rx.is_none() && at.elapsed() >= wait,
        };
        // asked again then: the window is drawn, though nothing else moves
        ctx.request_repaint_after(wait);
        if !due {
            return;
        }
        let Some(endpoint) = self.endpoint() else {
            return;
        };
        let (tx, rx) = crossbeam_channel::bounded(1);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let answer = crate::prims::engine_models::loaded_models(&endpoint, &call)
                .map_err(|e| e.to_string());
            let _ = tx.send(answer);
            ctx.request_repaint();
        });
        self.loaded_asked = Some((Instant::now(), Some(rx)));
    }
}

#[cfg(test)]
mod tests {
    use quickai_core::registry::engine::CompiledDevicesLog;

    use crate::config::{build_registry, ConfigDirs};
    use crate::devices::from_log_line;

    fn ollama_devices() -> CompiledDevicesLog {
        let registry = build_registry(&ConfigDirs::default());
        registry
            .engine("ollama")
            .and_then(|e| e.devices.clone())
            .expect("Ollama's document says how it names its cards")
    }

    /// C213: the cards Ollama names when it starts, as 0.12 and later say them and as 0.5 to 0.11 did; its processor is
    /// said too, and is no card.
    #[test]
    fn the_cards_ollama_names_in_its_log_are_read_and_its_processor_is_not_one() {
        let spec = ollama_devices();
        let new = r#"time=2026-10-06T09:20:26.402+02:00 level=INFO source=types.go:42 msg="inference compute" id=0 filter_id="" library=ROCm compute=gfx1031 name=ROCm0 description="AMD Radeon RX 6700 XT" libdirs=ollama,rocm driver=60342.13 pci_id=0000:03:00.0 type=discrete total="12.0 GiB" available="11.7 GiB""#;
        let card = from_log_line(&spec, new, 0).expect("a card");
        assert_eq!(card.name, "Radeon RX 6700 XT");
        assert_eq!(card.total_bytes, 12 * 1024 * 1024 * 1024);
        assert!(card.is_graphics());
        let old = r#"time=2025-03-01T10:00:00.000Z level=INFO source=types.go:130 msg="inference compute" id=GPU-4b1c library=cuda variant=v12 compute=8.9 driver=12.4 name="NVIDIA GeForce RTX 4090" total="23.6 GiB" available="23.2 GiB""#;
        let card = from_log_line(&spec, old, 1).expect("a card");
        assert_eq!(card.name, "RTX 4090");
        assert!(card.total_bytes > 23 * 1024 * 1024 * 1024);
        // recorded from Ollama 0.35.1 on a computer with no card: the processor is a device, and not graphics
        let cpu = r#"time=2026-11-10T10:47:22.657Z level=INFO source=types.go:50 msg="inference compute" id=cpu library=cpu compute="" name=cpu description=cpu libdirs=ollama driver="" pci_id="" type="" total="30.6 GiB" available="5.9 GiB""#;
        let processor = from_log_line(&spec, cpu, 0).expect("the processor is said");
        assert!(!processor.is_graphics(), "{processor:?}");
        assert_eq!(
            from_log_line(&spec, "level=INFO msg=\"Listening on 127.0.0.1:11434\"", 0),
            None
        );
    }
}
