//! The panel's Graphics and This computer (C213): a configuration that can run on a graphics card is fitted to the
//! card's memory, and a computer with several cards, linked or not, is catered for.
//!
//! The cards are what the installed llama.cpp lists (`llama-server --list-devices`), with their memory. What a model
//! needs is read from its file (`quickai_core::gguf`): its layers, and the bytes a token of context takes. From the two,
//! QuickAI says where each length of context fits (the dots of the context grid), puts as many layers on the cards as
//! fit when the whole model does not (Auto), and draws a meter for each card and for the memory.
//!
//! Where it runs is the GPU layers field: all of them (999) is **Card**, some is **Card + processor**, none is
//! **Processor**. With more than one card, the tick boxes are `--device`, the split `--split-mode` and the share
//! `--tensor-split`.

use std::path::{Path, PathBuf};

use crossbeam_channel::Receiver;
use eframe::egui;
use quickai_core::gguf::ModelFacts;
use quickai_core::heuristics::{RAM_FRACTION, RAM_LIMIT};

use super::icons::Icon;
use super::theme;
use super::App;
use crate::devices::{Device, DeviceKind};
use crate::hardware::SpeedTier;

const MIB: u64 = 1024 * 1024;
/// What llama.cpp keeps free on each card when it fits a model to it (its `--fit-target`); QuickAI keeps the same.
const MARGIN: u64 = 1024 * MIB;
/// The room llama.cpp's work takes beside the model and its context.
const WORK: u64 = 512 * MIB;
/// The share of a model's layers on the cards at which a split is fast, and medium (below it, as the processor).
const SPLIT_FAST: f64 = 0.8;
const SPLIT_MEDIUM: f64 = 0.4;
/// All the layers, as the GPU layers field says it.
const ALL_LAYERS: i64 = 999;
/// The fields of the graphics.
const LAYERS: &str = "n_gpu_layers";
const DEVICES: &str = "devices";
const SPLIT: &str = "split_mode";
const SHARE: &str = "tensor_split";

/// What is known of the devices of the llama.cpp that is installed.
#[derive(Default)]
pub(super) enum Devices {
    #[default]
    Unknown,
    /// Asked of the program at the path.
    Looking(PathBuf, Receiver<Result<(Vec<Device>, bool), String>>),
    /// The program's answer: its devices, and whether its NVIDIA cards are linked.
    Known {
        binary: PathBuf,
        list: Vec<Device>,
        linked: bool,
    },
    Failed(PathBuf, String),
}

/// Where the model runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Place {
    Card,
    /// This many of its layers on the cards, the rest on the processor.
    Mixed(u64),
    Processor,
}

/// Where a length of context fits, for its dot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Fit {
    /// On the cards (or comfortably in the memory, with no card).
    Good,
    /// Partly in the computer's memory, so slower (or tightly in it, with no card).
    Partly,
    TooBig,
}

/// What the selected model needs: its weights, its layers and the bytes a token of context takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Need {
    pub weights: u64,
    pub layers: u64,
    pub per_token: u64,
}

impl Need {
    fn total(&self, tokens: u64) -> u64 {
        self.weights + self.per_token.saturating_mul(tokens) + WORK
    }
}

/// What a run takes of a card or of the memory: the model, its context, and llama.cpp's work.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Takes {
    pub model: u64,
    pub context: u64,
    pub work: u64,
}

impl Takes {
    fn sum(&self) -> u64 {
        self.model + self.context + self.work
    }
}

/// Where a length fits on cards with `room` bytes free (beyond the margin), and a computer of `ram` bytes; with no room
/// (no card, or the processor chosen), in the memory alone.
pub(super) fn fit_of(need: &Need, tokens: u64, room: Option<u64>, ram: u64) -> Fit {
    let total = need.total(tokens);
    let ram = ram as f64;
    match room {
        Some(room) => {
            if total <= room {
                Fit::Good
            } else if (total as f64) <= room as f64 + ram * RAM_LIMIT {
                Fit::Partly
            } else {
                Fit::TooBig
            }
        }
        None => {
            if (total as f64) <= ram * RAM_FRACTION {
                Fit::Good
            } else if (total as f64) <= ram * RAM_LIMIT {
                Fit::Partly
            } else {
                Fit::TooBig
            }
        }
    }
}

/// Where a length fits with Card + processor (C215): as many layers go on the cards as fit at that length (`auto_fit`)
/// and the rest in the memory, so it fits as the processor's do with the cards' room added. A split has room for more
/// than either alone; how fast it is, is the speed's to say.
pub(super) fn split_fit_of(need: &Need, tokens: u64, room: u64, ram: u64) -> Fit {
    let total = need.total(tokens) as f64;
    let (room, ram) = (room as f64, ram as f64);
    if total <= room + ram * RAM_FRACTION {
        Fit::Good
    } else if total <= room + ram * RAM_LIMIT {
        Fit::Partly
    } else {
        Fit::TooBig
    }
}

/// How many layers fit on cards with `room` bytes free, each taking its share of the weights and of the context.
pub(super) fn layers_that_fit(need: &Need, tokens: u64, room: u64) -> u64 {
    if need.layers == 0 {
        return 0;
    }
    let per_layer = (need.weights + need.per_token.saturating_mul(tokens)).div_ceil(need.layers);
    (room.saturating_sub(WORK) / per_layer.max(1)).min(need.layers)
}

/// The free room of `cards`, beyond what llama.cpp keeps free on each.
fn room_of(cards: &[Device]) -> u64 {
    cards
        .iter()
        .map(|c| c.free_bytes.saturating_sub(MARGIN))
        .sum()
}

/// The share of each of `cards` (proportions that add up to 1): as `share` says (`3,1`), else by their free memory.
fn shares(cards: &[Device], share: Option<&str>) -> Vec<f64> {
    let said: Vec<f64> = share
        .map(|s| s.split(',').filter_map(|n| n.trim().parse().ok()).collect())
        .unwrap_or_default();
    let raw: Vec<f64> = if said.len() == cards.len() && said.iter().sum::<f64>() > 0.0 {
        said
    } else {
        cards.iter().map(|c| c.free_bytes.max(1) as f64).collect()
    };
    let sum: f64 = raw.iter().sum();
    raw.iter().map(|r| r / sum).collect()
}

/// A number of bytes in GB, as the meters say it (`6.6`).
fn gb(bytes: u64) -> String {
    format!("{:.1}", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
}

impl App {
    /// Whether the cards are asked for: llama.cpp on this computer.
    pub(super) fn lists_devices(&self) -> bool {
        self.engine == crate::settings::DEFAULT_ENGINE
            && !self.in_companion
            && self.remote_in_use().is_none()
            && self.setup_in_use().is_none()
    }

    /// Asks the installed llama.cpp for its devices once (again when the program changes), and takes the answer when it
    /// comes: then the cards are settled and the model fitted to them.
    pub(super) fn poll_devices(&mut self, ctx: &egui::Context) {
        if !self.lists_devices() {
            return;
        }
        let binary = self.binary.clone();
        let answer = match &self.devices {
            Devices::Looking(asked, rx) if *asked == binary => match rx.try_recv() {
                Ok(answer) => Some(answer),
                Err(crossbeam_channel::TryRecvError::Empty) => return,
                Err(_) => Some(Err("the program gave no answer".to_string())),
            },
            Devices::Known { binary: asked, .. } | Devices::Failed(asked, _)
                if *asked == binary =>
            {
                return
            }
            _ => None,
        };
        if let Some(answer) = answer {
            self.devices = match answer {
                Ok((list, linked)) => Devices::Known {
                    binary,
                    list,
                    linked,
                },
                Err(why) => Devices::Failed(binary, why),
            };
            self.settle_devices();
            self.auto_fit();
            return;
        }
        if !binary.is_file() {
            return;
        }
        let (tx, rx) = crossbeam_channel::bounded(1);
        let asked = binary.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(crate::devices::list(&asked));
            ctx.request_repaint();
        });
        self.devices = Devices::Looking(binary, rx);
    }

    /// Look again: the devices are asked for anew (a driver was installed, a card was put in).
    fn look_again(&mut self) {
        self.devices = Devices::Unknown;
    }

    /// The graphics llama.cpp can use (not the processor under another name).
    pub(super) fn graphics_cards(&self) -> Vec<Device> {
        // an engine that finds its cards itself (Ollama) names them in its log when it starts (C213)
        if !self.lists_devices() {
            if self.remote_in_use().is_some() || self.setup_in_use().is_some() || self.in_companion
            {
                return Vec::new();
            }
            return self
                .engine_cards
                .iter()
                .filter(|d| d.is_graphics())
                .cloned()
                .collect();
        }
        match &self.devices {
            Devices::Known { list, .. } => {
                list.iter().filter(|d| d.is_graphics()).cloned().collect()
            }
            _ => Vec::new(),
        }
    }

    /// The cards the model goes on: those of the devices field, else every card.
    pub(super) fn ticked_cards(&self) -> Vec<Device> {
        let cards = self.graphics_cards();
        match self.values.text(DEVICES).filter(|t| !t.trim().is_empty()) {
            Some(ids) => {
                let ids: Vec<&str> = ids.split(',').map(str::trim).collect();
                let ticked: Vec<Device> = cards
                    .iter()
                    .filter(|c| ids.contains(&c.id.as_str()))
                    .cloned()
                    .collect();
                if ticked.is_empty() {
                    cards
                } else {
                    ticked
                }
            }
            None => cards,
        }
    }

    /// The devices field as the place and the cards want it: none for the processor (the GPU layers field says
    /// `--device none` then); built-in graphics left out beside a card of its own, unless they were ticked.
    pub(super) fn settle_devices(&mut self) {
        if self.engine_has_layers().is_none() || !matches!(self.devices, Devices::Known { .. }) {
            return;
        }
        let cards = self.graphics_cards();
        // names this build of llama.cpp does not list (another build was installed, a card was taken out) would stop it
        // from starting: they are left out, and the share with them
        if let Some(ids) = self.values.text(DEVICES).map(str::to_string) {
            let known: Vec<&str> = ids
                .split(',')
                .map(str::trim)
                .filter(|id| cards.iter().any(|c| c.id == *id))
                .collect();
            if known.len() != ids.split(',').count() {
                if known.is_empty() {
                    self.values.clear(DEVICES);
                } else {
                    self.values.set(DEVICES, known.join(","));
                }
                self.values.clear(SHARE);
            }
        }
        if cards.is_empty() {
            self.values.clear(SHARE);
            return;
        }
        if self.place() == Place::Processor {
            self.values.clear(DEVICES);
            return;
        }
        if self.values.text(DEVICES).is_none() {
            let own: Vec<&Device> = cards
                .iter()
                .filter(|c| c.kind != DeviceKind::Integrated)
                .collect();
            if !own.is_empty() && own.len() < cards.len() {
                let ids: Vec<&str> = own.iter().map(|c| c.id.as_str()).collect();
                self.values.set(DEVICES, ids.join(","));
            }
        }
    }

    /// Whether the engine has the GPU layers field (llama.cpp).
    fn engine_has_layers(&self) -> Option<()> {
        self.form()?.field(LAYERS).map(|_| ())
    }

    /// The facts of the selected model's file, read once for each file.
    pub(super) fn refresh_facts(&mut self) {
        let key = self
            .models
            .get(self.selected)
            .map(facts_key)
            .unwrap_or_default();
        if self.facts.as_ref().is_some_and(|(p, _)| *p == key) {
            return;
        }
        // a model an engine lists by name (Ollama's) is read from the weights in its store (C213)
        let path = match self.models.get(self.selected) {
            Some(m) if m.path.as_os_str().is_empty() => {
                self.weights_in_store(&m.filename).unwrap_or_default()
            }
            _ => key.clone(),
        };
        let facts = (path.is_file() && (is_gguf(&path) || key != path))
            .then(|| {
                std::fs::File::open(&path)
                    .and_then(|f| quickai_core::gguf::parse(std::io::BufReader::new(f)))
                    .ok()
            })
            .flatten();
        self.facts = Some((key, facts));
    }

    /// The weights of a model the engine lists by name, in the store it keeps them in: the one it is told to use, else
    /// the ones it uses of itself.
    fn weights_in_store(&self, name: &str) -> Option<PathBuf> {
        if !self.lists_models() {
            return None;
        }
        let form = self.form();
        let theirs = form
            .as_ref()
            .and_then(|f| f.field("use_default_store"))
            .and_then(|f| f.value.as_bool())
            .unwrap_or(false);
        let told = form
            .as_ref()
            .and_then(|f| f.field("models_dir"))
            .and_then(|f| f.value.as_str().map(PathBuf::from))
            .filter(|_| !theirs);
        let home = directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf());
        crate::model_store::ollama_stores(told.as_deref(), home.as_deref())
            .iter()
            .find_map(|store| crate::model_store::weights_in_store(store, name))
    }

    /// What the selected model's file says of it, when it was read.
    pub(super) fn model_facts(&self) -> Option<&ModelFacts> {
        let model = self.models.get(self.selected)?;
        match &self.facts {
            Some((key, Some(facts))) if *key == facts_key(model) => Some(facts),
            _ => None,
        }
    }

    /// Whether an engine that finds its devices itself said them, and none is a card (Ollama on a computer whose only
    /// device it can use is the processor).
    pub(super) fn engine_found_no_card(&self) -> bool {
        !self.lists_devices() && !self.engine_cards.is_empty() && self.graphics_cards().is_empty()
    }

    /// What the engine says it has loaded of the selected model, while it runs (`[models] loaded`, C213).
    pub(super) fn loaded_now(&self) -> Option<&crate::prims::engine_models::LoadedRow> {
        if !self.session.is_active() {
            return None;
        }
        let model = self.models.get(self.selected)?;
        self.loaded.as_ref().filter(|row| {
            row.id == model.filename
                || row.id.strip_suffix(":latest") == Some(model.filename.as_str())
        })
    }

    /// The share of the model on the cards, 0 to 1: llama.cpp's from its layers; for an engine that places a model
    /// itself (Ollama), what it says while the model runs, and before, the share that would fit on its cards.
    pub(super) fn card_share(&self) -> f64 {
        if self.engine_has_layers().is_some() {
            return match self.place() {
                Place::Card => 1.0,
                Place::Mixed(n) => self
                    .need()
                    .map_or(0.5, |need| n as f64 / need.layers.max(1) as f64),
                Place::Processor => 0.0,
            };
        }
        if !self.gpu_offload() {
            return 0.0;
        }
        if let Some(row) = self.loaded_now() {
            return row.card_share();
        }
        // the engine said what it found, and none of it is a card: the processor
        if self.engine_found_no_card() {
            return 0.0;
        }
        let cards = self.ticked_cards();
        match self.need().filter(|_| !cards.is_empty()) {
            Some(need) => {
                let tokens = self.context_tokens();
                let room = room_of(&cards);
                if need.total(tokens) <= room {
                    1.0
                } else {
                    layers_that_fit(&need, tokens, room) as f64 / need.layers.max(1) as f64
                }
            }
            None => 1.0,
        }
    }

    /// What the selected model needs, when its file says it.
    pub(super) fn need(&self) -> Option<Need> {
        let model = self.models.get(self.selected)?;
        let facts = self.model_facts()?;
        Some(Need {
            // a projector that is attached goes on the card with the weights (C213)
            weights: model.size_bytes + self.projector_bytes(),
            layers: facts.layers?,
            per_token: facts.kv_bytes_per_token?,
        })
    }

    /// The context in force, in tokens (4K when the engine decides it).
    pub(super) fn context_tokens(&self) -> u64 {
        self.values
            .int("ctx_size")
            .filter(|n| *n > 0)
            .unwrap_or(4096) as u64
    }

    /// Where the model runs, from the GPU layers field.
    pub(super) fn place(&self) -> Place {
        if self.engine_has_layers().is_none() && self.context_field().is_some() {
            let share = self.card_share();
            return if share >= 0.995 {
                Place::Card
            } else if share <= 0.005 {
                Place::Processor
            } else {
                let layers = self.need().map_or(100, |n| n.layers);
                Place::Mixed(
                    ((layers as f64 * share).round() as u64)
                        .clamp(1, layers.saturating_sub(1).max(1)),
                )
            };
        }
        let layers = self.values.int(LAYERS).unwrap_or(ALL_LAYERS);
        let of = self.need().map(|n| n.layers);
        if layers <= 0 {
            Place::Processor
        } else if layers >= ALL_LAYERS || of.is_some_and(|of| layers as u64 >= of) {
            Place::Card
        } else {
            Place::Mixed(layers as u64)
        }
    }

    /// The room on the cards the model goes on, when it goes on cards at all.
    fn card_room(&self) -> Option<u64> {
        let cards = self.ticked_cards();
        (!cards.is_empty() && self.place() != Place::Processor).then(|| room_of(&cards))
    }

    /// Where `tokens` of context fit with the model, as things are chosen: on the card whole (Card), split between
    /// the card and the memory (Card + processor), or in the memory (Processor); `None` when the model's file does
    /// not say.
    pub(super) fn fit(&self, tokens: u64) -> Option<Fit> {
        let need = self.need()?;
        Some(match (self.place(), self.card_room()) {
            (Place::Mixed(_), Some(room)) => {
                split_fit_of(&need, tokens, room, self.total_ram_bytes)
            }
            (_, room) => fit_of(&need, tokens, room, self.total_ram_bytes),
        })
    }

    /// Where `tokens` fit with the whole model on the cards when it goes on cards at all: what the suggested length
    /// goes by, so that it is the longest that runs fast.
    pub(super) fn fit_on_cards(&self, tokens: u64) -> Option<Fit> {
        let need = self.need()?;
        Some(fit_of(
            &need,
            tokens,
            self.card_room(),
            self.total_ram_bytes,
        ))
    }

    /// With Card + processor, how many of the model's layers fit on the cards at `tokens`, and of how many.
    pub(super) fn split_layers_at(&self, tokens: u64) -> Option<(u64, u64)> {
        let need = self.need()?;
        let room = self.card_room()?;
        Some((layers_that_fit(&need, tokens, room), need.layers))
    }

    /// Auto: puts the whole model on the cards when it fits there, else as many of its layers as fit, else none; for a
    /// model just picked, a configuration just chosen, a length of context just chosen, and the cards just known.
    /// Nothing is changed once the person chose where it runs, or while it runs.
    pub(super) fn auto_fit(&mut self) {
        if self.session.is_active() || self.engine_has_layers().is_none() {
            return;
        }
        if !self.gpu_auto {
            // Card + processor, chosen: as many layers on the cards as fit at the length now (C215). The ones there before
            // may not fit a longer one, and a shorter one leaves room for more.
            if let Place::Mixed(n) = self.place() {
                if self.need().is_some() && !self.ticked_cards().is_empty() {
                    let fit = self.mixed_layers();
                    if fit != n {
                        self.values.set(LAYERS, fit as i64);
                        self.settle_devices();
                    }
                }
            }
            return;
        }
        let cards = self.ticked_cards();
        let Some(need) = self.need().filter(|_| !cards.is_empty()) else {
            return;
        };
        let tokens = self.context_tokens();
        let room = room_of(&cards);
        let layers = if need.total(tokens) <= room {
            ALL_LAYERS
        } else {
            layers_that_fit(&need, tokens, room) as i64
        };
        let now = self.values.int(LAYERS).unwrap_or(ALL_LAYERS);
        let same = now == layers || (now >= need.layers as i64 && layers == ALL_LAYERS);
        if !same {
            self.values.set(LAYERS, layers);
        }
        self.settle_devices();
    }

    /// The person chose where it runs: Auto is off until another model is picked.
    pub(super) fn choose_place(&mut self, place: Place) {
        self.gpu_auto = false;
        let layers = match place {
            Place::Card => ALL_LAYERS,
            Place::Processor => 0,
            Place::Mixed(n) => n as i64,
        };
        self.values.set(LAYERS, layers);
        self.settle_devices();
    }

    /// The layers Card + processor starts with: as many as fit, at least one and not all.
    fn mixed_layers(&self) -> u64 {
        match self.need() {
            Some(need) => {
                let room = room_of(&self.ticked_cards());
                layers_that_fit(&need, self.context_tokens(), room)
                    .clamp(1, need.layers.saturating_sub(1).max(1))
            }
            None => 20,
        }
    }

    /// What the run takes of each card it goes on, and of the memory.
    pub(super) fn takes(&self) -> Option<(Vec<(Device, Takes)>, Takes)> {
        let model = self.models.get(self.selected)?;
        let tokens = self.context_tokens();
        // the projector, when it is attached, is counted with the model
        let weights = model.size_bytes + self.projector_bytes();
        // what a loaded model takes in all is what the engine says (Ollama's /api/ps): the rest of it is its context
        let context = match self.loaded_now() {
            Some(row) => row.size_bytes.saturating_sub(weights + WORK),
            None => self
                .model_facts()
                .and_then(|f| f.context_bytes(tokens))
                .unwrap_or(0),
        };
        let cards = self.ticked_cards();
        let share = if cards.is_empty() {
            0.0
        } else {
            self.card_share()
        };
        let on_cards = Takes {
            model: (weights as f64 * share) as u64,
            context: (context as f64 * share) as u64,
            work: if share > 0.0 { WORK } else { 0 },
        };
        let parts = shares(&cards, self.values.text(SHARE));
        let per_card = if share > 0.0 {
            cards
                .iter()
                .zip(parts)
                .enumerate()
                .map(|(i, (card, part))| {
                    (
                        card.clone(),
                        Takes {
                            model: (on_cards.model as f64 * part) as u64,
                            context: (on_cards.context as f64 * part) as u64,
                            // the work is the first card's
                            work: if i == 0 { on_cards.work } else { 0 },
                        },
                    )
                })
                .collect()
        } else {
            Vec::new()
        };
        let memory = Takes {
            model: weights - on_cards.model.min(weights),
            context: context - on_cards.context.min(context),
            work: if share > 0.0 { 0 } else { WORK },
        };
        Some((per_card, memory))
    }

    /// How fast it should be: on the cards whole, fast; split, by the share of its layers on the cards (C215: a token
    /// reads every layer, and a card reads its memory several times faster than the processor reads the computer's, so
    /// four fifths on the card is still fast, and under two fifths little faster than the processor alone); on the
    /// processor, as its size and the memory say.
    pub(super) fn speed(&self) -> Option<SpeedTier> {
        let model = self.models.get(self.selected)?;
        let known = (self.lists_devices() && matches!(self.devices, Devices::Known { .. }))
            || (!self.lists_devices() && !self.graphics_cards().is_empty());
        if known {
            let cards = self.ticked_cards();
            return Some(match self.place() {
                Place::Card if !cards.is_empty() => match self.fit(self.context_tokens()) {
                    Some(Fit::Good) | None => SpeedTier::Fast,
                    _ => SpeedTier::Medium,
                },
                Place::Mixed(n) if !cards.is_empty() => {
                    let share = self
                        .need()
                        .map_or(0.5, |need| n as f64 / need.layers.max(1) as f64);
                    if share >= SPLIT_FAST {
                        SpeedTier::Fast
                    } else if share >= SPLIT_MEDIUM {
                        SpeedTier::Medium
                    } else {
                        crate::hardware::speed_tier(model.size_bytes, self.total_ram_bytes, false)
                    }
                }
                _ => crate::hardware::speed_tier(model.size_bytes, self.total_ram_bytes, false),
            });
        }
        let offload = self.speed_basis()?;
        Some(crate::hardware::speed_tier(
            model.size_bytes,
            self.total_ram_bytes,
            offload,
        ))
    }

    /// Where it runs, in words, for the line of the run (`on the RTX 4090`, `38 of 62 layers on the card`).
    pub(super) fn place_words(&self) -> Option<String> {
        if !self.lists_devices() {
            return self.engine_place_words();
        }
        if !matches!(self.devices, Devices::Known { .. }) {
            return None;
        }
        let cards = self.ticked_cards();
        let named = match cards.as_slice() {
            [one] => format!("the {}", one.name),
            [] => return Some("on the processor".to_string()),
            many => format!("{} cards", many.len()),
        };
        Some(match self.place() {
            Place::Card => format!("on {named}"),
            Place::Mixed(n) => match self.need() {
                Some(need) => format!("{n} of {} layers on {named}", need.layers),
                None => format!("{n} layers on {named}"),
            },
            Place::Processor => "on the processor".to_string(),
        })
    }

    /// Where an engine that places a model itself (Ollama) runs it, in words: what it says while the model runs (`3.1 of
    /// 4.4 GB on the Radeon RX 6700 XT, the rest in memory`), and before, what would fit on the cards it named.
    fn engine_place_words(&self) -> Option<String> {
        self.context_field()?;
        if !self.gpu_offload() {
            return Some("on the processor".to_string());
        }
        let cards = self.ticked_cards();
        let named = match cards.as_slice() {
            [one] => format!("the {}", one.name),
            [] => "the graphics card".to_string(),
            many => format!("{} cards", many.len()),
        };
        if let Some(row) = self.loaded_now() {
            let share = row.card_share();
            return Some(if share >= 0.995 {
                format!("on {named}")
            } else if share <= 0.005 {
                "on the processor".to_string()
            } else {
                format!(
                    "{} of {} GB on {named}, the rest in memory",
                    gb(row.on_cards),
                    gb(row.size_bytes)
                )
            });
        }
        if self.engine_found_no_card() {
            return Some("on the processor".to_string());
        }
        if cards.is_empty() {
            return None;
        }
        let share = self.card_share();
        Some(if share >= 0.995 {
            format!("on {named}")
        } else {
            format!("about {:.0}% on {named}, the rest in memory", share * 100.0)
        })
    }

    // ---- what is drawn ------------------------------------------------------------------------------------

    /// Graphics: where the model runs, and with more than one card, which cards and how it is split between them.
    pub(super) fn show_graphics_section(&mut self, ui: &mut egui::Ui) {
        let enabled = !self.session.is_active();
        if self.engine_has_layers().is_some() && self.lists_devices() {
            self.show_llama_graphics(ui, enabled);
        } else if let Some(field) = self.form().and_then(|f| f.field("gpu").cloned()) {
            // Ollama: its own say of the cards, on or off; the cards it named, and how much of the model it put on them
            let on = field.value.as_str() != Some("off");
            let cards = self.graphics_cards();
            let head = match cards.as_slice() {
                [] if self.engine_found_no_card() => "no graphics card found".to_string(),
                [] => "Ollama decides how".to_string(),
                [one] => format!("{} · {} GB", one.name, gb(one.total_bytes)),
                many => format!("{} cards", many.len()),
            };
            super::inspector::section_head(
                ui,
                Icon::Chip,
                "Graphics",
                Some(
                    "Ollama puts as much of the model on the graphics cards it finds as fits, and the rest on the \
                     processor; or all of it on the processor if you turn them off. The cards are the ones it named when \
                     it last started.",
                ),
                |ui| {
                    ui.add(egui::Label::new(theme::muted(head).small()).truncate());
                },
            );
            let chosen = ui
                .add_enabled_ui(enabled, |ui| {
                    segments(
                        ui,
                        &[("Graphics", 1.0, true), ("Processor", 1.0, true)],
                        Some(if on { 0 } else { 1 }),
                    )
                })
                .inner;
            match chosen {
                Some(0) => self.values.set("gpu", "auto"),
                Some(1) => self.values.set("gpu", "off"),
                _ => {}
            }
            // how it is split, said: what Ollama says while it runs, else what would fit
            if on {
                let said = match (self.loaded_now(), self.engine_place_words()) {
                    (Some(_), Some(words)) => Some(format!("Now {words}.")),
                    (None, Some(words)) if words.starts_with("about") => {
                        Some(format!("It would run {words}: Ollama decides at Start."))
                    }
                    _ => None,
                };
                if let Some(said) = said {
                    ui.add_space(4.0);
                    ui.label(theme::muted(said).small());
                }
            }
        }
    }

    fn show_llama_graphics(&mut self, ui: &mut egui::Ui, enabled: bool) {
        let p = theme::palette();
        let cards = self.graphics_cards();
        let ticked = self.ticked_cards();
        let (looking, failed, linked) = match &self.devices {
            Devices::Known { linked, .. } => (false, None, *linked),
            Devices::Failed(_, why) => (false, Some(why.clone()), false),
            _ => (true, None, false),
        };
        let mut again = false;
        super::inspector::section_head(
            ui,
            Icon::Chip,
            "Graphics",
            Some(
                "Where the model runs. On a graphics card it is fastest; when the whole model does not fit on the \
                 card, part of it can run on the processor. QuickAI fits it to the card for each model, until you \
                 choose here.",
            ),
            |ui| match cards.as_slice() {
                _ if looking => {
                    ui.label(theme::muted("looking…").small());
                }
                [] => {
                    again = ui.link(egui::RichText::new("Look again").small()).clicked();
                    let said = ui.label(theme::muted("none found ·").small());
                    if let Some(why) = &failed {
                        said.on_hover_text(why);
                    }
                }
                [one] => {
                    ui.add(
                        egui::Label::new(theme::muted(format!("{} · {} GB", one.name, gb(one.total_bytes))).small())
                            .truncate(),
                    );
                }
                _ if linked => {
                    theme::chip(ui, "linked", Some(theme::Tone::Good))
                        .on_hover_text("The cards are linked with NVLink: split by rows is faster over the link.");
                }
                many => {
                    ui.label(theme::muted(format!("{} found", many.len())).small());
                }
            },
        );
        if again {
            self.look_again();
        }
        // with more than one, a tick box for each card
        if cards.len() > 1 {
            let mut toggled: Option<String> = None;
            for card in &cards {
                let on = ticked.iter().any(|t| t.id == card.id);
                // the last card ticked stays ticked: none would be the processor, which has its own choice
                let can = enabled && !(on && ticked.len() == 1);
                // its memory from the right, the tick box and its name in what is left (shortened when long)
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(theme::muted(format!("{} GB", gb(card.total_bytes))).small());
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            let mut tick = on;
                            let label = match card.kind {
                                DeviceKind::Integrated => format!("{} (built in)", card.name),
                                _ => card.name.clone(),
                            };
                            let boxed =
                                ui.add_enabled(can, egui::Checkbox::without_text(&mut tick));
                            let named = ui
                                .add_enabled(
                                    can,
                                    egui::Label::new(&label)
                                        .truncate()
                                        .sense(egui::Sense::click()),
                                )
                                .on_hover_text(format!("{label} ({})", card.id));
                            if boxed.changed() || named.clicked() {
                                toggled = Some(card.id.clone());
                            }
                        });
                    });
                });
            }
            if let Some(id) = toggled {
                let mut ids: Vec<String> = ticked.iter().map(|c| c.id.clone()).collect();
                if let Some(at) = ids.iter().position(|i| *i == id) {
                    ids.remove(at);
                } else {
                    ids.push(id);
                }
                // in the order llama.cpp lists them
                ids.sort_by_key(|i| cards.iter().position(|c| c.id == *i));
                if ids.len() == cards.len() {
                    self.values.clear(DEVICES);
                } else {
                    self.values.set(DEVICES, ids.join(","));
                }
                self.values.clear(SHARE);
                self.auto_fit();
            }
            ui.add_space(4.0);
        }
        let place = self.place();
        let need = self.need();
        let whole_fits = need
            .map(|n| n.weights + WORK <= room_of(&ticked))
            .unwrap_or(true);
        let have = !ticked.is_empty();
        let many = ticked.len() > 1;
        let lit = match place {
            _ if !have && !looking => 2,
            Place::Card => 0,
            Place::Mixed(_) => 1,
            Place::Processor => 2,
        };
        let cells = [
            (if many { "Cards" } else { "Card" }, 1.0, have && whole_fits),
            (
                if many {
                    "Cards + processor"
                } else {
                    "Card + processor"
                },
                1.7,
                have && need.is_some(),
            ),
            ("Processor", 1.1, have),
        ];
        let chosen = ui
            .add_enabled_ui(enabled, |ui| segments(ui, &cells, Some(lit)))
            .inner;
        match chosen {
            Some(0) => self.choose_place(Place::Card),
            Some(1) if !matches!(place, Place::Mixed(_)) => {
                let n = self.mixed_layers();
                self.choose_place(Place::Mixed(n));
            }
            Some(2) => self.choose_place(Place::Processor),
            _ => {}
        }
        // part on the cards: how many layers, dragged
        if let (Place::Mixed(n), Some(need)) = (self.place(), need) {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(theme::muted("Layers on the card").small());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.gpu_auto {
                        theme::chip(ui, "Auto", Some(theme::Tone::Good)).on_hover_text(
                            "As many as fit on the card, worked out for each model.",
                        );
                    }
                    ui.label(
                        egui::RichText::new(format!("{n} of {}", need.layers))
                            .small()
                            .color(p.body),
                    );
                });
            });
            if let Some(share) = drag_bar(ui, n as f32 / need.layers.max(1) as f32, p.warn, enabled)
            {
                let n = ((share * need.layers as f32).round() as u64)
                    .clamp(1, need.layers.saturating_sub(1).max(1));
                self.choose_place(Place::Mixed(n));
            }
        }
        // more than one card: how it is split, and each card's share
        if many && self.place() != Place::Processor {
            ui.add_space(8.0);
            ui.label(theme::muted("Split").small());
            let rows = self.values.text(SPLIT) == Some("row");
            let chosen = ui
                .add_enabled_ui(enabled, |ui| {
                    segments(
                        ui,
                        &[("By layers", 1.0, true), ("By rows", 1.0, true)],
                        Some(usize::from(rows)),
                    )
                })
                .inner
                .map(|i| if i == 1 { "row" } else { "layer" });
            if let Some(mode) = chosen {
                if mode == "layer" {
                    self.values.clear(SPLIT);
                } else {
                    self.values.set(SPLIT, mode);
                }
            }
            if ticked.len() == 2 {
                let parts = shares(&ticked, self.values.text(SHARE));
                let auto = self.values.text(SHARE).is_none();
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(theme::muted(&ticked[0].name).small());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(theme::muted(&ticked[1].name).small());
                        if auto {
                            theme::chip(ui, "Auto", Some(theme::Tone::Good))
                                .on_hover_text("In the measure of the cards' free memory.");
                        } else if ui
                            .add_enabled(
                                enabled,
                                egui::Link::new(egui::RichText::new("Auto").small()),
                            )
                            .on_hover_text(
                                "Share it in the measure of the cards' free memory again.",
                            )
                            .clicked()
                        {
                            self.values.clear(SHARE);
                        }
                    });
                });
                if let Some(share) = drag_bar(ui, parts[0] as f32, p.accent_text, enabled) {
                    let first = (share * 100.0).round().clamp(5.0, 95.0) as u32;
                    self.values.set(SHARE, format!("{first},{}", 100 - first));
                }
            }
        }
    }

    /// This computer, at the foot of the panel: how fast it should be, and a meter for each card the model goes on and
    /// for the memory, each with the model, its context and llama.cpp's work.
    /// The meters take at most `meters` high, and scroll beyond it (many cards, a short window).
    pub(super) fn show_this_computer(&mut self, ui: &mut egui::Ui, meters: f32) {
        let p = theme::palette();
        let speed = self.speed();
        super::inspector::section_head(
            ui,
            Icon::Computer,
            "This computer",
            Some(
                "What the model and its context take of each graphics card and of the memory, as its file says, and \
                 how fast it should answer here.",
            ),
            |ui| {
                if let Some(tier) = speed {
                    let color = match tier {
                        SpeedTier::Fast => p.accent_text,
                        SpeedTier::Slow => p.warn,
                        SpeedTier::Medium => p.warn,
                    };
                    ui.label(egui::RichText::new(tier.label()).color(color));
                }
            },
        );
        let Some((cards, memory)) = self.takes() else {
            return;
        };
        // with many cards the meters scroll, so that the foot leaves the panel its room
        egui::ScrollArea::vertical()
            .id_salt("computer_meters")
            .max_height(meters)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for (card, takes) in &cards {
                    meter_row(ui, &card.name, takes, card.total_bytes);
                }
                meter_row(ui, "Memory", &memory, self.total_ram_bytes);
            });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            for (word, color) in [
                ("model", p.accent_text),
                ("context", context_color()),
                ("other", work_color()),
            ] {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                ui.painter().rect_filled(rect, 2.0, color);
                ui.label(egui::RichText::new(word).size(11.0).color(p.faint));
                ui.add_space(4.0);
            }
        });
    }
}

/// Whether a path is a GGUF file by its name.
fn is_gguf(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("gguf"))
}

fn context_color() -> egui::Color32 {
    let p = theme::palette();
    p.accent_text.lerp_to_gamma(p.raised, 0.5)
}

fn work_color() -> egui::Color32 {
    let p = theme::palette();
    p.accent_text.lerp_to_gamma(p.raised, 0.75)
}

/// A meter: the name and `used of total GB` over a bar of the model, its context and the work.
fn meter_row(ui: &mut egui::Ui, name: &str, takes: &Takes, total: u64) {
    let p = theme::palette();
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(format!("{} of {} GB", gb(takes.sum()), gb(total)))
                    .small()
                    .color(p.body),
            );
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.add(
                    egui::Label::new(egui::RichText::new(name).small().color(p.muted)).truncate(),
                )
                .on_hover_text(name);
            });
        });
    });
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 7.0), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 3.5, p.raised);
    let total = total.max(1) as f32;
    let mut x = rect.left();
    for (bytes, color) in [
        (takes.model, p.accent_text),
        (takes.context, context_color()),
        (takes.work, work_color()),
    ] {
        let w = (bytes as f32 / total * width).min(rect.right() - x);
        if w <= 0.0 {
            continue;
        }
        let part = egui::Rect::from_min_size(
            egui::pos2(x, rect.top()),
            egui::vec2(w.max(2.0), rect.height()),
        );
        painter.rect_filled(part, 3.5, color);
        x += w;
    }
}

/// A bar filled as far as `share` (0 to 1) of it, with a knob there that can be dragged; the share it was dragged to
/// comes back.
fn drag_bar(ui: &mut egui::Ui, share: f32, rest: egui::Color32, enabled: bool) -> Option<f32> {
    let p = theme::palette();
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(width, 16.0),
        if enabled {
            egui::Sense::click_and_drag()
        } else {
            egui::Sense::hover()
        },
    );
    let response = response.on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
    let bar = egui::Rect::from_center_size(rect.center(), egui::vec2(width, 10.0));
    let share = share.clamp(0.0, 1.0);
    let at = bar.left() + bar.width() * share;
    let painter = ui.painter();
    painter.rect_filled(bar, 5.0, rest.lerp_to_gamma(p.raised, 0.35));
    painter.rect_filled(
        egui::Rect::from_min_max(bar.min, egui::pos2(at, bar.bottom())),
        5.0,
        p.accent_text,
    );
    painter.rect_filled(
        egui::Rect::from_center_size(egui::pos2(at, rect.center().y), egui::vec2(4.0, 16.0)),
        2.0,
        if response.hovered() || response.dragged() {
            p.text
        } else {
            p.body
        },
    );
    if response.dragged() || response.clicked() {
        let x = response.interact_pointer_pos()?.x;
        return Some(((x - bar.left()) / bar.width()).clamp(0.0, 1.0));
    }
    None
}

/// A row of choices in one box, as the panel draws them: `(word, how wide, enabled)` each, the lit one in the accent.
/// What was clicked comes back.
pub(super) fn segments(
    ui: &mut egui::Ui,
    cells: &[(&str, f32, bool)],
    lit: Option<usize>,
) -> Option<usize> {
    let p = theme::palette();
    let enabled = ui.is_enabled();
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 36.0), egui::Sense::hover());
    let painter = ui.painter().clone();
    painter.rect(
        rect,
        9.0,
        p.card,
        egui::Stroke::new(1.0, p.line),
        egui::StrokeKind::Inside,
    );
    let inner = rect.shrink(3.0);
    let gap = 3.0;
    let weights: f32 = cells.iter().map(|c| c.1).sum::<f32>().max(0.1);
    let room = inner.width() - gap * (cells.len().saturating_sub(1)) as f32;
    let mut x = inner.left();
    let mut chosen = None;
    for (i, (word, weight, can)) in cells.iter().enumerate() {
        let cell = egui::Rect::from_min_size(
            egui::pos2(x, inner.top()),
            egui::vec2(room * weight / weights, inner.height()),
        );
        x = cell.right() + gap;
        let on = lit == Some(i);
        let active = enabled && *can;
        let sense = if active && !on {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        };
        let response = ui.interact(cell, ui.id().with(("segment", word)), sense);
        if on {
            painter.rect(
                cell,
                6.0,
                p.accent_soft,
                egui::Stroke::new(1.0, p.accent_text.gamma_multiply(0.45)),
                egui::StrokeKind::Inside,
            );
        } else if active && response.hovered() {
            painter.rect_filled(cell, 6.0, p.raised_hover.gamma_multiply(0.6));
        }
        let color = match (on, active) {
            (true, _) => p.accent_text,
            (false, true) => p.muted,
            (false, false) => p.faint,
        };
        let mut font = egui::TextStyle::Body.resolve(ui.style());
        font.size = 13.0;
        if on {
            font.family = theme::semibold(ui);
        }
        // words wider than their part (a narrow panel) are drawn smaller, down to 10 points
        let mut galley = painter.layout_no_wrap(word.to_string(), font.clone(), color);
        let room = cell.width() - 8.0;
        if galley.size().x > room {
            font.size = (font.size * room / galley.size().x).max(10.0);
            galley = painter.layout_no_wrap(word.to_string(), font, color);
        }
        painter.galley(cell.center() - galley.size() / 2.0, galley, color);
        if response.clicked() {
            chosen = Some(i);
        }
    }
    chosen
}

/// What the facts of a model are kept under: its file, or for a model an engine lists by name, its name.
fn facts_key(model: &crate::models::ModelEntry) -> PathBuf {
    if model.path.as_os_str().is_empty() {
        PathBuf::from(format!("\u{0}listed/{}", model.filename))
    } else {
        model.path.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GB: u64 = 1024 * 1024 * 1024;

    fn card(id: &str, total_gb: u64, free_gb: u64) -> Device {
        Device {
            id: id.to_string(),
            name: id.to_string(),
            total_bytes: total_gb * GB,
            free_bytes: free_gb * GB,
            kind: DeviceKind::Discrete,
        }
    }

    #[test]
    fn a_length_fits_on_the_card_partly_in_memory_or_not_at_all() {
        // 2.5 GB of weights, 112 KB a token: 32K takes 3.5 GB of context
        let need = Need {
            weights: 5 * GB / 2,
            layers: 28,
            per_token: 112 * 1024,
        };
        let room = room_of(&[card("CUDA0", 12, 12)]);
        assert_eq!(fit_of(&need, 32768, Some(room), 60 * GB), Fit::Good);
        assert_eq!(fit_of(&need, 131072, Some(room), 60 * GB), Fit::Partly);
        assert_eq!(fit_of(&need, 1 << 22, Some(room), 8 * GB), Fit::TooBig);
        // with no card: comfortably in half of the memory, tightly in four fifths
        assert_eq!(fit_of(&need, 32768, None, 16 * GB), Fit::Good);
        assert_eq!(fit_of(&need, 65536, None, 16 * GB), Fit::Partly);
    }

    #[test]
    fn as_many_layers_go_on_the_card_as_fit_with_their_share_of_the_context() {
        // 16 GB of weights in 62 layers, little context, on a card with 11 GB free beyond the margin
        let need = Need {
            weights: 16 * GB,
            layers: 62,
            per_token: 0,
        };
        let n = layers_that_fit(&need, 0, 11 * GB);
        // (11 GB - the work) / (16 GB / 62)
        assert_eq!(n, 40);
        assert_eq!(layers_that_fit(&need, 0, 100 * GB), 62);
        assert_eq!(layers_that_fit(&need, 0, 0), 0);
    }

    #[test]
    fn cards_share_by_their_free_memory_unless_a_share_is_said() {
        let cards = [card("CUDA0", 24, 20), card("CUDA1", 12, 10)];
        let by_free = shares(&cards, None);
        assert!((by_free[0] - 2.0 / 3.0).abs() < 1e-9);
        let said = shares(&cards, Some("3,1"));
        assert!((said[0] - 0.75).abs() < 1e-9);
        // a share that does not name each card is not taken
        let odd = shares(&cards, Some("1"));
        assert!((odd[0] - 2.0 / 3.0).abs() < 1e-9);
    }
}
