//! The page of an engine that is not here yet: one card that names the version it installs and when it was published,
//! what comes with it (a box for each part that is the person's to choose, ComfyUI-Manager, and a tick for each that is
//! not), the room it needs and the button. While it installs, the same card lists its steps, the one under way with what
//! it is doing, under one bar and one Cancel.

use std::cmp::Ordering;
use std::path::Path;

use eframe::egui;
use quickai_core::acquire;

use super::icons::{self, Icon};
use super::runtime_manager::{display_root, INSTALL_PYTHON_FIELD};
use super::theme::{self, Role, Tone};
use super::App;
use crate::hardware::format_bytes;
use crate::prims::install::{self, InstallRequest, StepRecord};

/// How wide the card is, at most.
pub(super) const CARD_WIDTH: f32 = 640.0;

/// What the card says on its button that opens the list of the other versions.
pub(super) const OTHER_VERSIONS: &str = "Other versions…";

// ---- what the card says, as functions of what it is told -------------------------------------------------------------

/// `2026-11-11` as `11 November 2026`; `None` for what is not a date of that shape.
pub(super) fn long_date(iso: &str) -> Option<String> {
    const MONTHS: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    let mut parts = iso.trim().splitn(3, '-');
    let (year, month, day) = (parts.next()?, parts.next()?, parts.next()?);
    if year.len() != 4 {
        return None;
    }
    let year: u32 = year.parse().ok()?;
    let month: usize = month.parse().ok()?;
    let day: u32 = day.parse().ok()?;
    let month = MONTHS.get(month.checked_sub(1)?)?;
    (1..=31)
        .contains(&day)
        .then(|| format!("{day} {month} {year}"))
}

/// `the source of v0.45.2` as `The source of v0.45.2`.
pub(super) fn sentence_case(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// What the card says under the version's name: when it was published, where it comes from, and how it is checked
/// (`Released 11 November 2026 · built from its source at github.com/Comfy-Org/ComfyUI and checked against its sha256`).
pub(super) fn version_line(
    date: Option<&str>,
    repo: &str,
    from_source: bool,
    pinned: bool,
) -> String {
    let whence = match (from_source, repo.is_empty()) {
        (true, false) => format!("built from its source at {repo}"),
        (true, true) => "built from its source".to_string(),
        (false, false) => format!("downloaded from {repo}"),
        (false, true) => "downloaded".to_string(),
    };
    let whence = if pinned {
        format!("{whence} and checked against its sha256")
    } else {
        whence
    };
    match date.and_then(long_date) {
        Some(day) => format!("Released {day} · {whence}"),
        None => sentence_case(&whence),
    }
}

/// `30GiB` as `30 GiB`.
pub(super) fn spaced_size(size: &str) -> String {
    let size = size.trim();
    match size.find(|c: char| c.is_ascii_alphabetic()) {
        Some(at) if at > 0 && !size[..at].ends_with(' ') => {
            format!("{} {}", &size[..at], &size[at..])
        }
        _ => size.to_string(),
    }
}

/// Bytes as the card says them: whole gigabytes from 10 GB up (`266 GB`), and as everywhere else below that.
fn round_bytes(bytes: u64) -> String {
    const GB: u64 = 1024 * 1024 * 1024;
    if bytes >= 10 * GB {
        format!("{} GB", bytes / GB)
    } else {
        format_bytes(bytes)
    }
}

/// What the card says of the room: `Needs 30 GiB · 266 GB free`, either half alone when the other is not known.
pub(super) fn room_line(needs: Option<&str>, free: Option<u64>) -> Option<String> {
    let needs = needs.map(|n| format!("Needs {}", spaced_size(n)));
    let free = free.map(|f| format!("{} free", round_bytes(f)));
    match (needs, free) {
        (Some(needs), Some(free)) => Some(format!("{needs} · {free}")),
        (one, other) => one.or(other),
    }
}

/// `Step 4 of 8 · PyTorch for AMD GPUs (ROCm 7.2) · downloading …` as `(4, 8, "PyTorch for AMD GPUs (ROCm 7.2) · …")`.
pub(super) fn step_at(detail: &str) -> Option<(usize, usize, &str)> {
    let rest = detail.trim().strip_prefix("Step ")?;
    let (at, rest) = rest.split_once(" of ")?;
    let (of, rest) = rest.split_once(" · ").unwrap_or((rest, ""));
    Some((at.trim().parse().ok()?, of.trim().parse().ok()?, rest))
}

/// Where a step of the install is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum StepState {
    Done,
    Now,
    Next,
}

/// One step as the card lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct StepRow {
    pub title: String,
    pub state: StepState,
    /// What the step under way is doing, when it has said more than its name.
    pub said: Option<String>,
}

/// The steps of the install as the card lists them, from its plan and the line it said last: those before the one it
/// is on are done, and that one has what it is doing. An install that has said nothing yet is on its first step. `None`
/// when the line does not count the same steps as the plan (an install begun with other parts than the ones now
/// ticked): the card then says the line as it is.
pub(super) fn step_rows(planned: &[StepRecord], detail: Option<&str>) -> Option<Vec<StepRow>> {
    if planned.is_empty() {
        return None;
    }
    let (at, of, rest) = match detail {
        Some(detail) => step_at(detail)?,
        None => (1, planned.len(), ""),
    };
    if of != planned.len() || at == 0 || at > of {
        return None;
    }
    Some(
        planned
            .iter()
            .enumerate()
            .map(|(i, step)| {
                let state = match (i + 1).cmp(&at) {
                    Ordering::Less => StepState::Done,
                    Ordering::Equal => StepState::Now,
                    Ordering::Greater => StepState::Next,
                };
                StepRow {
                    title: sentence_case(&step.label),
                    state,
                    said: (state == StepState::Now)
                        .then(|| what_it_does(rest, &step.label))
                        .flatten(),
                }
            })
            .collect(),
    )
}

/// What follows the step's name in the line: `Downloading torch…: 2.2 GB of 5.8 GB` of
/// `PyTorch for AMD GPUs (ROCm 7.2) · downloading torch…: 2.2 GB of 5.8 GB`, and `It loads every node` of
/// `the check that it runs (it loads every node)`.
fn what_it_does(rest: &str, label: &str) -> Option<String> {
    let rest = rest.trim();
    let Some(after) = rest.strip_prefix(label) else {
        return (!rest.is_empty()).then(|| sentence_case(rest));
    };
    let after = after.trim_start_matches([' ', '·', ':']).trim();
    let after = after
        .strip_prefix('(')
        .and_then(|a| a.strip_suffix(')'))
        .unwrap_or(after);
    (!after.is_empty()).then(|| sentence_case(after))
}

/// The words of a part of the install that is the person's to choose.
struct OptionWords {
    id: String,
    label: String,
    badge: Option<String>,
    summary: Option<String>,
    help: Option<String>,
}

// ---- the card ----------------------------------------------------------------------------------------------------------

/// The frame of the card: no margin of its own, since each of its parts has one and a line between them.
fn card_frame() -> egui::Frame {
    let p = theme::palette();
    egui::Frame::new()
        .fill(p.card)
        .stroke(egui::Stroke::new(1.0, p.line))
        .corner_radius(egui::CornerRadius::same(8))
}

/// A part of the card, with the line over it when it is not the first.
fn part<R>(ui: &mut egui::Ui, first: bool, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    if !first {
        let rect = ui.available_rect_before_wrap();
        ui.painter().hline(
            rect.x_range(),
            rect.top(),
            egui::Stroke::new(1.0, theme::palette().line),
        );
    }
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(16, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

/// The square with the engine's mark at the left of its name.
fn badge(ui: &mut egui::Ui) {
    let p = theme::palette();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(40.0, 40.0), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, 8.0, p.raised.lerp_to_gamma(p.accent, 0.12));
    icons::paint(
        ui.painter(),
        egui::Rect::from_center_size(rect.center(), egui::vec2(20.0, 20.0)),
        Icon::Package,
        p.accent_text,
    );
}

/// The small heading of a part of the card (`WHAT IT INSTALLS`).
fn small_heading(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text.to_uppercase())
            .small()
            .family(theme::semibold(ui))
            .color(theme::palette().muted),
    );
    ui.add_space(2.0);
}

/// A part that comes with the install whatever is ticked: a quiet tick, its name, and what it is.
fn included(ui: &mut egui::Ui, title: &str, words: Option<&str>) -> Option<egui::Response> {
    let p = theme::palette();
    ui.horizontal_top(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
        icons::paint(
            ui.painter(),
            egui::Rect::from_center_size(rect.center(), egui::vec2(13.0, 13.0)),
            Icon::Check,
            p.faint,
        );
        ui.vertical(|ui| {
            ui.label(egui::RichText::new(title).color(p.text));
            words.map(|words| ui.add(egui::Label::new(theme::muted(words)).wrap()))
        })
        .inner
    })
    .inner
}

/// The mark of a step: a tick in a green ring when it is done, a ring with a dot while it is under way, an empty ring
/// before.
fn step_mark(ui: &mut egui::Ui, state: StepState) {
    let p = theme::palette();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
    let c = rect.center();
    match state {
        StepState::Done => {
            ui.painter().circle_filled(c, 9.0, p.accent_soft);
            icons::paint(
                ui.painter(),
                egui::Rect::from_center_size(c, egui::vec2(11.0, 11.0)),
                Icon::Check,
                p.accent_text,
            );
        }
        StepState::Now => {
            ui.painter()
                .circle_stroke(c, 8.0, egui::Stroke::new(2.0, p.accent_text));
            ui.painter().circle_filled(c, 3.0, p.accent_text);
        }
        StepState::Next => {
            ui.painter()
                .circle_stroke(c, 8.0, egui::Stroke::new(1.5, p.outline));
        }
    }
}

impl App {
    /// The page of an engine that is not here: the card, the ways round it under it (another computer, the engine that
    /// is ready), and why the last try failed.
    pub(super) fn show_engine_setup(&mut self, ui: &mut egui::Ui) {
        if !self.needs_install() {
            return;
        }
        let Some(engine) = self.engine_doc() else {
            return;
        };
        let name = engine.doc.engine.name.clone();
        let summary = engine.doc.engine.summary.clone();
        let repo = acquire::feed_repo_label(engine).unwrap_or_default();
        let from_source = acquire::builds_from_source(engine);
        let tag = acquire::tested_tag(engine).map(str::to_string);
        let date = acquire::tested_date(engine).map(str::to_string);
        // a release's source has no digest of GitHub's: it is checked only against the document's pin
        let pinned = !from_source
            || tag.as_deref().is_some_and(|t| {
                acquire::pin_for(engine, &acquire::source_archive_name(t)).is_some()
            });
        let root_path = acquire::root_dir(engine, &self.host_facts).unwrap_or_default();
        let root = display_root(Path::new(&root_path), &self.host_facts.home);
        let space = acquire::install_space(engine).map(str::to_string);
        let options: Vec<OptionWords> = acquire::install_options(engine, &self.host_facts)
            .into_iter()
            .map(|o| OptionWords {
                id: o.id.clone(),
                label: o.label.clone(),
                badge: o.badge.clone(),
                summary: o.summary.clone(),
                help: o.help.clone(),
            })
            .collect();
        let mut parts: Vec<String> = acquire::pip_steps(engine, &self.host_facts)
            .into_iter()
            .map(|s| sentence_case(s.label.as_deref().unwrap_or(&s.id)))
            .collect();
        parts.extend(
            acquire::node_packs(engine, &self.host_facts)
                .into_iter()
                .map(|n| sentence_case(n.label.as_deref().unwrap_or(&n.id))),
        );
        let own_python = acquire::own_python_version(engine).map(str::to_string);
        let named_python = self
            .values
            .text(INSTALL_PYTHON_FIELD)
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(str::to_string);
        let fallback = (self.engine != crate::settings::DEFAULT_ENGINE)
            .then(|| self.registry.engine(crate::settings::DEFAULT_ENGINE))
            .flatten()
            .map(|e| (e.id.clone(), e.doc.engine.name.clone()));
        let spec = self.install_spec();
        let free = self.free_space_at(ui.ctx(), &root_path, spec.as_ref().map(|s| s.free_space));
        let chosen = self.chosen_options();
        let looking = self.runtime.lookup_rx.is_some() && self.runtime.auto_install.is_some();
        let installing = self.runtime.install.is_some();
        let p = theme::palette();

        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new(if installing {
                    format!("Installing {name}")
                } else {
                    format!("Install {name}")
                })
                .size(22.0)
                .family(theme::semibold(ui))
                .color(p.text),
            );
            if let Some(summary) = summary.as_deref().filter(|_| !installing) {
                ui.add_space(2.0);
                ui.label(egui::RichText::new(summary).color(p.muted));
            }
        });
        ui.add_space(14.0);

        if installing {
            self.show_install_steps(ui, &name, &chosen, &options, spec.as_ref());
        } else {
            let mut wanted = Wanted::default();
            card_frame().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 0.0;
                // the version
                part(ui, true, |ui| {
                    ui.spacing_mut().item_spacing.y = 3.0;
                    ui.horizontal(|ui| {
                        badge(ui);
                        ui.add_space(6.0);
                        let other_width = theme::text_width(ui, OTHER_VERSIONS, egui::TextStyle::Button)
                            + 2.0 * ui.spacing().button_padding.x
                            + 8.0;
                        let words_width = (ui.available_width() - other_width).max(120.0);
                        ui.allocate_ui_with_layout(
                            egui::vec2(words_width, 0.0),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(
                                        egui::RichText::new(&name)
                                            .size(18.0)
                                            .family(theme::semibold(ui))
                                            .color(p.text),
                                    );
                                    if let Some(tag) = &tag {
                                        ui.label(
                                            egui::RichText::new(tag)
                                                .size(16.0)
                                                .monospace()
                                                .color(p.text),
                                        );
                                        theme::chip(ui, "tested with QuickAI", Some(Tone::Good))
                                            .on_hover_text(format!(
                                                "QuickAI was tested with {name} {tag}: it is the version Install \
                                                 puts in, and the one whose source is checked against a sha256 \
                                                 recorded when it was tested."
                                            ));
                                    }
                                });
                                ui.add(
                                    egui::Label::new(theme::muted(version_line(
                                        date.as_deref(),
                                        &repo,
                                        from_source,
                                        pinned && tag.is_some(),
                                    )))
                                    .wrap(),
                                );
                            },
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if theme::button_sized(ui, Role::Quiet, true, OTHER_VERSIONS, egui::vec2(0.0, 26.0))
                                .on_hover_text(format!(
                                    "Settings, Program: every {name} build on offer (the newer ones too), the ones \
                                     installed, and a {name} you already have."
                                ))
                                .clicked()
                            {
                                wanted.settings = true;
                            }
                        });
                    });
                });
                // what comes with it
                if !options.is_empty() || !parts.is_empty() || own_python.is_some() {
                    part(ui, false, |ui| {
                        ui.spacing_mut().item_spacing.y = 3.0;
                        small_heading(ui, "What it installs");
                        for option in &options {
                            let on = chosen.contains(&option.id);
                            if let Some(flipped) = option_row(ui, option, on, !looking) {
                                wanted.option = Some((option.id.clone(), flipped));
                            }
                            ui.add_space(6.0);
                        }
                        for part in &parts {
                            included(ui, part, None);
                            ui.add_space(4.0);
                        }
                        match (&named_python, &own_python) {
                            (Some(path), _) => {
                                included(ui, &format!("Your Python: {path}"), Some(
                                    "The Python you named in Settings makes the environment.",
                                ));
                            }
                            (None, Some(version)) => {
                                included(ui, &format!("Python {version}, QuickAI's own"), Some(
                                    "Downloaded once; this computer's Python is not used.",
                                ));
                            }
                            (None, None) => {}
                        }
                        if named_python.is_some() || own_python.is_some() {
                            ui.horizontal(|ui| {
                                ui.add_space(26.0);
                                if theme::button_sized(
                                    ui,
                                    Role::Quiet,
                                    true,
                                    egui::RichText::new("Use another Python…").small().color(p.accent_text),
                                    egui::vec2(0.0, 18.0),
                                )
                                .on_hover_text(
                                    "Settings, Program: the Python that makes the environment, if you want one of \
                                     this computer's instead.",
                                )
                                .clicked()
                                {
                                    wanted.settings = true;
                                }
                            });
                        }
                    });
                }
                // the room, the folder, and the button
                part(ui, false, |ui| {
                    ui.horizontal(|ui| {
                        let label = match &tag {
                            Some(tag) => format!("Install {name} {tag}"),
                            None => format!("Install {name}"),
                        };
                        let button_width = theme::text_width(ui, &label, egui::TextStyle::Button)
                            + 2.0 * ui.spacing().button_padding.x
                            + 24.0;
                        ui.allocate_ui_with_layout(
                            egui::vec2((ui.available_width() - button_width - 12.0).max(120.0), 0.0),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.spacing_mut().item_spacing.y = 2.0;
                                if let Some(room) = room_line(space.as_deref(), free) {
                                    ui.label(theme::muted(room));
                                }
                                ui.add(
                                    egui::Label::new(theme::muted(format!(
                                        "Kept in {root}. Nothing else on your computer changes."
                                    )))
                                    .wrap(),
                                )
                                .on_hover_text(
                                    "Nothing is downloaded until you click Install. Deleting it again is in \
                                     Settings.",
                                );
                            },
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let idle = !looking && self.runtime.delete.is_none();
                            if theme::button_sized(
                                ui,
                                Role::Primary,
                                idle,
                                label.as_str(),
                                egui::vec2(button_width, 34.0),
                            )
                            .on_hover_text(match &tag {
                                Some(tag) => format!(
                                    "Download {name} {tag} (or the newest, when that one is gone), check it and \
                                     install it."
                                ),
                                None => format!(
                                    "Download the newest {name} for this computer, check it and install it."
                                ),
                            })
                            .clicked()
                            {
                                wanted.install = true;
                            }
                            if looking {
                                ui.spinner();
                            }
                        });
                    });
                });
            });
            if looking {
                ui.add_space(6.0);
                ui.vertical_centered(|ui| {
                    ui.label(theme::muted(self.runtime.lookup_status.clone()));
                });
            }
            // the ways round it, quiet, under the card
            let mut links: Vec<(&str, String)> = Vec::new();
            if self.can_run_remote() {
                links.push((
                    "Run on another computer…",
                    format!(
                        "Use the {name} of a more powerful computer instead, reached over SSH: nothing is installed \
                         here."
                    ),
                ));
            }
            let other = fallback
                .as_ref()
                .map(|(_, other)| format!("Use {other} instead"));
            if let (Some(other), Some((_, other_name))) = (&other, &fallback) {
                links.push((
                    other.as_str(),
                    format!("{other_name} is ready to use, with no download."),
                ));
            }
            if !links.is_empty() {
                ui.add_space(8.0);
                let font = egui::TextStyle::Button.resolve(ui.style());
                let widths: f32 = links
                    .iter()
                    .map(|(l, _)| {
                        ui.painter()
                            .layout_no_wrap(l.to_string(), font.clone(), p.muted)
                            .size()
                            .x
                            + 2.0 * ui.spacing().button_padding.x
                            + ui.spacing().item_spacing.x
                    })
                    .sum();
                ui.horizontal(|ui| {
                    ui.add_space(((ui.available_width() - widths) / 2.0).max(0.0));
                    for (i, (label, hover)) in links.iter().enumerate() {
                        if theme::button_sized(ui, Role::Quiet, true, *label, egui::vec2(0.0, 26.0))
                            .on_hover_text(hover.as_str())
                            .clicked()
                        {
                            wanted.link = Some(i);
                        }
                    }
                });
            }
            match wanted.link.map(|i| links[i].0) {
                Some("Run on another computer…") => self.open_computers(),
                Some(_) => {
                    if let Some((id, _)) = &fallback {
                        self.switch_engine(id);
                    }
                }
                None => {}
            }
            if let Some((id, on)) = wanted.option {
                self.set_install_option(&id, on);
            }
            if wanted.settings {
                self.advanced_section_wanted = true;
                self.open_settings();
            }
            if wanted.install {
                self.install_engine(ui.ctx());
            }
        }
        if let Some(why) = self.runtime.setup_failure.clone().filter(|_| !installing) {
            ui.add_space(10.0);
            theme::alert(ui, Tone::Bad, |ui| {
                ui.add(egui::Label::new(egui::RichText::new(why).color(p.bad)).wrap());
            });
        }
    }

    /// The card while the engine installs: its version and the parts chosen, one bar for all of it and Cancel, then
    /// every step, the one under way with what it is doing.
    fn show_install_steps(
        &mut self,
        ui: &mut egui::Ui,
        name: &str,
        chosen: &[String],
        options: &[OptionWords],
        spec: Option<&install::InstallSpec>,
    ) {
        let p = theme::palette();
        let Some(job) = &self.runtime.install else {
            return;
        };
        let label = job.label.clone();
        let fraction = job.fraction();
        let detail = job.detail().map(str::to_string);
        let planned = spec
            .map(|spec| install::planned_steps(spec, &InstallRequest::bare("", &label)))
            .unwrap_or_default();
        let rows = step_rows(&planned, detail.as_deref());
        let counted = detail
            .as_deref()
            .and_then(step_at)
            .map(|(at, of, _)| (at, of));
        let mut cancel = false;
        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            part(ui, true, |ui| {
                ui.spacing_mut().item_spacing.y = 6.0;
                ui.horizontal(|ui| {
                    badge(ui);
                    ui.add_space(6.0);
                    let cancel_width = theme::text_width(ui, "Cancel", egui::TextStyle::Button)
                        + 2.0 * ui.spacing().button_padding.x
                        + 12.0;
                    ui.allocate_ui_with_layout(
                        egui::vec2((ui.available_width() - cancel_width).max(120.0), 0.0),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(
                                    egui::RichText::new(name)
                                        .size(18.0)
                                        .family(theme::semibold(ui))
                                        .color(p.text),
                                );
                                ui.label(
                                    egui::RichText::new(&label)
                                        .size(16.0)
                                        .monospace()
                                        .color(p.text),
                                );
                                for option in options.iter().filter(|o| chosen.contains(&o.id)) {
                                    let what = option
                                        .label
                                        .strip_prefix("Install ")
                                        .unwrap_or(&option.label);
                                    theme::chip(ui, &format!("with {what}"), None);
                                }
                            });
                            ui.horizontal(|ui| {
                                let said = match (counted, fraction) {
                                    (Some((at, of)), Some(f)) => {
                                        format!(
                                            "Step {at} of {of} · {}%",
                                            (f * 100.0).floor() as u32
                                        )
                                    }
                                    (Some((at, of)), None) => format!("Step {at} of {of}"),
                                    (None, Some(f)) => format!("{}%", (f * 100.0).floor() as u32),
                                    (None, None) => String::new(),
                                };
                                let said_width =
                                    theme::text_width(ui, &said, egui::TextStyle::Small) + 10.0;
                                let bar_width = (ui.available_width() - said_width).max(60.0);
                                ui.scope(|ui| {
                                    ui.set_width(bar_width);
                                    super::widgets::progress_bar(ui, fraction);
                                });
                                ui.label(theme::muted(said));
                            });
                        },
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if self.runtime.cancel.is_some()
                            && ui
                                .button("Cancel")
                                .on_hover_text(
                                    "Stop the install. What was fetched so far is thrown away.",
                                )
                                .clicked()
                        {
                            cancel = true;
                        }
                    });
                });
            });
            part(ui, false, |ui| match &rows {
                Some(rows) => {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    for row in rows {
                        ui.horizontal_top(|ui| {
                            step_mark(ui, row.state);
                            ui.add_space(4.0);
                            ui.vertical(|ui| {
                                let color = if row.state == StepState::Next {
                                    p.muted
                                } else {
                                    p.text
                                };
                                ui.label(egui::RichText::new(&row.title).color(color));
                                if let Some(said) = &row.said {
                                    ui.add(egui::Label::new(theme::muted(said)).wrap());
                                }
                            });
                        });
                        ui.add_space(4.0);
                    }
                }
                None => {
                    if let Some(detail) = &detail {
                        ui.add(egui::Label::new(theme::muted(detail)).wrap());
                    } else {
                        ui.label(theme::muted("Starting…"));
                    }
                }
            });
        });
        ui.add_space(8.0);
        ui.vertical_centered(|ui| {
            ui.label(theme::muted("Every line it prints is in the Log."));
        });
        if cancel {
            if let Some(flag) = &self.runtime.cancel {
                flag.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        }
    }

    /// The version of the build in use, when it is one QuickAI installed: the name of its folder under the engine's
    /// builds (`v0.45.2`). `None` for a program the person named, or one found elsewhere.
    pub(super) fn build_in_use(&self) -> Option<String> {
        let engine = self.engine_doc()?;
        let root = acquire::root_dir(engine, &self.host_facts)?;
        build_label_of(&self.binary, Path::new(&root))
    }

    /// How much room there is where the engine's builds go: asked of the system once every few seconds, not at every
    /// frame. `None` when the install has no way to ask, or the system did not say.
    fn free_space_at(
        &self,
        ctx: &egui::Context,
        root: &str,
        ask: Option<fn(&Path) -> Option<u64>>,
    ) -> Option<u64> {
        let ask = ask?;
        if root.is_empty() {
            return None;
        }
        let id = egui::Id::new(("install_page_free_space", root.to_string()));
        let now = ctx.input(|i| i.time);
        let kept: Option<(f64, Option<u64>)> = ctx.data(|d| d.get_temp(id));
        match kept {
            Some((when, free)) if now - when < 5.0 => free,
            _ => {
                let free = ask(Path::new(root));
                ctx.data_mut(|d| d.insert_temp(id, (now, free)));
                free
            }
        }
    }
}

/// The folder of `root` that `binary` is in (`v0.45.2` of `~/.quickai/comfyui/v0.45.2/comfyui`); `None` when it is not
/// below `root`, or is `root`'s own file.
pub(super) fn build_label_of(binary: &Path, root: &Path) -> Option<String> {
    let rest = binary.strip_prefix(root).ok()?;
    let mut parts = rest.components();
    let first = parts.next()?;
    parts.next()?;
    Some(first.as_os_str().to_string_lossy().to_string())
}

/// What the person asked of the card in this frame, done once the card is drawn.
#[derive(Default)]
struct Wanted {
    install: bool,
    settings: bool,
    option: Option<(String, bool)>,
    link: Option<usize>,
}

/// A part of the install that is the person's to choose: its box, its name and badge, and in a line under them what it
/// is for. The name can be clicked as the box can. What it was turned to, when it was.
fn option_row(ui: &mut egui::Ui, option: &OptionWords, on: bool, enabled: bool) -> Option<bool> {
    let p = theme::palette();
    let mut ticked = on;
    let mut flipped = false;
    ui.horizontal_top(|ui| {
        let tick = ui.add_enabled(enabled, egui::Checkbox::without_text(&mut ticked));
        if tick.changed() {
            flipped = true;
        }
        if let Some(help) = &option.help {
            tick.on_hover_text(help);
        }
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                let name = ui.add_enabled(
                    enabled,
                    egui::Label::new(egui::RichText::new(&option.label).color(p.text))
                        .sense(egui::Sense::click()),
                );
                let name = match &option.help {
                    Some(help) => name.on_hover_text(help),
                    None => name,
                };
                if name.clicked() {
                    ticked = !on;
                    flipped = true;
                }
                if let Some(badge) = &option.badge {
                    let chip = theme::chip(ui, badge, Some(Tone::Good));
                    if let Some(help) = &option.help {
                        chip.on_hover_text(help);
                    }
                }
            });
            let words = match (&option.summary, on || ticked) {
                (Some(summary), true) => Some(summary.clone()),
                (Some(summary), false) => Some(format!(
                    "{summary} You can add it later: install again with this ticked."
                )),
                (None, _) => None,
            };
            if let Some(words) = words {
                ui.add(egui::Label::new(theme::muted(words)).wrap());
            }
        });
    });
    flipped.then_some(ticked)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(id: &str, label: &str) -> StepRecord {
        StepRecord {
            id: id.into(),
            label: label.into(),
        }
    }

    fn comfy_plan() -> Vec<StepRecord> {
        vec![
            step("source", "the source of v0.45.2"),
            step("node:gguf", "ComfyUI-GGUF"),
            step("venv", "the Python environment"),
            step("pip:torch_amd", "PyTorch for AMD GPUs (ROCm 7.2)"),
            step("pip:requirements", "what ComfyUI needs"),
            step("pip:manager", "ComfyUI-Manager"),
            step("needs:gguf", "what ComfyUI-GGUF needs"),
            step("check", "the check that it runs"),
        ]
    }

    #[test]
    fn the_version_in_use_is_the_folder_of_the_build_under_the_engines_builds() {
        let root = Path::new("/home/u/.quickai/comfyui");
        assert_eq!(
            build_label_of(&root.join("v0.45.2/comfyui"), root).as_deref(),
            Some("v0.45.2")
        );
        assert_eq!(build_label_of(&root.join("comfyui"), root), None);
        assert_eq!(
            build_label_of(Path::new("/opt/comfyui/main.py"), root),
            None
        );
    }

    #[test]
    fn the_date_of_the_document_is_said_in_words_and_what_is_not_a_date_is_not() {
        assert_eq!(long_date("2026-11-11").as_deref(), Some("11 November 2026"));
        assert_eq!(long_date("2026-01-05").as_deref(), Some("5 January 2026"));
        assert_eq!(long_date("2026-13-01"), None);
        assert_eq!(long_date("2026-00-01"), None);
        assert_eq!(long_date("26-11-11"), None);
        assert_eq!(long_date("2026-11-40"), None);
        assert_eq!(long_date("soon"), None);
    }

    #[test]
    fn the_line_under_the_version_says_when_where_from_and_how_it_is_checked() {
        assert_eq!(
            version_line(Some("2026-11-11"), "github.com/Comfy-Org/ComfyUI", true, true),
            "Released 11 November 2026 · built from its source at github.com/Comfy-Org/ComfyUI and checked against \
             its sha256"
        );
        // no date in the document, and a release that has no pin: nothing is claimed of either
        assert_eq!(
            version_line(None, "github.com/ollama/ollama", false, false),
            "Downloaded from github.com/ollama/ollama"
        );
        assert_eq!(
            version_line(Some("not a date"), "", true, false),
            "Built from its source"
        );
    }

    #[test]
    fn the_room_is_what_the_document_needs_and_what_the_disk_has() {
        const GB: u64 = 1024 * 1024 * 1024;
        assert_eq!(spaced_size("30GiB"), "30 GiB");
        assert_eq!(spaced_size("30 GiB"), "30 GiB");
        assert_eq!(spaced_size("big"), "big");
        assert_eq!(
            room_line(Some("30GiB"), Some(266 * GB + 5)).as_deref(),
            Some("Needs 30 GiB · 266 GB free")
        );
        assert_eq!(
            room_line(Some("30GiB"), None).as_deref(),
            Some("Needs 30 GiB")
        );
        assert_eq!(
            room_line(None, Some(3 * GB)).as_deref(),
            Some("3.0 GB free")
        );
        assert_eq!(room_line(None, None), None);
    }

    #[test]
    fn the_step_under_way_is_read_from_the_line_and_those_before_it_are_done() {
        let plan = comfy_plan();
        let line = "Step 4 of 8 · PyTorch for AMD GPUs (ROCm 7.2) · downloading torch-2.14.1+rocm7.2.whl: 2.2 GB of \
                    5.8 GB, 6.9 MB/s, about 9 min 02 s left";
        let rows = step_rows(&plan, Some(line)).expect("the line counts the plan's steps");
        let states: Vec<StepState> = rows.iter().map(|r| r.state).collect();
        use StepState::*;
        assert_eq!(states, [Done, Done, Done, Now, Next, Next, Next, Next]);
        assert_eq!(rows[0].title, "The source of v0.45.2");
        assert_eq!(rows[3].title, "PyTorch for AMD GPUs (ROCm 7.2)");
        assert_eq!(
            rows[3].said.as_deref(),
            Some("Downloading torch-2.14.1+rocm7.2.whl: 2.2 GB of 5.8 GB, 6.9 MB/s, about 9 min 02 s left")
        );
        assert!(rows
            .iter()
            .filter(|r| r.state != Now)
            .all(|r| r.said.is_none()));
    }

    #[test]
    fn a_step_that_said_only_its_name_says_nothing_more_and_the_check_says_what_is_in_brackets() {
        let plan = comfy_plan();
        let rows = step_rows(&plan, Some("Step 2 of 8 · ComfyUI-GGUF")).unwrap();
        assert_eq!(rows[1].state, StepState::Now);
        assert_eq!(rows[1].said, None);
        let rows = step_rows(
            &plan,
            Some("Step 8 of 8 · the check that it runs (it loads every node; the first time can take a minute)"),
        )
        .unwrap();
        assert_eq!(
            rows[7].said.as_deref(),
            Some("It loads every node; the first time can take a minute")
        );
    }

    #[test]
    fn an_install_that_said_nothing_yet_is_on_its_first_step_and_one_that_counts_otherwise_is_not_listed(
    ) {
        let plan = comfy_plan();
        let rows = step_rows(&plan, None).unwrap();
        assert_eq!(rows[0].state, StepState::Now);
        assert!(rows[1..].iter().all(|r| r.state == StepState::Next));
        // begun without the Manager: seven steps, not the eight the plan has now
        assert_eq!(
            step_rows(&plan, Some("Step 3 of 7 · the Python environment")),
            None
        );
        assert_eq!(step_rows(&plan, Some("Unpacking ollama.tar.zst")), None);
        assert_eq!(step_rows(&[], Some("Step 1 of 1 · x")), None);
        assert_eq!(step_at("Step 3 of 7"), Some((3, 7, "")));
    }
}
