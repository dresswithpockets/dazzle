//! TF2 asset preloader based on and compatible with cueki's casual preloader.
//!
//! It supports these mods:
//!
//! - Particles
//! - Models
//! - Animations
//! - VGUI elements
//! - Lightwarps
//! - Skyboxes
//! - Warpaints
//! - Game sounds
//!
//! # Why?
//!
//! Cueki has done a good amount of work creating a usable preloader. The goal is to create a simpler and more 
//! performant implementation.
//!
//! I'm also using this as a means to practice more idiomatic Rust.

#![feature(assert_matches)]
#![feature(duration_constructors)]
#![warn(clippy::pedantic)]

use std::{ffi::OsStr, fs::{self, File, OpenOptions}, io::{self, BufReader, BufWriter, Seek, SeekFrom}, path::{Path, PathBuf}, process};

use directories::ProjectDirs;
use thiserror::Error;
use vpk::VPK;

struct App<'a> {
    config_dir: &'a Path,
    config_file: &'a Path,
    mods_dir: &'a Path,
    backup_dir: &'a Path,
}

fn main() {
    /*
       TODO: on first-run establish an application folder for configuration & storing unprocessed mods
       TODO: if not already configured, detect/select a tf/ directory
       TODO: tui for configuring enabled/disabled custom particles found in addons
       TODO: tui for selecting addons to install/uninstall
       TODO: detect conflicts in selected addons
       TODO: process addons and pack into a custom VPK
    */

    /*
     technical work:
       TODO: port PCK parser
       TODO: port VPK parser

       General technical process:
           - more...
           - patches tf_misc_dir.vpk with particles
           - patches hud overrides
           - generates VMTs
           - creates a _QuickPrecache.vpk for precached map props
           - generates a w/config.cfg for execution at launch (preloading, etc)
           - packs processed mods into custom vpk
    */

    // starting out, we're going to get custom particles working

    const TF2_VPK_NAME: &str = "tf2_misc_dir.vpk";
    let tf_dir: PathBuf = PathBuf::from("/home/snale/.local/share/Steam/steamapps/common/Team Fortress 2/tf/");

    let Some(project_dirs) = ProjectDirs::from("net", "dresswithpockets", "tf2preloader") else {
        eprintln!("Couldn't retrieve a home directory to store configurations in. Please ensure tf2-preloader can read and write into a $HOME directory.");
        process::exit(1);
    };

    let config_dir = project_dirs.config_local_dir();
    if let Err(err) = fs::create_dir_all(config_dir) {
        eprintln!("Couldn't create the config directory: {err}");
        process::exit(1);
    }

    let config_file = config_dir.join("config.toml");
    if let Err(err) = File::create_new(&config_file) && err.kind() != io::ErrorKind::AlreadyExists {
        eprintln!("Couldn't create config.toml: {err}");
        process::exit(1);
    }

    let data_dir = project_dirs.data_local_dir();

    let mods_dir = data_dir.join("mods");
    if let Err(err) = fs::create_dir_all(&mods_dir) {
        eprintln!("Couldn't create the mods directory: {err}");
        process::exit(1);
    }

    let backup_dir = data_dir.join("backup");
    if let Err(err) = fs::create_dir_all(&backup_dir) {
        eprintln!("Couldn't create the backup directory: {err}");
        process::exit(1);
    }

    let app = App {
        config_dir,
        config_file: &config_file,
        mods_dir: &mods_dir,
        backup_dir: &backup_dir,
    };

    // TODO: detect tf directory
    // TODO: prompt user to verify or provide their own tf directory after discovery attempt

    let vpk_path = tf_dir.join(TF2_VPK_NAME);
    let mut vpk = match VPK::read(vpk_path) {
        Ok(vpk) => vpk,
        Err(err) => {
            eprintln!("Couldn't open tf/tf2_misc_dir.vpk: {err}");
            process::exit(1);
        },
    };

    // To patch particles, we have to modify the game's VPKs containing the particles;
    // to guarantee we're always starting from a valid state, we rely on a backup of every file we're patching.
    // TODO: "restore" particle files. backup_particle_files should be a list of paths to particle files in our backup folder
    let backup_particle_files: Vec<&Path> = Vec::new();

    for particle_file in backup_particle_files {
        let filename = particle_file.file_name().and_then(OsStr::to_str).expect("missing filename in particle file path");
        let path = format!("particles/{filename}");
        if let Err(err) = patch_file(&mut vpk, &path, particle_file) {
            eprintln!("Error patching particle file '{}': {err}", particle_file.display());
        }
    }
}

#[derive(Debug, Error)]
pub enum PatchError {
    #[error("file not found in vpk")]
    NotFound,

    #[error("can't patch file that has preload data")]
    HasPreloadData,

    #[error("the input file's size ({0} bytes) does not match the file in the vpk archive ({1} bytes)")]
    MismatchedSizes(u64, u64),

    #[error("only wrote {0} of the expected {1} bytes")]
    PartialWrite(u64, u64),

    #[error(transparent)]
    IoError(#[from] io::Error)
}

/// patches data over an existing entry in the vpk's tree
/// 
fn patch_file(vpk: &mut VPK, path: &str, new_file: &Path) -> Result<(), PatchError> {
    let entry = vpk.tree.get(path).ok_or(PatchError::NotFound)?;
    
    if entry.dir_entry.preload_length > 0 {
        return Err(PatchError::HasPreloadData);
    }

    let Some(archive_path) = &entry.archive_path else {
        return Err(PatchError::HasPreloadData);
    };

    // TODO: what about preload_length? does patch_file need to ever handle preloaded files?
    let entry_size = u64::from(entry.dir_entry.file_length);
    let new_file_size = new_file.symlink_metadata()?.len();
    
    if entry_size != new_file_size {
        return Err(PatchError::MismatchedSizes(new_file_size, entry_size));
    }

    let new_file = File::open(new_file)?;
    let mut new_file = BufReader::new(new_file);

    let archive_file = OpenOptions::new().write(true).open(archive_path.as_ref())?;
    let mut archive_file = BufWriter::new(archive_file);
    archive_file.seek(SeekFrom::Start(u64::from(entry.dir_entry.archive_offset)))?;

    let copied = io::copy(&mut new_file, &mut archive_file)?;
    if copied != entry_size {
        return Err(PatchError::PartialWrite(copied, entry_size))
    }

    Ok(())
}
