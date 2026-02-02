//! This crate is a collection of utilities for interop with studiomdl - Valve's proprietary 3D model compiler.
//!
//! ## Wine Dependency on Linux!
//!
//! Unfourtunately there are no known distributions of studiomdl which target linux natively. In order to
//! operate studiomdl on linux, wine must be installed.

#![feature(exit_status_error)]

use std::{fs, io::{self, ErrorKind}, process::{self, Command}};

use faccess::PathExt;
use thiserror::Error;
use typed_path::{Utf8PlatformPath, Utf8PlatformPathBuf};

#[derive(Debug, Error)]
pub enum ToolsPathError {
    #[error("`studiomdl.exe` does not exist in the tools path given")]
    NotFound,

    #[error("the tools path exists but it cant be read from")]
    NotReadable,

    #[error("`studiomdl.exe` exists but it is not a file")]
    NotAFile,

    #[error("`studiomdl.exe` exists at the path given but it cant be read from executed.")]
    NotExecutable,

    #[error("the tools path cant be verified due to an io error")]
    Io(#[from] io::Error),
}

/// Validates that `path` is a valid tools path for executing studiomdl.
pub fn validate_tools_path(path: &Utf8PlatformPath) -> Result<(), ToolsPathError> {
    let studiomdl_path = path.join("studiomdl.exe");
    let metadata = fs::metadata(&studiomdl_path)
        .map_err(|err| match err.kind() {
            ErrorKind::NotFound => ToolsPathError::NotFound,
            ErrorKind::PermissionDenied => ToolsPathError::NotReadable,
            _ => err.into(),
        })?;
    
    if !metadata.is_file() {
        return Err(ToolsPathError::NotAFile)
    }

    if !studiomdl_path.executable() {
        return Err(ToolsPathError::NotExecutable)
    }

    Ok(())
}


#[derive(Debug, Error)]
pub enum GameDirError {
    #[error("`studiomdl.exe` does not exist in the tools path given")]
    NotFound,

    #[error("the tools path exists but it cant be read from")]
    NotReadable,

    #[error("`studiomdl.exe` exists but it is not a file")]
    NotAFile,

    #[error("`studiomdl.exe` exists at the path given but it cant be read from executed.")]
    NotExecutable,

    #[error("the tools path cant be verified due to an io error")]
    Io(#[from] io::Error),
}

pub fn validate_game_dir(path: &Utf8PlatformPath) -> Result<(), GameDirError> {
    todo!();
}

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    ToolsPath(#[from] ToolsPathError),

    #[error(transparent)]
    GameDir(#[from] GameDirError),

    #[error("couldn't execute studiomdl.exe due to an io error")]
    Io(#[from] io::Error),

    #[error("studiomdl.exe returned an error")]
    Status(#[from] process::ExitStatusError),
}

pub struct Runner {
    game_dir: Utf8PlatformPathBuf,
    studiomdl_path: Utf8PlatformPathBuf,
}

impl Runner {
    pub fn new(studiomdl_path: Utf8PlatformPathBuf, game_dir: Utf8PlatformPathBuf) -> Result<Self, Error> {
        validate_tools_path(&studiomdl_path)?;
        validate_game_dir(&studiomdl_path)?;
        Ok(Self {
            studiomdl_path,
            game_dir,
        })
    }

    #[cfg(target_os = "windows")]
    pub fn build_simple(model_path: &Utf8PlatformPath) -> Result<Self, Error> {
        todo!();
    }
    
    #[cfg(target_os = "linux")]
    pub fn build_simple(&self, model_path: &Utf8PlatformPath) -> Result<Utf8PlatformPathBuf, Error> {
        let output = Command::new("wine")
            .current_dir(self.game_dir.as_str())
            .arg(self.studiomdl_path.as_str())
            .arg("-nop4")
            .arg("-verbose")
            .arg(format!("Z:{model_path}"))
            .output()
            .map_err(|err| match err.kind() {
                ErrorKind::NotFound => ToolsPathError::NotFound.into(),
                ErrorKind::PermissionDenied => ToolsPathError::NotExecutable.into(),
                _ => Error::Io(err)
            })?;
        
        output.exit_ok()?;

        Ok(model_path.with_extension("mdl"))
    }
}

#[cfg(target_os = "windows")]
fn get_platform_default_sdk_path() -> String {
    match env::var("PROGRAMFILES(X86)") {
        Ok(programfiles) => {
            let mut path = Utf8PlatformPathBuf::from(programfiles);
            path.extend(["Steam", "steamapps", "common", "Source SDK Base 2013 Multiplayer", "bin"]);

            match path.absolutize() {
                Ok(path) => path.into_string(),
                Err(_) => String::default(),
            }
        }
        Err(_) => String::default(),
    }
}

#[cfg(target_os = "linux")]
fn get_platform_default_sdk_path() -> String {
    use std::env;
    use typed_path::Utf8PlatformPathBuf;

    match env::var("HOME") {
        Ok(home) if !home.is_empty() => {
            let mut path = Utf8PlatformPathBuf::from(home);
            path.extend([
                ".local",
                "share",
                "Steam",
                "steamapps",
                "common",
                "Source SDK Base 2013 Multiplayer",
                "bin",
            ]);

            match path.absolutize() {
                Ok(path) => path.into_string(),
                Err(_) => String::default(),
            }
        }
        _ => String::default(),
    }
}
