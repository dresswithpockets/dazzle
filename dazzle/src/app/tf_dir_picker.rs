use eframe::egui::{self, Align2, TextEdit, TextStyle, Vec2b};
use faccess::{AccessMode, PathExt};
use std::{
    fs,
    io::{self, ErrorKind},
};
use thiserror::Error;
use typed_path::{Utf8PlatformPath, Utf8PlatformPathBuf};

use crate::styles;

#[derive(Debug)]
pub(crate) struct TfDirPicker {
    picked_dir: String,
    last_error: Option<TfValidationError>,
    new_dir_picked: bool,
}

impl TfDirPicker {
    pub(crate) fn new(picked_dir: String) -> Self {
        Self {
            new_dir_picked: !picked_dir.is_empty(),
            picked_dir,
            last_error: None,
        }
    }

    pub(crate) fn update(&mut self, ctx: &egui::Context, tf_dir: &mut Option<Utf8PlatformPathBuf>) -> bool {
        let mut done = false;
        egui::Window::new("Welcome")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, (0.0, 0.0))
            .max_width(600.0)
            .scroll(Vec2b::FALSE)
            .show(ctx, |ui| {
                ui.vertical(|ui| {

                    ui.horizontal(|ui| {
                        ui.strong(egui::RichText::new("This is dazzle").text_style(styles::big()));
                        ui.add_space(8.0);
                        ui.label(egui::RichText::new(" - a mod installer for Team Fortress 2.").text_style(styles::big()))
                    });

                    ui.add_space(24.0);

                    ui.label(
                        egui::RichText::new("Dazzle will process many kinds of mods & install them into your Team Fortress 2 installation. Mods installed by dazzle will typically work in Casual and other sv_pure servers!")
                            .text_style(styles::big())
                    );

                    ui.add_space(16.0);

                    ui.label(
                        egui::RichText::new("In order to install mods, dazzle needs to know where your TF2 installation is. Please provide a valid path to your 'Team Fortress 2/tf' directory:")
                            .text_style(styles::big())
                    );

                    ui.add_space(16.0);

                    ui.group(|ui| ui.horizontal(|ui| {
                        let changed = TextEdit::singleline(&mut self.picked_dir).desired_width(f32::INFINITY)
                            .font(TextStyle::Monospace)
                            .show(ui)
                            .response.changed();

                        if changed {
                            self.new_dir_picked = true;
                        }

                        if ui.button("Browse").clicked() && let Some(selected_path) = rfd::FileDialog::new().pick_folder() {
                            self.picked_dir = selected_path.into_os_string().to_string_lossy().into_owned();
                            self.new_dir_picked = true;
                        }
                    }));

                    if self.new_dir_picked {
                        let path = Utf8PlatformPath::new(&self.picked_dir);
                        match validate(path) {
                            Ok(()) => {
                                *tf_dir = Some(path.to_owned());
                                self.last_error = None;
                            },
                            Err(err) => {
                                *tf_dir = None;
                                self.last_error = Some(err);
                            },
                        }
                    }

                    if let Some(err) = &self.last_error {
                        ui.group(|ui| {
                            ui.take_available_width();
                            ui.horizontal(|ui| {
                                ui.image(egui::include_image!("../static/images/warning.png"));
                                ui.strong(format!("the selected path is not valid: {err}"));
                            })
                        });
                    }

                    ui.vertical_centered(|ui| ui.add_enabled_ui(tf_dir.is_some(), |ui| {
                        if ui.button("Lets go!").clicked() {
                            done = true;
                        }
                    }));
                });
            });

        done
    }
}

#[derive(Debug, Error)]
pub(crate) enum TfValidationError {
    #[error("The path contains invalid characters")]
    InvalidPath,

    #[error("The path specified doesnt exist")]
    DoesntExist,

    #[error("The path specified is not a directory")]
    NotADirectory,

    #[error("We lack the permissions to read that directory")]
    PermissionDenied,

    #[error(transparent)]
    Io(#[from] io::Error),

    #[error("Couldn't find a 'custom/' subfolder in the path specified")]
    MissingCustomFolder,

    #[error("'custom' exists but it is not a directory")]
    CustomNotADirectory,

    #[error("The 'custom/' subfolder exists but we lack permissions to read or write to it")]
    MissingCustomFolderPermissions,

    #[error("Couldn't find 'tf2_misc_dir.vpk' in the path specified")]
    MissingVpk,

    #[error("'tf2_misc_dir.vpk' exists but it is not a file")]
    VpkNotAFile,

    #[error("The 'tf2_misc_dir.vpk' file exists but we lack permissions to read or write to it")]
    MissingVpkPermissions,

    #[error("Couldn't find 'gameinfo.txt' in the path specified")]
    MissingGameInfo,

    #[error("'gameinfo.txt' exists but it is not a file")]
    GameInfoNotAFile,

    #[error("The 'gameinfo.txt' file exists but we lack permissions to read or write to it")]
    MissingGameInfoPermissions,
}

pub(crate) fn validate(path: &Utf8PlatformPath) -> Result<(), TfValidationError> {
    // the picked directory must be a valid tf2 installation. We have the following heuristics to
    // ensure that this is the case:
    //   - {picked_dir}/tf2_misc_dir.vpk exists, is a file, is a valid VPK index, and we have read/write permissions
    //   - {picked_dir}/custom exists, and is a dir, and we have read/write permissions
    //   - {picked_dir}/gameinfo.txt exists, and is a file, and we have read/write permissions

    if !path.is_valid() {
        return Err(TfValidationError::InvalidPath);
    }

    let metadata = fs::metadata(path).map_err(|err| match err.kind() {
        ErrorKind::NotFound => TfValidationError::DoesntExist,
        ErrorKind::PermissionDenied => TfValidationError::PermissionDenied,
        _ => TfValidationError::Io(err),
    })?;

    if !metadata.is_dir() {
        return Err(TfValidationError::NotADirectory);
    }

    let custom_dir = path.join("custom");
    let metadata = fs::metadata(&custom_dir).map_err(|err| match err.kind() {
        ErrorKind::NotFound => TfValidationError::MissingCustomFolder,
        ErrorKind::PermissionDenied => TfValidationError::MissingCustomFolderPermissions,
        _ => TfValidationError::Io(err),
    })?;

    if !metadata.is_dir() {
        return Err(TfValidationError::CustomNotADirectory);
    }

    if custom_dir.access(AccessMode::READ | AccessMode::WRITE).is_err() {
        return Err(TfValidationError::MissingCustomFolderPermissions);
    }

    let tf2_misc_vpk = path.join("tf2_misc_dir.vpk");
    let metadata = fs::metadata(&tf2_misc_vpk).map_err(|err| match err.kind() {
        ErrorKind::NotFound => TfValidationError::MissingVpk,
        ErrorKind::PermissionDenied => TfValidationError::MissingVpkPermissions,
        _ => TfValidationError::Io(err),
    })?;

    if !metadata.is_file() {
        return Err(TfValidationError::VpkNotAFile);
    }

    if tf2_misc_vpk.access(AccessMode::READ | AccessMode::WRITE).is_err() {
        return Err(TfValidationError::MissingVpkPermissions);
    }

    let gameinfo_path = path.join("gameinfo.txt");
    let metadata = fs::metadata(&gameinfo_path).map_err(|err| match err.kind() {
        ErrorKind::NotFound => TfValidationError::MissingGameInfo,
        ErrorKind::PermissionDenied => TfValidationError::MissingGameInfoPermissions,
        _ => TfValidationError::Io(err),
    })?;

    if !metadata.is_file() {
        return Err(TfValidationError::GameInfoNotAFile);
    }

    if gameinfo_path.access(AccessMode::READ | AccessMode::WRITE).is_err() {
        return Err(TfValidationError::MissingVpkPermissions);
    }

    Ok(())
}
