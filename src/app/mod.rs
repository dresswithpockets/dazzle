mod addon_manager;
mod initial_load;
mod process;
mod tf_dir_picker;

use std::{collections::HashMap, env, ffi::CString, fs, io, mem, str::FromStr, thread::JoinHandle};

use directories::ProjectDirs;
use eframe::egui::{self, CentralPanel, Layout, Window};
use egui_extras::{Column, TableBuilder};
use nanoserde::DeJson;
use pcf::Pcf;
use single_instance::SingleInstance;
use thiserror::Error;
use typed_path::{Utf8PlatformPath, Utf8PlatformPathBuf};

use crate::{
    app::{
        addon_manager::Manager,
        initial_load::{LoadError, LoadedData},
        process::ProcessView,
    },
    packing::PcfBinMap,
};
use addon::Addon;
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
        bins: PcfBinMap,
        vanilla_graphs: Vec<(String, Vec<Pcf>)>,
        manager: Manager,
    },

    /// We're processing all of their addons and installing them!
    /// Will always transitioin to [`State::ChoosingAddons`].
    Installing,

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
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        CentralPanel::default().show(ctx, |ui| {
            let state = mem::replace(&mut self.state, State::Intermediate);
            let _ = mem::replace(
                &mut self.state,
                match state {
                    State::InitialTfDir { mut tf_dir, mut picker } => {
                        if picker.update(ui.ctx(), &mut tf_dir) {
                            let (load_view, job_handle) =
                                initial_load::start_initial_load(ui.ctx(), self.paths.clone());

                            State::InitialLoad {
                                tf_dir: tf_dir.unwrap(),
                                load_view,
                                job_handle,
                            }
                        } else {
                            State::InitialTfDir { tf_dir, picker }
                        }
                    }
                    State::InitialLoad {
                        tf_dir,
                        mut load_view,
                        job_handle,
                    } => {
                        load_view.show("vanilla pcf and addon loading", ui.ctx());
                        if job_handle.is_finished() {
                            // TODO: present errors to the user as a modal
                            let data = job_handle.join().unwrap().unwrap();
                            State::ManagingAddons {
                                tf_dir,
                                bins: data.bins,
                                vanilla_graphs: data.vanilla_graphs,
                                manager: Manager::new(data.addons),
                            }
                        } else {
                            State::InitialLoad {
                                tf_dir,
                                load_view,
                                job_handle,
                            }
                        }
                    }
                    State::ManagingAddons {
                        tf_dir,
                        bins,
                        vanilla_graphs,
                        mut manager,
                    } => {
                        manager.show(ui);

                        State::ManagingAddons {
                            tf_dir,
                            bins,
                            vanilla_graphs,
                            manager,
                        }
                    }
                    State::Installing => State::Installing,
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
