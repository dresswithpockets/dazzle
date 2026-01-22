use std::str::FromStr;
use std::{collections::HashMap, ffi::CString};
use std::{fs, io};

use directories::ProjectDirs;
use single_instance::SingleInstance;
use thiserror::Error;
use typed_path::{Utf8PlatformPath, Utf8PlatformPathBuf, Utf8UnixPathBuf};

use super::TF2_VPK_NAME;

use vpk::VPK;

use super::PARTICLE_SYSTEM_MAP;

use nanoserde::DeJson;

use super::paths;

use super::APP_NAME;

use super::APP_ORG;

use super::APP_TLD;

use super::APP_INSTANCE_NAME;

#[derive(Debug)]
pub struct App {
    pub addons_dir: Utf8PlatformPathBuf,
    pub extracted_content_dir: Utf8PlatformPathBuf,
    pub backup_dir: Utf8PlatformPathBuf,
    pub working_vpk_dir: Utf8PlatformPathBuf,
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

#[derive(Default)]
pub(crate) struct AppBuilder;

impl AppBuilder {
    pub(crate) fn new() -> Self {
        Self
    }

    pub(crate) fn create_single_instance() -> Result<SingleInstance, BuildError> {
        // TODO: single_instance's macos implementation might not be desirable since this program is intended to be portable... maybe we just dont support macos (:
        let instance = SingleInstance::new(APP_INSTANCE_NAME)?;
        if instance.is_single() {
            Ok(instance)
        } else {
            Err(BuildError::MultipleInstances)
        }
    }

    pub(crate) fn create_project_dirs() -> Result<ProjectDirs, BuildError> {
        ProjectDirs::from(APP_TLD, APP_ORG, APP_NAME).ok_or(BuildError::NoValidHomeDirectory)
    }

    pub(crate) fn get_data_dir(dirs: &ProjectDirs) -> Utf8PlatformPathBuf {
        let working_dir = dirs.data_local_dir();
        paths::to_typed(&working_dir).into_owned()
    }

    pub(crate) fn get_config_path(dirs: &ProjectDirs) -> Utf8PlatformPathBuf {
        let working_dir = dirs.config_local_dir().join("config.toml");
        paths::to_typed(&working_dir).into_owned()
    }

    pub(crate) fn create_new_content_cache_dir(dir: &Utf8PlatformPath) -> Result<Utf8PlatformPathBuf, BuildError> {
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

    pub(crate) fn create_new_working_vpk_dir(dir: &Utf8PlatformPath) -> Result<Utf8PlatformPathBuf, BuildError> {
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

    pub(crate) fn create_addons_dir(dir: &Utf8PlatformPath) -> Result<Utf8PlatformPathBuf, BuildError> {
        let addons_dir = dir.join("addons");
        fs::create_dir_all(&addons_dir).map_err(BuildError::CantCreateAddonsDirectory)?;
        Ok(addons_dir)
    }

    pub(crate) fn get_backup_dir() -> Result<Utf8PlatformPathBuf, BuildError> {
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

    pub(crate) fn get_vanilla_pcf_map() -> HashMap<String, Vec<CString>> {
        DeJson::deserialize_json(PARTICLE_SYSTEM_MAP).expect("the PARTICLE_SYSTEM_MAP should always be valid JSON")
    }

    pub(crate) fn build(self) -> Result<App, BuildError> {
        _ = Self::create_single_instance()?;

        let project_dirs = Self::create_project_dirs()?;
        let data_dir = Self::get_data_dir(&project_dirs);
        let extracted_content_dir = Self::create_new_content_cache_dir(&data_dir)?;
        let working_vpk_dir = Self::create_new_working_vpk_dir(&data_dir)?;
        let addons_dir = Self::create_addons_dir(&data_dir)?;
        let backup_dir = Self::get_backup_dir()?;

        Ok(App {
            addons_dir,
            extracted_content_dir,
            backup_dir,
            working_vpk_dir,
        })
    }
}
