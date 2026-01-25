mod addon_manager;
mod initial_load;
mod process;
mod tf_dir_picker;

use std::{collections::HashMap, env, ffi::CString, fs, io, mem, str::FromStr, thread::JoinHandle};

use directories::ProjectDirs;
use eframe::egui::{self, CentralPanel, Id, Modal, Sides};
use nanoserde::DeJson;
use pcf::Pcf;
use single_instance::SingleInstance;
use thiserror::Error;
use typed_path::{Utf8PlatformPath, Utf8PlatformPathBuf};

use crate::app::{
    addon_manager::AddonState,
    initial_load::{LoadError, LoadedData},
    process::ProcessView,
};
use tf_dir_picker::TfDirPicker;

use super::{APP_INSTANCE_NAME, APP_NAME, APP_ORG, APP_TLD, PARTICLE_SYSTEM_MAP};

#[derive(Debug, Clone)]
pub(crate) struct Paths {
    pub addons_dir: Utf8PlatformPathBuf,
    pub extracted_content_dir: Utf8PlatformPathBuf,
    pub backup_dir: Utf8PlatformPathBuf,
    pub working_vpk_dir: Utf8PlatformPathBuf,
}

#[derive(Debug)]
pub(crate) enum State {
    /// The user has launched for the first time is choosing a valid tf/ directory
    /// Will always transition to [`State::InitialLoad`].
    InitialTfDir {
        tf_dir: Option<Utf8PlatformPathBuf>,
        picker: TfDirPicker,
    },

    /// We're loading vanilla PCFs & all addons in their addons directory. Doing so allows us to ensure each addon is
    /// valid, and to evaluate conflicts between addons.
    /// Will always transition to [`State::ChoosingAddons`].
    InitialLoad {
        tf_dir: Utf8PlatformPathBuf,
        load_view: ProcessView,
        job_handle: JoinHandle<Result<LoadedData, LoadError>>,
    },

    /// The user is picking which addons to enable/disable, and re-ordering their load priority.
    /// Will always transition to [`State::Installing`].
    ManagingAddons {
        tf_dir: Utf8PlatformPathBuf,
        bins: Vec<pcfpack::Bin>,
        vanilla_graphs: Vec<(String, Vec<Pcf>)>,
        addons: Vec<AddonState>,
        confirming_delete: Option<usize>,
    },

    /// The user has decided to delete an addon's contents and remove it from the list.
    /// Will always transition to [`State::ManagingAddons`]
    RemovingAddon {
        tf_dir: Utf8PlatformPathBuf,
        bins: Vec<pcfpack::Bin>,
        vanilla_graphs: Vec<(String, Vec<Pcf>)>,
        addons: Vec<AddonState>,
        remove_view: ProcessView,
        job_handle: JoinHandle<Result<(), io::Error>>,
    },

    /// The user has selected a new addon to be added to the list
    /// Will always transition to [`State::ManagingAddons`].
    AddingAddon {
        tf_dir: Utf8PlatformPathBuf,
        bins: Vec<pcfpack::Bin>,
        vanilla_graphs: Vec<(String, Vec<Pcf>)>,
        addons: Vec<AddonState>,
    },

    /// We're processing all of their addons and installing them!
    /// Will always transition to [`State::ManagingAddons`].
    Installing,

    /// We're restoring tf2_misc.vpk, removing _dazzle_addons.vpk, and removing _dazzle_qpc.vpk
    /// Will always transition to [`State::ManagingAddons`].
    Uninstalling,

    /// An intermediate value used as the enum's default when using helpers like [`std::mem::take`] and [`std::mem::replace`]
    Intermediate,
}

#[derive(Debug)]
pub(crate) struct App {
    paths: Paths,
    state: State,
}

impl App {
    pub(crate) fn new() -> Result<Self, BuildError> {
        _ = create_single_instance()?;

        let project_dirs = create_project_dirs()?;
        let data_dir = get_data_dir(&project_dirs);
        let extracted_content_dir = create_new_content_cache_dir(&data_dir)?;
        let working_vpk_dir = create_new_working_vpk_dir(&data_dir)?;
        let addons_dir = create_addons_dir(&data_dir)?;
        let backup_dir = get_backup_dir()?;

        let tf_dir = get_default_platform_tf_dir();

        Ok(Self {
            paths: Paths {
                addons_dir,
                extracted_content_dir,
                backup_dir,
                working_vpk_dir,
            },
            state: State::InitialTfDir {
                tf_dir: None,
                picker: TfDirPicker::new(tf_dir),
            },
        })
    }

    fn update_state_initial_tf_dir(
        ui: &mut egui::Ui,
        paths: Paths,
        mut tf_dir: Option<Utf8PlatformPathBuf>,
        mut picker: TfDirPicker,
    ) -> State {
        if picker.update(ui.ctx(), &mut tf_dir) {
            let (load_view, job_handle) = initial_load::start_initial_load(ui.ctx(), paths);

            State::InitialLoad {
                tf_dir: tf_dir.unwrap(),
                load_view,
                job_handle,
            }
        } else {
            State::InitialTfDir { tf_dir, picker }
        }
    }

    fn update_state_initial_load(
        ui: &mut egui::Ui,
        tf_dir: Utf8PlatformPathBuf,
        mut load_view: ProcessView,
        job_handle: JoinHandle<Result<LoadedData, LoadError>>,
    ) -> State {
        load_view.show("vanilla pcf and addon loading", ui.ctx());
        if job_handle.is_finished() {
            // TODO: present errors to the user as a modal
            let data = job_handle.join().unwrap().unwrap();
            State::ManagingAddons {
                tf_dir,
                bins: data.bins,
                vanilla_graphs: data.vanilla_graphs,
                addons: data
                    .addons
                    .into_iter()
                    .map(|addon| AddonState { enabled: true, addon })
                    .collect(),
                confirming_delete: None,
            }
        } else {
            State::InitialLoad {
                tf_dir,
                load_view,
                job_handle,
            }
        }
    }

    fn update_state_managing_addons(
        ui: &mut egui::Ui,
        tf_dir: Utf8PlatformPathBuf,
        bins: Vec<pcfpack::Bin>,
        vanilla_graphs: Vec<(String, Vec<Pcf>)>,
        mut addons: Vec<AddonState>,
        mut confirming_delete: Option<usize>,
    ) -> State {
        let response = addon_manager::addons_manager(ui, &mut addons);
        let mut delete_confirmed = None;

        if let Some(delete_idx) = confirming_delete {
            let modal = Modal::new(Id::new("Confirm Addon Deletion")).show(ui.ctx(), |ui| {
                ui.set_width(500.0);
                ui.heading("Are you sure?");
                ui.add_space(16.0);
                ui.strong(format!(
                    "You're about to permanently delete '{}'. Please confirm:",
                    addons.get(delete_idx).unwrap().addon.name()
                ));
                ui.add_space(16.0);
                Sides::new().show(
                    ui,
                    |_ui| {},
                    |ui| {
                        if ui.button("Delete It!").clicked() {
                            delete_confirmed = Some(delete_idx);
                            ui.close();
                        }

                        if ui.button("No! Stop that!").clicked() {
                            ui.close();
                        }
                    },
                )
            });

            if modal.should_close() {
                confirming_delete = None;
            }
        } else if let Some(action) = response.action {
            match action {
                addon_manager::Action::OpenAddonsFolder => todo!(),
                // TODO: after adding the selected addon, refresh all of our other addons to ensure we're up to date
                addon_manager::Action::AddAddon => todo!(),
                // TODO: detect if any of the addons have been changed since load, and ask user for confirmation if they have been
                // TODO: show installation confirmation modal, then transition accordingly
                addon_manager::Action::InstallAddons => todo!(),
                // TODO: show confirmation modal, then transition accordingly
                addon_manager::Action::UninstallAddons => todo!(),
                addon_manager::Action::DeleteAddon(delete_idx) => confirming_delete = Some(delete_idx),
            }
        }

        // the user confirmed that they want to delete the addon association with this index, so we
        // should start the delete process & transition to the delete state.
        if let Some(remove_idx) = delete_confirmed {
            let addon = addons.remove(remove_idx);
            let (remove_view, job_handle) = addon_manager::start_addon_removal(ui.ctx(), addon.addon);

            State::RemovingAddon {
                tf_dir,
                bins,
                vanilla_graphs,
                addons,
                remove_view,
                job_handle,
            }
        } else {
            State::ManagingAddons {
                tf_dir,
                bins,
                vanilla_graphs,
                addons,
                confirming_delete,
            }
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        CentralPanel::default().show(ctx, |ui| {
            let state = mem::replace(&mut self.state, State::Intermediate);
            let _ = mem::replace(
                &mut self.state,
                match state {
                    State::InitialTfDir { tf_dir, picker } => {
                        Self::update_state_initial_tf_dir(ui, self.paths.clone(), tf_dir, picker)
                    }
                    State::InitialLoad {
                        tf_dir,
                        load_view,
                        job_handle,
                    } => Self::update_state_initial_load(ui, tf_dir, load_view, job_handle),
                    State::ManagingAddons {
                        tf_dir,
                        bins,
                        vanilla_graphs,
                        addons,
                        confirming_delete,
                    } => {
                        Self::update_state_managing_addons(ui, tf_dir, bins, vanilla_graphs, addons, confirming_delete)
                    }
                    State::RemovingAddon {
                        tf_dir,
                        bins,
                        vanilla_graphs,
                        addons,
                        mut remove_view,
                        job_handle,
                    } => {
                        remove_view.show("removing addon contents", ui.ctx());
                        if job_handle.is_finished() {
                            // TODO: present errors to the user as a modal
                            job_handle.join().unwrap().unwrap();
                            State::ManagingAddons {
                                tf_dir,
                                bins,
                                vanilla_graphs,
                                addons,
                                confirming_delete: None,
                            }
                        } else {
                            State::RemovingAddon {
                                tf_dir,
                                bins,
                                vanilla_graphs,
                                addons,
                                remove_view,
                                job_handle,
                            }
                        }
                    }
                    State::AddingAddon {
                        tf_dir,
                        bins,
                        vanilla_graphs,
                        addons,
                    } => State::AddingAddon {
                        tf_dir,
                        bins,
                        vanilla_graphs,
                        addons,
                    },
                    State::Installing => State::Installing,
                    State::Uninstalling => State::Uninstalling,
                    State::Intermediate => panic!("under no circumstances should state be Intermediate in the matcher"),
                },
            );
        });
    }
}

#[derive(Debug, Error)]
pub(crate) enum BuildError {
    #[error("couldn't verify that there is only a single instance of dazzle running, due to an internal error")]
    CantInitSingleInstance(#[from] single_instance::error::SingleInstanceError),

    #[error("there are multiple instances of dazzle running")]
    MultipleInstances,

    #[error("couldn't find a valid home directory, which is necessary for some operations")]
    NoValidHomeDirectory,

    #[error("couldn't clear the addon content cache, due to an IO error")]
    CantClearContentCache(io::Error),

    #[error("couldn't create the addon content cache, due to an IO error")]
    CantCreateContentCache(io::Error),

    #[error("couldn't clear the working VPK directory, due to an IO error")]
    CantClearWorkingVpkDirectory(io::Error),

    #[error("couldn't create the working VPK directory, due to an IO error")]
    CantCreateWorkingVpkDirectory(io::Error),

    #[error("couldn't create the addons directory, due to an IO error")]
    CantCreateAddonsDirectory(io::Error),

    #[error("couldn't find the backup assets directory")]
    MissingBackupDirectory,

    #[error("couldn't find the backup assets directory, due to an IO error")]
    IoBackupDirectory(io::Error),
}

#[cfg(target_os = "windows")]
fn get_default_platform_tf_dir() -> String {
    match env::var("PROGRAMFILES(X86)") {
        Ok(programfiles) => {
            let mut path = Utf8PlatformPathBuf::from(programfiles);
            path.extend(["Steam", "steamapps", "common", "Team Fortress 2", "tf"]);

            match path.absolutize() {
                Ok(path) => path.into_string(),
                Err(_) => String::default(),
            }
        }
        Err(_) => String::default(),
    }
}

#[cfg(target_os = "linux")]
fn get_default_platform_tf_dir() -> String {
    match env::var("HOME") {
        Ok(home) => {
            let mut path = Utf8PlatformPathBuf::from(home);
            path.extend([
                ".local",
                "share",
                "Steam",
                "steamapps",
                "common",
                "Team Fortress 2",
                "tf",
            ]);

            match path.absolutize() {
                Ok(path) => path.into_string(),
                Err(_) => String::default(),
            }
        }
        Err(_) => String::default(),
    }
}

fn create_single_instance() -> Result<SingleInstance, BuildError> {
    // NB: single_instance's macos implementation might not be desirable since this program is intended to be portable... maybe we just dont support macos (:
    let instance = SingleInstance::new(APP_INSTANCE_NAME)?;
    if instance.is_single() {
        Ok(instance)
    } else {
        Err(BuildError::MultipleInstances)
    }
}

fn create_project_dirs() -> Result<ProjectDirs, BuildError> {
    ProjectDirs::from(APP_TLD, APP_ORG, APP_NAME).ok_or(BuildError::NoValidHomeDirectory)
}

fn get_data_dir(dirs: &ProjectDirs) -> Utf8PlatformPathBuf {
    let working_dir = dirs.data_local_dir();
    paths::to_typed(&working_dir).into_owned()
}

fn get_config_path(dirs: &ProjectDirs) -> Utf8PlatformPathBuf {
    let working_dir = dirs.config_local_dir().join("config.toml");
    paths::to_typed(&working_dir).into_owned()
}

fn create_new_content_cache_dir(dir: &Utf8PlatformPath) -> Result<Utf8PlatformPathBuf, BuildError> {
    let extracted_addons_dir = dir.join("extracted");
    if let Err(err) = fs::remove_dir_all(&extracted_addons_dir)
        && err.kind() != io::ErrorKind::NotFound
    {
        Err(BuildError::CantClearContentCache(err))
    } else {
        fs::create_dir_all(&extracted_addons_dir).map_err(BuildError::CantCreateContentCache)?;
        Ok(extracted_addons_dir)
    }
}

fn create_new_working_vpk_dir(dir: &Utf8PlatformPath) -> Result<Utf8PlatformPathBuf, BuildError> {
    let working_vpk_dir = dir.join("vpk");
    if let Err(err) = fs::remove_dir_all(&working_vpk_dir)
        && err.kind() != io::ErrorKind::NotFound
    {
        Err(BuildError::CantClearWorkingVpkDirectory(err))
    } else {
        fs::create_dir_all(&working_vpk_dir).map_err(BuildError::CantCreateWorkingVpkDirectory)?;
        Ok(working_vpk_dir)
    }
}

fn create_addons_dir(dir: &Utf8PlatformPath) -> Result<Utf8PlatformPathBuf, BuildError> {
    let addons_dir = dir.join("addons");
    fs::create_dir_all(&addons_dir).map_err(BuildError::CantCreateAddonsDirectory)?;
    Ok(addons_dir)
}

fn get_backup_dir() -> Result<Utf8PlatformPathBuf, BuildError> {
    let backup_dir = Utf8PlatformPathBuf::from_str("./backup")
        .expect("from_str should always succeed with this path")
        .absolutize()
        .map_err(BuildError::IoBackupDirectory)?;

    let metadata = fs::metadata(&backup_dir).map_err(|err| {
        if err.kind() == io::ErrorKind::NotFound {
            BuildError::MissingBackupDirectory
        } else {
            BuildError::IoBackupDirectory(err)
        }
    })?;

    if metadata.is_dir() {
        Ok(backup_dir)
    } else {
        Err(BuildError::MissingBackupDirectory)
    }
}

fn get_vanilla_pcf_map() -> HashMap<String, Vec<CString>> {
    DeJson::deserialize_json(PARTICLE_SYSTEM_MAP).expect("the PARTICLE_SYSTEM_MAP should always be valid JSON")
}
