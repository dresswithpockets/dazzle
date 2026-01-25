use std::{
    fs,
    io::{self, ErrorKind},
    thread::{self, JoinHandle},
    time::Duration,
};

use eframe::egui::{self, Align2, Layout, Vec2b, Window};
use egui_extras::{Column, Size, StripBuilder, TableBuilder};

use addon::Addon;

use crate::app::process::{ProcessState, ProcessView};

#[derive(Debug)]
pub struct AddonState {
    pub enabled: bool,
    pub addon: Addon,
}

pub fn addons_manager(ui: &mut egui::Ui, addons: &mut Vec<AddonState>) -> Response {
    let mut action = None;

    let desired_size = ui.available_size() - (100.0, 160.0).into();
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
                        ui.group(|ui| {
                            if let Some(delete_idx) = addons_table(ui, addons) {
                                action = Some(Action::DeleteAddon(delete_idx));
                            }
                        });
                    });

                    strip.cell(|ui| {
                        ui.group(|ui| {
                            if let Some(inner) = actions(ui) {
                                action = Some(inner);
                            }
                        });
                    });
                });
        });

    Response { action }
}

fn addons_table(ui: &mut egui::Ui, addons: &mut [AddonState]) -> Option<usize> {
    let mut move_addon_up = None;
    let mut move_addon_down = None;
    let mut delete_addon = None;

    TableBuilder::new(ui)
        .striped(true)
        .resizable(true)
        .cell_layout(Layout::left_to_right(egui::Align::Center))
        .column(Column::auto())
        .column(Column::auto())
        .column(Column::remainder())
        .column(Column::remainder())
        .column(Column::remainder())
        .header(20.0, |mut header| {
            header.col(|ui| {
                ui.strong("Enabled");
            });
            header.col(|ui| {
                ui.strong("Name");
            });
            header.col(|ui| {
                ui.strong("Author");
            });
            header.col(|ui| {
                ui.strong("Description");
            });
            header.col(|ui| {
                ui.strong("Actions");
            });
        })
        .body(|body| {
            // TODO: how do we get/store configuration for each addon? such as their priority and whether or not to disable/enable them
            let row_count = addons.len();
            body.rows(20.0, row_count, |mut row| {
                let row_index = row.index();
                let AddonState { enabled, addon } = addons.get_mut(row_index).unwrap();

                row.col(|ui| {
                    if *enabled {
                        ui.label("✔");
                    }
                });
                row.col(|ui| { ui.label(addon.name()); });
                row.col(|ui| { ui.label(""); });
                row.col(|ui| { ui.label(""); });
                row.col(|ui| {
                    let button = if *enabled {
                        ui.button("disable")
                    } else {
                        ui.button("enable")
                    };

                    if button.on_hover_text("When disabled, addons do not get installed.").clicked() {
                        *enabled = !*enabled;
                    }

                    let up_button = ui.add_enabled_ui(row_index > 0, |ui| {
                        ui.button("up").on_hover_text("Files from higher priority addons will get chosen first when a conflict between two addons is discovered")
                    }).inner;

                    let down_button = ui.add_enabled_ui(row_index < row_count - 1, |ui| {
                        ui.button("down").on_hover_text("Files from higher priority addons will get chosen first when a conflict between two addons is discovered")
                    }).inner;

                    if up_button.clicked() {
                        move_addon_up = Some(row_index);
                    }

                    if down_button.clicked() {
                        move_addon_down = Some(row_index);
                    }

                    if ui.button("delete").on_hover_text("Permanently deletes the addon's files from the addons folder").clicked() {
                        delete_addon = Some(row_index);
                    }
                });

                // TODO: drag/drop for reordering? it seems like it would be quite complicated to track the positions of each item in the table

                if row.response().clicked() {
                    // TODO: addon row has been left-clicked, should we toggle enable/disable on left click?
                }

                if row.response().secondary_clicked() {
                    // TODO: addon row has been right-clicked, show context menu to:
                    //  - enable/disable addon
                    //  - reorder addon >
                    //      - up
                    //      - down
                    //      - top
                    //      - bottom
                    //  - delete addon
                    //
                    // mark row as selected to show that the context menu is for this particular row. make sure all other rows are not selected
                }
            });
        });

    if let Some(idx) = move_addon_up {
        addons.swap(idx, idx - 1);
    }

    if let Some(idx) = move_addon_down {
        addons.swap(idx, idx + 1);
    }

    delete_addon
}

fn actions(ui: &mut egui::Ui) -> Option<Action> {
    let mut response = None;
    StripBuilder::new(ui)
        .cell_layout(Layout::left_to_right(egui::Align::Center))
        .size(Size::remainder())
        .size(Size::remainder())
        .size(Size::remainder())
        .size(Size::remainder())
        .horizontal(|mut strip| {
            strip.cell(|ui| {
                ui.centered_and_justified(|ui| {
                    if ui
                        .button("Open Addons Folder")
                        .on_hover_text("opens dazzle addons folder in your file explorer")
                        .clicked()
                    {
                        response = Some(Action::OpenAddonsFolder);
                    }
                });
            });
            strip.cell(|ui| {
                ui.centered_and_justified(|ui| {
                    if ui
                        .button("Add Addon")
                        .on_hover_text("open a dialogue to select an addon to install")
                        .clicked()
                    {
                        response = Some(Action::AddAddon);
                    }
                });
            });
            strip.cell(|ui| {
                ui.centered_and_justified(|ui| {
                    if ui
                        .button("Install Addons")
                        .on_hover_text("installs selected addons into your tf directory")
                        .clicked()
                    {
                        response = Some(Action::InstallAddons);
                    }
                });
            });
            strip.cell(|ui| {
                ui.centered_and_justified(|ui| {
                    if ui
                        .button("Uninstall Addons")
                        .on_hover_text(
                            "removes any Dazzle customizations from your tf directory, resetting them back to vanilla",
                        )
                        .clicked()
                    {
                        response = Some(Action::UninstallAddons);
                    }
                });
            });
        });

    response
}

pub struct Response {
    pub action: Option<Action>,
}

pub enum Action {
    DeleteAddon(usize),
    OpenAddonsFolder,
    AddAddon,
    InstallAddons,
    UninstallAddons,
}

pub fn start_addon_removal(ctx: &egui::Context, addon: Addon) -> (ProcessView, JoinHandle<Result<(), io::Error>>) {
    let (state, view) = ProcessState::with_spinner(ctx);
    let handle = thread::spawn(move || -> Result<(), io::Error> {
        state.push_status(format!("Removing '{}'", addon.name()));

        // for small addons, this job ends up running too fast - theres no good feedback for the user. So we sleep a bit
        thread::sleep(Duration::from_millis(500));

        fs::remove_dir_all(&addon.content_path)?;
        let result = if let Err(err) = fs::remove_dir_all(&addon.source_path) {
            if err.kind() == ErrorKind::NotADirectory {
                fs::remove_file(&addon.source_path)
            } else {
                Err(err)
            }
        } else {
            Ok(())
        };

        state.push_status("Done!");
        thread::sleep(Duration::from_millis(500));

        result
    });

    (view, handle)
}
