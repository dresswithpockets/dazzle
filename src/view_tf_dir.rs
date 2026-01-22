use eframe::egui::{self, Align2, TextEdit, TextStyle, Vec2b};
use thiserror::Error;
use std::{fs, io::{self, ErrorKind}};
use typed_path::{Utf8PlatformPath, Utf8PlatformPathBuf};

use crate::styles;

pub(crate) struct TfDirPicker<'a> {
    output: &'a mut Option<Utf8PlatformPathBuf>,
    picked_dir: String,
    last_error: Option<TfValidationError>,
}

impl<'a> TfDirPicker<'a> {
    pub(crate) fn new(cc: &eframe::CreationContext<'_>, output: &'a mut Option<Utf8PlatformPathBuf>, picked_dir: String) -> Self {
        styles::configure_fonts(&cc.egui_ctx);
        styles::configure_text_styles(&cc.egui_ctx);

        let mut last_error = None;

        if !picked_dir.is_empty() {
            let path = Utf8PlatformPath::new(&picked_dir);
            match validate(path) {
                Ok(()) => *output = Some(path.to_owned()),
                Err(err) => last_error = Some(err),
            }
        }

        Self {
            output,
            picked_dir,
            last_error,
        }
    }
}

impl eframe::App for TfDirPicker<'_> {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::Window::new("tf dir picker")
                .title_bar(false)
                .collapsible(false)
                .resizable(false)
                .anchor(Align2::CENTER_CENTER, (0.0, 0.0))
                .max_width(600.0)
                .scroll(Vec2b::FALSE)
                .show(ui.ctx(), |ui| {
                    ui.vertical(|ui| {
                        ui.label(
                            egui::RichText::new("Dazzle handles installing mods into your Team Fortress 2 installation. Please select a valid path to your game's \"tf\" directory.")
                                .text_style(styles::big())
                        );

                        let mut new_dir_picked = false;
                        ui.group(|ui| ui.horizontal(|ui| {
                            new_dir_picked = TextEdit::singleline(&mut self.picked_dir).desired_width(f32::INFINITY)
                                .font(TextStyle::Monospace)
                                .show(ui)
                                .response.changed();

                            if ui.button("Browse").clicked() && let Some(selected_path) = rfd::FileDialog::new().pick_folder() {
                                self.picked_dir = selected_path.into_os_string().to_string_lossy().into_owned();
                                new_dir_picked = true;
                            }
                        }));

                        if new_dir_picked {
                            let path = Utf8PlatformPath::new(&self.picked_dir);
                            match validate(path) {
                                Ok(()) => {
                                    *self.output = Some(path.to_owned());
                                    self.last_error = None;
                                },
                                Err(err) => {
                                    *self.output = None;
                                    self.last_error = Some(err);
                                },
                            }
                        }

                        if let Some(err) = &self.last_error {
                            ui.group(|ui| {
                                ui.take_available_width();
                                ui.horizontal(|ui| {
                                    ui.image(egui::include_image!("static/images/warning.png"));
                                    ui.strong(format!("the selected path is not valid: {err}"));
                                })
                            });
                        }

                        ui.vertical_centered(|ui| ui.add_enabled_ui(self.output.is_some(), |ui| {
                            if ui.button("Lets go!").clicked() {
                                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                            }
                        }));
                    });
                });
        });
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
}

pub(crate) fn validate(path: &Utf8PlatformPath) -> Result<(), TfValidationError> { 
    // the picked directory must be a valid tf2 installation. We have the following heuristics to 
    // ensure that this is the case:
    //   - {picked_dir}/tf2_misc_dir.vpk exists, is a file, is a valid VPK index, and we have read/write permissions
    //   - {picked_dir}/custom exists, and is a dir, and we have read/write permissions

    if !path.is_valid() {
        return Err(TfValidationError::InvalidPath);
    }

    let metadata = fs::metadata(path).map_err(|err| 
        match err.kind() {
            ErrorKind::NotFound => TfValidationError::DoesntExist,
            ErrorKind::PermissionDenied => TfValidationError::PermissionDenied,
            _ => TfValidationError::Io(err),
        }
    )?;

    if !metadata.is_dir() {
        return Err(TfValidationError::NotADirectory);
    }

    let custom_dir = path.join("custom");
    let metadata = fs::metadata(&custom_dir).map_err(|err| 
        match err.kind() {
            ErrorKind::NotFound => TfValidationError::MissingCustomFolder,
            ErrorKind::PermissionDenied => TfValidationError::MissingCustomFolderPermissions,
            _ => TfValidationError::Io(err),
        }
    )?;

    if !metadata.is_dir() {
        return Err(TfValidationError::CustomNotADirectory);
    }

    let readable = permissions::is_readable(&custom_dir)
        .map_err(|err| {
            match err.kind() {
                ErrorKind::NotFound => TfValidationError::MissingCustomFolder,
                ErrorKind::PermissionDenied => TfValidationError::MissingCustomFolderPermissions,
                _ => TfValidationError::Io(err),
            }
        })?;

    if !readable {
        return Err(TfValidationError::MissingCustomFolderPermissions);
    }

    let writable = permissions::is_writable(&custom_dir)
        .map_err(|err| {
            match err.kind() {
                ErrorKind::NotFound => TfValidationError::MissingCustomFolder,
                ErrorKind::PermissionDenied => TfValidationError::MissingCustomFolderPermissions,
                _ => TfValidationError::Io(err),
            }
        })?;

    if !writable {
        return Err(TfValidationError::MissingCustomFolderPermissions);
    }

    let tf2_misc_vpk = path.join("tf2_misc_dir.vpk");
    let metadata = fs::metadata(&tf2_misc_vpk).map_err(|err| 
        match err.kind() {
            ErrorKind::NotFound => TfValidationError::MissingVpk,
            ErrorKind::PermissionDenied => TfValidationError::MissingVpkPermissions,
            _ => TfValidationError::Io(err),
        }
    )?;

    if !metadata.is_file() {
        return Err(TfValidationError::VpkNotAFile);
    }

    let readable = permissions::is_readable(&tf2_misc_vpk)
        .map_err(|err| {
            match err.kind() {
                ErrorKind::NotFound => TfValidationError::MissingVpk,
                ErrorKind::PermissionDenied => TfValidationError::MissingVpkPermissions,
                _ => TfValidationError::Io(err),
            }
        })?;

    if !readable {
        return Err(TfValidationError::MissingVpkPermissions);
    }

    let writable = permissions::is_writable(&tf2_misc_vpk)
        .map_err(|err| {
            match err.kind() {
                ErrorKind::NotFound => TfValidationError::MissingVpk,
                ErrorKind::PermissionDenied => TfValidationError::MissingVpkPermissions,
                _ => TfValidationError::Io(err),
            }
        })?;

    if !writable {
        return Err(TfValidationError::MissingVpkPermissions);
    }

    Ok(())
}
