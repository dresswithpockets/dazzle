mod addon_manager;
mod config;
mod file_explorer;
mod initial_load;
mod process;
mod tf_dir_picker;

use std::{env, fs, io, mem, thread::JoinHandle};

use addon::Addon;
use derive_more::From;
use directories::ProjectDirs;
use eframe::egui::{self, CentralPanel, Id, Modal, Sides};
use rfd::FileDialog;
use single_instance::SingleInstance;
use thiserror::Error;
use typed_path::{Utf8PlatformPath, Utf8PlatformPathBuf};

use crate::app::{
    addon_manager::{Action, AddingAddonsJob, AddonState, RemovingAddonJob},
    config::{Config, Error},
    initial_load::InitialLoadJob,
    process::ProcessView,
};
use tf_dir_picker::TfDirPicker;

use super::{APP_INSTANCE_NAME, APP_NAME, APP_ORG, APP_TLD};

#[derive(Debug, Clone)]
pub(crate) struct Paths {
    pub addons: Utf8PlatformPathBuf,
    pub extracted_content: Utf8PlatformPathBuf,
    pub working_vpk: Utf8PlatformPathBuf,
    pub config: Utf8PlatformPathBuf,
}

pub trait HandleState {
    fn handle(self, ui: &mut egui::Ui, app: &mut App) -> State;
}

#[derive(Debug)]
pub(crate) struct Launch {
    config: Config,
}

impl Launch {
    pub fn new(config: Config) -> Self {
        Self { config }
    }
}

impl HandleState for Launch {
    fn handle(self, ui: &mut egui::Ui, app: &mut App) -> State {
        if self.config.tf_dir.as_str().is_empty() {
            ConfiguringTfDir::new(self.config, get_default_platform_tf_dir()).into()
        } else if tf_dir_picker::validate(&self.config.tf_dir).is_err() {
            let tf_dir = self.config.tf_dir.to_string();
            ConfiguringTfDir::new(self.config, tf_dir).into()
        } else {
            InitialLoad::new(self.config, ui.ctx(), &app.paths).into()
        }
    }
}

#[derive(Debug)]
pub(crate) struct ConfiguringTfDir {
    config: Config,
    picker: TfDirPicker,
}

impl ConfiguringTfDir {
    pub fn new(config: Config, tf_path: String) -> Self {
        let picker = TfDirPicker::new(tf_path);
        Self {
            config,
            picker,
        }
    }
}

impl HandleState for ConfiguringTfDir {
    fn handle(mut self, ui: &mut egui::Ui, app: &mut App) -> State {
        let mut tf_dir = if self.config.tf_dir.as_str().is_empty() {
            None
        } else {
            Some(self.config.tf_dir)
        };

        if self.picker.update(ui.ctx(), &mut tf_dir) {
            let config = Config {
                tf_dir: tf_dir.unwrap(),
                ..self.config
            };

            // TODO: present errors to the user as a modal
            config::write_config(&app.paths.config, &config).unwrap();

            InitialLoad::new(config, ui.ctx(), &app.paths).into()
        } else {
            Self {
                config: Config {
                    tf_dir: tf_dir.unwrap(),
                    ..self.config
                },
                picker: self.picker,
            }.into()
        }
    }
}

#[derive(Debug)]
pub(crate) struct InitialLoad {
    config: Config,
    view: ProcessView,
    job: InitialLoadJob,
}

impl InitialLoad {
    pub fn new(config: Config, ctx: &egui::Context, paths: &Paths) -> Self {
        let (view, job) = initial_load::start_initial_load(ctx, paths);

        Self {
            config,
            view,
            job,
        }
    }
}

impl HandleState for InitialLoad {
    fn handle(mut self, ui: &mut egui::Ui, _app: &mut App) -> State {
        self.view.show("vanilla pcf and addon loading", ui.ctx());

        if self.job.is_finished() {
            // TODO: present errors to the user as a modal
            let addons = self.job.join().unwrap().unwrap();
            let mut addons: Vec<_> = addons
                .into_iter()
                .map(|addon| (self.config.addons.get(addon.name()).copied().unwrap_or_default(), addon))
                .collect();

            addons.sort_by_key(|(config, _)| config.order);

            let addons = addons.into_iter()
                    .map(|(config, addon)| AddonState {
                        enabled: config.enabled,
                        addon,
                    })
                    .collect();

            ManagingAddons::new(self.config, addons).into()
        } else {
            self.into()
        }
    }
}

#[derive(Debug)]
enum ManagingAddonsState {
    Managing,
    ConfirmingInstall,
    ConfirmingUninstall,
    ConfirmingDelete(usize),
}

#[derive(Debug)]
pub(crate) struct ManagingAddons {
    config: Config,
    addons: Vec<AddonState>,
    state: ManagingAddonsState,
}

impl ManagingAddons {
    pub fn new(config: Config, addons: Vec<AddonState>) -> Self {
        Self {
            config,
            addons,
            state: ManagingAddonsState::Managing,
        }
    }

    fn handle_add_addon_files(self, ui: &mut egui::Ui, app: &mut App) -> State {
        match FileDialog::new().add_filter("Addon", &["vpk"]).pick_files() {
            Some(files) if !files.is_empty() => {
                let files = files.into_iter().map(paths::std_buf_to_typed).collect();

                AddingAddons::new(self.config, self.addons, files, ui.ctx(), app).into()
            }
            _ => self.into(),
        }
    }

    fn handle_add_addon_folders(self, ui: &mut egui::Ui, app: &mut App) -> State {
        match FileDialog::new().pick_folders() {
            Some(files) if !files.is_empty() => {
                let files = files.into_iter().map(paths::std_buf_to_typed).collect();

                AddingAddons::new(self.config, self.addons, files, ui.ctx(), app).into()
            }
            _ => self.into(),
        }
    }

    #[allow(clippy::needless_pass_by_value)]
    fn handle_action(self, action: Action, ui: &mut egui::Ui, app: &mut App) -> State {
        match action {
            Action::OpenAddonsFolder => {
                file_explorer::open_file_explorer(&app.paths.addons);
                self.into()
            }
            Action::OpenTfFolder => {
                file_explorer::open_file_explorer(&self.config.tf_dir);
                self.into()
            }
            // TODO: after adding the selected addon, refresh all of our other addons to ensure we're up to date
            Action::AddAddonFiles => self.handle_add_addon_files(ui, app),
            Action::AddAddonFolders => self.handle_add_addon_folders(ui, app),
            // TODO: detect if any of the addons have been changed since load, and ask user for confirmation if they have been
            // TODO: show installation confirmation modal, then transition accordingly
            Action::InstallAddons => Self {
                state: ManagingAddonsState::ConfirmingInstall,
                ..self
            }.into(),
            // TODO: show confirmation modal, then transition accordingly
            Action::UninstallAddons => Self {
                state: ManagingAddonsState::ConfirmingUninstall,
                ..self
            }.into(),
            Action::DeleteAddon(delete_idx) => Self {
                state: ManagingAddonsState::ConfirmingDelete(delete_idx),
                ..self
            }.into(),
        }
    }

    fn handle_substate(self, ui: &mut egui::Ui, app: &mut App) -> State {
        match self.state {
            ManagingAddonsState::Managing => self.into(),
            ManagingAddonsState::ConfirmingInstall => self.handle_confirming_install(ui, app),
            ManagingAddonsState::ConfirmingUninstall => self.handle_confirming_uninstall(ui, app),
            ManagingAddonsState::ConfirmingDelete(delete_idx) => self.handle_confirming_delete(ui, delete_idx),
        }
    }

    fn handle_confirming_install(self, ui: &mut egui::Ui, app: &mut App) -> State {
        let mut install_confirmed = false;
        let modal = Modal::new(Id::new("Confirm Addon Installation")).show(ui.ctx(), |ui| {
            ui.set_width(500.0);
            ui.heading("Are you sure?");
            ui.add_space(16.0);
            ui.strong("You're about to install the addons as you've configured them. Doing so will override any addons you've installed via dazzle.");
            ui.add_space(16.0);
            Sides::new().show(
                ui,
                |_ui| {},
                |ui| {
                    if ui.button("No! Stop that!").clicked() {
                        ui.close();
                    }

                    if ui.button("Yes, install!").clicked() {
                        install_confirmed = true;
                        ui.close();
                    }
                },
            )
        });

        if install_confirmed {
            // the user confirmed that they want to install their addons
            Installing::new(self.config, self.addons, ui.ctx(), app).into()
        } else if modal.should_close() {
            Self {
                state: ManagingAddonsState::Managing,
                ..self
            }.into()
        } else {
            self.into()
        }
    }

    fn handle_confirming_uninstall(self, ui: &mut egui::Ui, app: &mut App) -> State {
        let mut uninstall_confirmed = false;
        let modal = Modal::new(Id::new("Confirm Addon Uninstallation")).show(ui.ctx(), |ui| {
            ui.set_width(500.0);
            ui.heading("Are you sure?");
            ui.add_space(16.0);
            ui.strong("You're about to uninstall any addons you've previously installed via dazzle.");
            ui.add_space(16.0);
            Sides::new().show(
                ui,
                |_ui| {},
                |ui| {
                    if ui.button("No! Stop that!").clicked() {
                        ui.close();
                    }

                    if ui.button("Yes, uninstall!").clicked() {
                        uninstall_confirmed = true;
                        ui.close();
                    }
                },
            )
        });

        if uninstall_confirmed {
            // the user confirmed that they want to install their addons
            Uninstalling::new(self.config, self.addons, ui.ctx(), app).into()
        } else if modal.should_close() {
            Self {
                state: ManagingAddonsState::Managing,
                ..self
            }.into()
        } else {
            self.into()
        }
    }

    fn handle_confirming_delete(mut self, ui: &mut egui::Ui, delete_idx: usize) -> State {
        let mut delete_confirmed = false;
        let modal = Modal::new(Id::new("Confirm Addon Deletion")).show(ui.ctx(), |ui| {
            ui.set_width(500.0);
            ui.heading("Are you sure?");
            ui.add_space(16.0);
            ui.strong(format!(
                "You're about to permanently delete '{}'. Please confirm:",
                self.addons.get(delete_idx).unwrap().addon.name()
            ));
            ui.add_space(16.0);
            Sides::new().show(
                ui,
                |_ui| {},
                |ui| {
                    if ui.button("Delete It!").clicked() {
                        delete_confirmed = true;
                        ui.close();
                    }

                    if ui.button("No! Stop that!").clicked() {
                        ui.close();
                    }
                },
            )
        });

        if delete_confirmed {
            // the user confirmed that they want to delete the addon association with this index, so we
            // should start the delete process & transition to the delete state.
            let addon = self.addons.remove(delete_idx);

            RemovingAddon::new(self.config, self.addons, ui.ctx(), addon.addon).into()
        } else if modal.should_close() {
            Self {
                state: ManagingAddonsState::Managing,
                ..self
            }.into()
        } else {
            self.into()
        }
    }
}

impl HandleState for ManagingAddons {
    fn handle(mut self, ui: &mut egui::Ui, app: &mut App) -> State {
        addon_manager::addons_manager(ui, &mut self.addons);

        if let Some(action) = addon_manager::addons_manager(ui, &mut self.addons).action {
            self.handle_action(action, ui, app)
        } else {
            self.handle_substate(ui, app)
        }
    }
}

#[derive(Debug)]
pub(crate) struct RemovingAddon {
    config: Config,
    addons: Vec<AddonState>,
    view: ProcessView,
    job: RemovingAddonJob,
}

impl RemovingAddon {
    pub fn new(config: Config, addons: Vec<AddonState>, ctx: &egui::Context, addon: Addon) -> Self {
        let (view, job) = addon_manager::start_addon_removal(ctx, addon);

        Self {
            config,
            addons,
            view,
            job,
        }
    }
}

impl HandleState for RemovingAddon {
    fn handle(mut self, ui: &mut egui::Ui, _app: &mut App) -> State {
        self.view.show("removing addon contents", ui.ctx());
        if self.job.is_finished() {
            // TODO: present job errors to the user as a modal
            self.job.join().unwrap().unwrap();
            ManagingAddons::new(self.config, self.addons).into()
        } else {
            self.into()
        }
    }
}

#[derive(Debug)]
pub(crate) struct AddingAddons {
    config: Config,
    view: ProcessView,
    job: AddingAddonsJob,
}

impl AddingAddons {
    pub fn new(config: Config, addons: Vec<AddonState>, files: Vec<Utf8PlatformPathBuf>, ctx: &egui::Context, app: &App) -> Self {
        let (view, job) = addon_manager::start_addon_add(ctx, &app.paths, addons, files);

        Self {
            config,
            view,
            job,
        }
    }
}

impl HandleState for AddingAddons {
    fn handle(mut self, ui: &mut egui::Ui, _app: &mut App) -> State {
        self.view.show("adding addons", ui.ctx());
        if self.job.is_finished() {
            // TODO: present job errors to the user as a modal
            let result = self.job.join().unwrap();
            for (path, err) in result.1 {
                eprintln!("There was an error loading {path}: {err}");
            }

            ManagingAddons::new(self.config, result.0).into()
        } else {
            self.into()
        }
    }
}

#[derive(Debug)]
pub(crate) struct Installing {
    config: Config,
    view: ProcessView,
    job: JoinHandle<anyhow::Result<Vec<AddonState>>>,
}

impl Installing {
    pub fn new(config: Config, addons: Vec<AddonState>, ctx: &egui::Context, app: &App) -> Self {
        let (view, job) = addon_manager::start_addon_install(ctx, &app.paths, &config, addons);

        Self {
            config,
            view,
            job,
        }
    }
}

impl HandleState for Installing {
    fn handle(mut self, ui: &mut egui::Ui, _app: &mut App) -> State {
        self.view.show("installing addons", ui.ctx());

        if self.job.is_finished() {
            // TODO: present job errors to the user as a modal
            let addons = self.job.join().unwrap().unwrap();
            ManagingAddons::new(self.config, addons).into()
        } else {
            self.into()
        }
    }
}

#[derive(Debug)]
pub(crate) struct Uninstalling {
    config: Config,
    view: ProcessView,
    job: JoinHandle<anyhow::Result<Vec<AddonState>>>,
}

impl Uninstalling {
    pub fn new(config: Config, addons: Vec<AddonState>, ctx: &egui::Context, app: &App) -> Self {
        let (view, job) = addon_manager::start_addon_uninstall(ctx, &app.paths, &config, addons);

        Self {
            config,
            view,
            job,
        }
    }
}

impl HandleState for Uninstalling {
    fn handle(mut self, ui: &mut egui::Ui, _app: &mut App) -> State {
        self.view.show("installing addons", ui.ctx());

        if self.job.is_finished() {
            // TODO: present job errors to the user as a modal
            let addons = self.job.join().unwrap().unwrap();
            ManagingAddons::new(self.config, addons).into()
        } else {
            self.into()
        }
    }
}

#[derive(Debug, From)]
pub(crate) enum State {
    Launch(Launch),

    /// The user has launched for the first time is choosing a valid tf/ directory
    /// Will always transition to [`State::InitialLoad`].
    ConfiguringTfDir(ConfiguringTfDir),

    /// We're loading vanilla PCFs & all addons in their addons directory. Doing so allows us to ensure each addon is
    /// valid, and to evaluate conflicts between addons.
    /// Will always transition to [`State::ChoosingAddons`].
    InitialLoad(InitialLoad),

    /// The user is picking which addons to enable/disable, and re-ordering their load priority.
    /// Will always transition to [`State::Installing`].
    ManagingAddons(ManagingAddons),

    /// The user has decided to delete an addon's contents and remove it from the list.
    /// Will always transition to [`State::ManagingAddons`]
    RemovingAddon(RemovingAddon),

    /// The user has selected a new addon to be added to the list
    /// Will always transition to [`State::ManagingAddons`].
    AddingAddons(AddingAddons),

    /// We're processing all of their addons and installing them!
    /// Will always transition to [`State::ManagingAddons`].
    Installing(Installing),

    #[allow(clippy::doc_markdown)]
    /// We're restoring tf2_misc.vpk, removing _dazzle_addons.vpk, and removing _dazzle_qpc.vpk
    /// Will always transition to [`State::ManagingAddons`].
    Uninstalling(Uninstalling),

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
        let config_path = get_config_path(&project_dirs);
        let config = config::create_or_read_config(&config_path)?;

        Ok(Self {
            paths: Paths {
                addons: addons_dir,
                extracted_content: extracted_content_dir,
                working_vpk: working_vpk_dir,
                config: config_path,
            },
            state: Launch::new(config).into(),
        })
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        CentralPanel::default().show(ctx, |ui| {
            let state = match mem::replace(&mut self.state, State::Intermediate) {
                State::Launch(launch) => launch.handle(ui, self),
                State::ConfiguringTfDir(configuring_tf_dir) => configuring_tf_dir.handle(ui, self),
                State::InitialLoad(initial_load) => initial_load.handle(ui, self),
                State::ManagingAddons(managing_addons) => managing_addons.handle(ui, self),
                State::RemovingAddon(removing_addon) => removing_addon.handle(ui, self),
                State::AddingAddons(adding_addons) => adding_addons.handle(ui, self),
                State::Installing(installing) => installing.handle(ui, self),
                State::Uninstalling(uninstalling) => uninstalling.handle(ui, self),
                State::Intermediate => panic!("under no circumstances should state be Intermediate in the matcher"),
            };

            self.state = state;
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

    #[error(transparent)]
    Config(#[from] Error),
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
    paths::to_typed(working_dir).into_owned()
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
