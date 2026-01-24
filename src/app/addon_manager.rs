use eframe::egui::{self, Align2, Layout, Vec2b, Window};
use egui_extras::{Column, Size, StripBuilder, TableBuilder};

use crate::addon::Addon;

#[derive(Debug)]
pub(crate) struct Manager {
    addons: Vec<Addon>,
}

impl Manager {
    pub(crate) fn new(addons: Vec<Addon>) -> Self {
        Self { addons }
    }

    pub(crate) fn show(&mut self, ui: &mut egui::Ui) {
        let desired_size = ui.available_size() - (20.0, 60.0).into();

        Window::new("✨ Addons")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, (0.0, 0.0))
            .min_size(desired_size)
            .scroll(Vec2b::FALSE)
            .show(ui.ctx(), |ui| {
                StripBuilder::new(ui)
                    .size(Size::remainder())
                    .size(Size::relative(0.1))
                    .vertical(|mut strip| {
                        strip.cell(|ui| {
                            ui.group(|ui|
                            TableBuilder::new(ui)
                                .striped(true)
                                .resizable(true)
                                .cell_layout(Layout::left_to_right(egui::Align::Center))
                                .column(Column::auto())
                                .column(Column::remainder())
                                .header(20.0, |mut header| {
                                    header.col(|ui| {
                                        ui.strong("Name");
                                    });
                                    header.col(|ui| {
                                        ui.strong("blah blah");
                                    });
                                })
                                .body(|mut body| {
                                    for addon in &self.addons {
                                        body.row(20.0, |mut ui| {
                                            ui.col(|ui| {
                                                ui.label(addon.name());
                                            });
                                            ui.col(|ui| {
                                                ui.label(addon.name());
                                            });
                                        });
                                    }
                                }));
                        });

                        strip.cell(|ui| {
                            ui.group(|ui| {
                                ui.scope_builder(
                                    egui::UiBuilder::new().layout(Layout::left_to_right(egui::Align::Center).with_cross_justify(true)),
                                    |ui| {
                                        if ui.button("Open Addons Folder").clicked() {
                                            // TODO:
                                        }

                                        if ui.button("Add Addon").clicked() {
                                            // TODO:
                                        }

                                        if ui.button("Install Addons").on_hover_text("installs selected addons into your tf/ directory").clicked() {
                                            // TODO:
                                        }

                                        if ui.button("Uninstall Addons").on_hover_text("removes any Dazzle customizations from your tf/ directory, resetting them back to vanilla").clicked() {
                                            // TODO:
                                        }
                                    }
                                );
                            });
                        });
                    });
            });
    }
}
