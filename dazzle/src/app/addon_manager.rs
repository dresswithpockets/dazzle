use std::{
    collections::HashSet,
    fs,
    io::{self, ErrorKind},
    thread::{self, JoinHandle},
    time::Duration,
};

use anyhow::anyhow;
use bytes::{Buf, BufMut, BytesMut};
use dmx::Dmx;
use eframe::egui::{self, Align2, Layout, Vec2b, Window};
use egui_extras::{Column, Size, StripBuilder, TableBuilder};

use addon::{Addon, Sources};
use itertools::Itertools;
use pcfpack::BinPack;
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use typed_path::Utf8PlatformPathBuf;
use vpk::VPK;
use walkdir::WalkDir;
use writevpk::patch::PatchVpkExt;

use crate::{
    app::{
        Paths,
        config::{self, AddonConfig, Config},
        initial_load::LoadError,
        process::{ProcessState, ProcessView},
    },
    particles_manifest,
};

const SPLIT_BY_2GB: u32 = 2 << 30;

#[derive(Debug)]
pub struct AddonState {
    pub enabled: bool,
    pub addon: Addon,
}

pub fn addons_manager(ui: &mut egui::Ui, addons: &mut [AddonState]) -> Response {
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
    let mut move_addon_top = None;
    let mut move_addon_down = None;
    let mut move_addon_bottom = None;
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

                    ui.separator();

                    let up_button = ui.add_enabled_ui(row_index > 0, |ui| {
                        ui.button("up").on_hover_text("Files from higher priority addons will get chosen first when a conflict between two addons is discovered")
                    }).inner;

                    if up_button.clicked() {
                        move_addon_up = Some(row_index);
                    }

                    let top_button = ui.add_enabled_ui(row_index > 0, |ui| {
                        ui.button("top").on_hover_text("Files from higher priority addons will get chosen first when a conflict between two addons is discovered")
                    }).inner;

                    if top_button.clicked() {
                        move_addon_top = Some(row_index);
                    }

                    let down_button = ui.add_enabled_ui(row_index < row_count - 1, |ui| {
                        ui.button("down").on_hover_text("Files from higher priority addons will get chosen first when a conflict between two addons is discovered")
                    }).inner;

                    if down_button.clicked() {
                        move_addon_down = Some(row_index);
                    }

                    let bottom_button = ui.add_enabled_ui(row_index < row_count - 1, |ui| {
                        ui.button("bottom").on_hover_text("Files from higher priority addons will get chosen first when a conflict between two addons is discovered")
                    }).inner;

                    if bottom_button.clicked() {
                        move_addon_bottom = Some(row_index);
                    }

                    ui.separator();

                    if ui.button("delete").on_hover_text("Permanently deletes the addon's files from the addons folder").clicked() {
                        delete_addon = Some(row_index);
                    }
                });

                // TODO: drag/drop for reordering? it seems like it would be quite complicated to track the positions of each item in the table

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

    if let Some(idx) = move_addon_top {
        addons.swap(idx, 0);
    }

    if let Some(idx) = move_addon_down {
        addons.swap(idx, idx + 1);
    }

    if let Some(idx) = move_addon_bottom {
        addons.swap(idx, addons.len() - 1);
    }

    delete_addon
}

fn actions(ui: &mut egui::Ui) -> Option<Action> {
    let mut response = None;
    StripBuilder::new(ui)
        .cell_layout(Layout::left_to_right(egui::Align::Center))
        .size(Size::relative(0.225))
        .size(Size::relative(0.225))
        .size(Size::remainder())
        .size(Size::remainder())
        .horizontal(|mut strip| {
            strip.cell(|ui| {
                ui.vertical_centered_justified(|ui| {
                    if ui
                        .button("Add Addon - From Vpk")
                        .on_hover_text("open a dialogue to select an archive files (vpk, zip, tarball, etc) to install")
                        .clicked()
                    {
                        response = Some(Action::AddAddonFiles);
                    }
                    if ui
                        .button("Add Addon - From Folder")
                        .on_hover_text("open a dialogue to select addon folders to install")
                        .clicked()
                    {
                        response = Some(Action::AddAddonFolders);
                    }
                });
            });
            strip.cell(|ui| {
                ui.vertical_centered_justified(|ui| {
                    if ui
                        .button("Open Addons Folder")
                        .on_hover_text("opens dazzle addons folder in your file explorer")
                        .clicked()
                    {
                        response = Some(Action::OpenAddonsFolder);
                    }
                    if ui
                        .button("Open TF Folder")
                        .on_hover_text("opens the TF folder folder in your file explorer")
                        .clicked()
                    {
                        response = Some(Action::OpenTfFolder);
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
    OpenTfFolder,
    AddAddonFiles,
    AddAddonFolders,
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

pub fn start_addon_add(
    ctx: &egui::Context,
    paths: &Paths,
    mut addons: Vec<AddonState>,
    files: Vec<Utf8PlatformPathBuf>,
) -> (
    ProcessView,
    JoinHandle<(Vec<AddonState>, Vec<(Utf8PlatformPathBuf, LoadError)>)>,
) {
    assert!(!files.is_empty());

    let steps = (files.len() * 3) + 1;
    let addons_dir = paths.addons.clone();
    let extracted_content_dir = paths.extracted_content.clone();
    let (state, view) = ProcessState::with_progress_bar(ctx, steps.try_into().unwrap());
    let handle = thread::spawn(move || -> (Vec<AddonState>, Vec<(Utf8PlatformPathBuf, LoadError)>) {
        let original_count = files.len();
        let files: Vec<_> = files
            .into_iter()
            .filter(|file| {
                let name = file.file_name().unwrap();

                if addons.iter().any(|state| state.addon.name().eq_ignore_ascii_case(name)) {
                    let choice = state.confirm(
                        format!("An addon with the name '{name}' has already been added. What do you want to do?"),
                        ["Skip", "Replace Existing"],
                    );

                    choice == 1
                } else {
                    false
                }
            })
            .collect();

        let steps_to_increment_by_for_removed = original_count - files.len();
        if steps_to_increment_by_for_removed > 0 {
            state.add_progress(steps_to_increment_by_for_removed * 3);
        }

        if files.is_empty() {
            return (addons, Vec::new());
        }

        let files: Vec<_> = files
            .into_iter()
            .map(
                |file| -> Result<Utf8PlatformPathBuf, (Utf8PlatformPathBuf, io::Error)> {
                    state.push_status(format!("Copying {file} to addons folder"));

                    let target = addons_dir.join(file.file_name().unwrap());
                    fs::copy(&file, &target).map_err(|err| (file, err))?;

                    state.increment_progress();

                    Ok(target)
                },
            )
            .collect();

        let (files, mut errors): (Vec<_>, Vec<_>) = files.into_iter().partition_map(|file| match file {
            Ok(file) => itertools::Either::Left(file),
            Err((path, err)) => itertools::Either::Right((path, LoadError::Sources(addon::Error::Io(err)))),
        });

        if files.is_empty() {
            return (addons, errors);
        }

        state.push_status("Reading sources");

        let sources = Sources::read_paths(files.iter());

        if !sources.failures.is_empty() {
            // TODO: we should present information about addons that failed to load to the user
            eprintln!("There were some errors reading some or all addon sources:");
            for (path, error) in sources.failures {
                eprintln!("  {path}: {error}");
                errors.push((path, error.into()));
            }
        }

        state.increment_progress();

        let extracted_addons: Vec<_> = sources
            .sources
            .into_par_iter()
            .map(|source| {
                state.push_status(format!("Extracting addon {}", source.name().unwrap_or_default()));

                let extracted = source.extract_as_subfolder_in(&extracted_content_dir);

                state.increment_progress();

                extracted.map_err(|err| (source.into_inner(), err))
            })
            .collect();

        let (extracted_addons, mut errors): (Vec<_>, Vec<_>) =
            extracted_addons.into_iter().partition_map(|addon| match addon {
                Ok(addon) => itertools::Either::Left(addon),
                Err((path, err)) => itertools::Either::Right((path, err.into())),
            });

        for addon in extracted_addons {
            state.push_status(format!("Parsing contents of {}", addon.name().unwrap_or_default()));

            let source_path = addon.source_path().to_owned();
            let addon = match addon.parse_content() {
                Ok(parsed_content) => parsed_content,
                Err(err) => {
                    errors.push((source_path, err.into()));
                    continue;
                }
            };

            addons.push(AddonState { enabled: true, addon });

            state.increment_progress();
        }

        state.push_status("Done!");

        // for small addons, this job ends up running too fast - theres no good feedback for the user. So we sleep a bit
        thread::sleep(Duration::from_millis(500));

        (addons, errors)
    });

    (view, handle)
}

pub fn start_addon_install(
    ctx: &egui::Context,
    paths: &Paths,
    config: &Config,
    addons: Vec<AddonState>,
) -> (ProcessView, JoinHandle<anyhow::Result<Vec<AddonState>>>) {
    const TF2_VPK_NAME: &str = "tf2_misc_dir.vpk";

    let (state, view) = ProcessState::with_spinner(ctx);

    let working_vpk_dir = paths.working_vpk.clone();

    let tf_custom_dir = config.tf_dir.join("custom");
    let vpk_path = config.tf_dir.join(TF2_VPK_NAME);
    let game_info_path = config.tf_dir.join("gameinfo.txt");
    let config_path = paths.config.clone();
    let mut config = config.clone();

    let handle = thread::spawn(move || -> anyhow::Result<Vec<AddonState>> {
        state.push_status("Saving updated config");
        for (idx, addon_state) in addons.iter().enumerate() {
            config
                .addons
                .entry(addon_state.addon.name().to_string())
                .and_modify(|addon_config| {
                    addon_config.enabled = addon_state.enabled;
                    addon_config.order = idx;
                })
                .or_insert(AddonConfig {
                    enabled: addon_state.enabled,
                    order: idx,
                });
        }
        config::write_config(&config_path, &config)?;

        state.push_status("Loading particle graph from manifest");
        let mut bins = particles_manifest::bins();
        let vanilla_graphs = particles_manifest::graphs();

        let mut packed_system_names = HashSet::new();
        // N.B. addons that come first in the array need to have priority
        for addon_state in addons.iter().rev() {
            if !addon_state.enabled {
                continue;
            }

            // first we bin-pack our addons' custom particles
            for (path, pcf) in &addon_state.addon.particle_files {
                state.push_status(format!("Bin-packing {}'s {path}", addon_state.addon.name()));
                let graph = pcf.clone().into_connected();
                for mut pcf in graph {
                    packed_system_names.extend(pcf.particle_systems().iter().map(|system| system.name.clone()));
                    bins.pack(&mut pcf).unwrap();
                }
            }

            // then we copy over all non-particle contents to the vpk working directory - which will be packed into
            // _dazzle_addons.vpk later.
            let content_path = &addon_state.addon.content_path;
            for entry in WalkDir::new(content_path).contents_first(false) {
                let entry = entry?;
                let metadata = entry.metadata()?;

                state.push_status(format!(
                    "Copying {}'s {}",
                    addon_state.addon.name(),
                    entry.path().display()
                ));

                let path = paths::to_typed(entry.path()).absolutize()?;
                let new_out_path = working_vpk_dir.join(path.strip_prefix(content_path)?);

                // create the directory before we copy anything over. We guarantee that the directory is iterated first
                // with contents_first(false) earlier
                if metadata.is_dir() {
                    if let Err(err) = fs::create_dir(&new_out_path)
                        && err.kind() != io::ErrorKind::AlreadyExists
                    {
                        return Err(err.into());
                    }
                    continue;
                }

                fs::copy(&path, &new_out_path)?;
            }
        }

        // the bins don't contain any of the necessary particle systems by default, since they're supposed to be a blank
        // slate for our addons; so, we pack every vanilla particle system not present in the bins.
        for (name, graphs) in &vanilla_graphs {
            state.push_status(format!("Bin-packing missing vanilla particle systems from {name}."));

            for graph in graphs {
                if graph
                    .particle_systems()
                    .iter()
                    .any(|system| !packed_system_names.contains(&system.name))
                {
                    let mut pcf = graph.clone();
                    bins.pack(&mut pcf).unwrap();
                }
            }
        }

        // TODO: create quickprecache assets for props & pack them into _dazzle_qpc.vpk

        let mut vpk = VPK::read(vpk_path)?;

        state.push_status("Restoring tf2_misc.vpk");
        for (name, pcf_data) in particles_manifest::PARTICLES_BYTES {
            let mut reader = pcf_data.reader();
            vpk.patch_file(name, pcf_data.len() as u64, &mut reader)?;
        }

        state.push_status("Removing old _dazzle_addons.vpk");
        for entry in fs::read_dir(&tf_custom_dir)? {
            let entry = entry?;
            let path = paths::std_buf_to_typed(entry.path());
            let file_name = path.file_name().unwrap();
            let extension = path.extension().unwrap_or("");
            let is_dazzle = file_name.starts_with("_dazzle_addons")
                && (extension.eq_ignore_ascii_case("vpk") || extension.eq_ignore_ascii_case("cache"));
            let metadata = entry.metadata()?;
            if !metadata.is_file() {
                if is_dazzle {
                    return Err(anyhow!("Unexpected directory or symlink with _dazzle_addons*.vpk name"));
                }
                continue;
            }

            if is_dazzle {
                fs::remove_file(&path)?;
            }
        }

        for bin in bins {
            let (name, pcf) = bin.into_inner();
            state.push_status(format!("Writing tf2_misc.vpk/{name}"));
            let dmx: Dmx = pcf.into();

            let mut writer = BytesMut::new().writer();
            dmx.encode(&mut writer)?;

            let buffer = writer.into_inner();
            let size = buffer.len() as u64;
            let mut reader = buffer.reader();
            vpk.patch_file(&name, size, &mut reader)?;
        }

        // we can finally generate our _dazzle_addons VPKs from our addon contents.
        state.push_status("Packing addons into _dazzle_addons.vpk");
        writevpk::pack::pack_directory(&working_vpk_dir, &tf_custom_dir, "_dazzle_addons", SPLIT_BY_2GB)?;

        // NOTE(dress) after packing everything, cueki does a full-scan of every VPK & file in tf/custom for $ignorez 1 then
        //             replaces each with spaces. This isn't necessary at all, so we just don't do it; anyone can bypass her
        //             code with a modicum of motivation and python knoweledge. Considering how easy it is to remove it from
        //             her preloader, I wouldn't be surprised if I frequently run into people using $ignorez trickfoolery in
        //             pubs.

        // TODO: do some proper gameinfo parsing since this is pretty flakey if the user has modified gameinfo.txt at all
        state.push_status("Writing gameinfo.txt");
        let gameinfo = fs::read_to_string(&game_info_path)?;
        let gameinfo = gameinfo.replace("type multiplayer_only", "type singleplayer_only");
        fs::write(&game_info_path, gameinfo)?;

        // we delete & re-create the working vpk dir to ensure that its empty before copying addons over. If we dont do
        // this, then the contents of the addons from the previous install will still be present.
        state.push_status("Cleaning up working files");
        if let Err(err) = fs::remove_dir_all(&working_vpk_dir)
            && err.kind() != ErrorKind::NotFound
        {
            Err(err)?;
        }

        fs::create_dir(&working_vpk_dir)?;

        state.push_status("Done!");
        thread::sleep(Duration::from_millis(500));

        Ok(addons)
    });

    (view, handle)
}

pub fn start_addon_uninstall(
    ctx: &egui::Context,
    paths: &Paths,
    config: &Config,
    addons: Vec<AddonState>,
) -> (ProcessView, JoinHandle<anyhow::Result<Vec<AddonState>>>) {
    const TF2_VPK_NAME: &str = "tf2_misc_dir.vpk";

    let (state, view) = ProcessState::with_spinner(ctx);

    let working_vpk_dir = paths.working_vpk.clone();

    let tf_custom_dir = config.tf_dir.join("custom");
    let vpk_path = config.tf_dir.join(TF2_VPK_NAME);
    let game_info_path = config.tf_dir.join("gameinfo.txt");
    let config_path = paths.config.clone();
    let mut config = config.clone();

    let handle = thread::spawn(move || -> anyhow::Result<Vec<AddonState>> {
        state.push_status("Saving updated config");
        for (idx, addon_state) in addons.iter().enumerate() {
            config
                .addons
                .entry(addon_state.addon.name().to_string())
                .and_modify(|addon_config| {
                    addon_config.enabled = addon_state.enabled;
                    addon_config.order = idx;
                })
                .or_insert(AddonConfig {
                    enabled: addon_state.enabled,
                    order: idx,
                });
        }
        config::write_config(&config_path, &config)?;

        let mut vpk = VPK::read(vpk_path)?;

        state.push_status("Restoring tf2_misc.vpk");
        for (name, pcf_data) in particles_manifest::PARTICLES_BYTES {
            let mut reader = pcf_data.reader();
            vpk.patch_file(name, pcf_data.len() as u64, &mut reader)?;
        }

        state.push_status("Removing _dazzle_addons.vpk");
        for entry in fs::read_dir(&tf_custom_dir)? {
            let entry = entry?;
            let path = paths::std_buf_to_typed(entry.path());
            let file_name = path.file_name().unwrap();
            let extension = path.extension().unwrap_or("");
            let is_dazzle = file_name.starts_with("_dazzle_addons")
                && (extension.eq_ignore_ascii_case("vpk") || extension.eq_ignore_ascii_case("cache"));
            let metadata = entry.metadata()?;
            if !metadata.is_file() {
                if is_dazzle {
                    return Err(anyhow!("Unexpected directory or symlink with _dazzle_addons*.vpk name"));
                }
                continue;
            }

            if is_dazzle {
                fs::remove_file(&path)?;
            }
        }

        // TODO: remove _dazzle_qpc.vpk

        // TODO: do some proper gameinfo parsing since this is pretty flakey if the user has modified gameinfo.txt at all
        state.push_status("Writing gameinfo.txt");
        let gameinfo = fs::read_to_string(&game_info_path)?;
        let gameinfo = gameinfo.replace("type singleplayer_only", "type multiplayer_only");
        fs::write(&game_info_path, gameinfo)?;

        // we delete & re-create the working vpk dir to ensure that its empty when installing addons again.
        state.push_status("Cleaning up working files");
        if let Err(err) = fs::remove_dir_all(&working_vpk_dir)
            && err.kind() != ErrorKind::NotFound
        {
            Err(err)?;
        }

        fs::create_dir(&working_vpk_dir)?;

        state.push_status("Done!");
        thread::sleep(Duration::from_millis(500));

        Ok(addons)
    });

    (view, handle)
}
