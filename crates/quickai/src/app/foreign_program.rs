//! The question a document of the person's asks before it starts a program the application did not
//! find (docs/architecture.md, section 12.1; decision C33). Nothing starts until it is answered, and a
//! yes is remembered by the hash of what the merged document says, so the same document is not asked
//! about twice and a changed one is.

use std::path::Path;

use eframe::egui;
use quickai_core::guard::{judge, Verdict};
use quickai_core::plan::{RunPlan, Start};

use super::App;
use crate::launch;

impl App {
    /// Whether `program` is one the application found by the strategies of its own document for this
    /// engine (with the executable the person chose in the window, which is theirs to choose), or
    /// one that is in a runtime it installed.
    fn trusted_program(&self, program: &str) -> bool {
        let Some(engine) = self.embedded.engine(&self.engine) else {
            return false;
        };
        let found = launch::locate_binary_in(
            &self.embedded,
            &self.engine,
            self.values.text("bin_override"),
            self.data_dir_override.as_deref(),
        );
        if Path::new(program) == found {
            return true;
        }
        quickai_core::acquire::root_dir(engine, &self.host_facts)
            .is_some_and(|root| Path::new(program).starts_with(root))
    }

    /// The program the plan starts, if the person has not yet said it may run.
    pub(super) fn foreign_program(&self, plan: &RunPlan) -> Option<quickai_core::guard::Foreign> {
        let engine = self.engine_doc()?;
        let program = plan
            .units
            .first()?
            .start
            .iter()
            .find_map(|start| match start {
                Start::Spawn(spawn) => Some(spawn.program.value.clone()),
                Start::Attach(_) => None,
            })?;
        match judge(
            engine,
            &program,
            &|candidate| self.trusted_program(candidate),
            &self.stored.confirmed_engines(),
        ) {
            Verdict::NeedsConfirmation(foreign) => Some(foreign),
            Verdict::Trusted | Verdict::Confirmed => None,
        }
    }

    /// Asks, while a start is waiting on it. Yes remembers the document and starts; no starts
    /// nothing and leaves nothing behind.
    pub(super) fn show_foreign_program_modal(&mut self, ctx: &egui::Context) {
        let Some(foreign) = &self.pending_foreign else {
            return;
        };
        let name = self.engine_name();
        let (program, files, shown_hash) = (
            foreign.program.clone(),
            foreign.files.join(", "),
            foreign.hash[..12].to_string(),
        );
        let mut run = false;
        let mut decline = false;
        let modal = egui::Modal::new(egui::Id::new("foreign_program_modal")).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.label(egui::RichText::new("Run this program?").strong());
            ui.add_space(6.0);
            ui.label(format!(
                "A document in your own configuration ({files}) says that {name} starts\n{program}\n\
                 which QuickAI did not install and did not find. Run it only if you wrote that document \
                 or trust whoever did."
            ));
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(format!(
                    "Document {shown_hash}. QuickAI asks once for each version of a document."
                ))
                .small()
                .color(egui::Color32::GRAY),
            );
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.button("Don't run it").clicked() {
                    decline = true;
                }
                if ui.button("Run it").clicked() {
                    run = true;
                }
            });
        });
        if modal.should_close() {
            decline = true;
        }
        if run || decline {
            self.answer_foreign_question(run, ctx);
        }
    }

    /// The person's answer: yes remembers the document by its hash and starts what was asked for,
    /// no starts nothing and leaves nothing behind but a line in the log.
    pub(super) fn answer_foreign_question(&mut self, run: bool, ctx: &egui::Context) {
        let Some(foreign) = self.pending_foreign.take() else {
            return;
        };
        if run {
            self.stored.confirm_engine(&foreign.hash);
            self.write_settings(&self.stored.clone());
            self.start_selected(ctx);
        } else {
            self.push_log(format!(
                "[quickai] {} was not started: you did not allow {} to run.",
                self.engine_name(),
                foreign.program
            ));
        }
    }
}
