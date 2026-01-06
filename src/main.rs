//! TF2 asset preloader inspired by cueki's casual preloader.
//!
//! It supports these addons:
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
//! This preloader only supports TF2, unlike cueki's which supports TF2 and Goldrush.
//!
//! # Why?
//!
//! Cueki has done a good amount of work creating a usable preloader. My goal is to create a simpler and more
//! performant implementation.
//!
//! I'm also using this as a means to practice more idiomatic Rust.

#![feature(assert_matches)]
#![feature(duration_constructors)]
#![feature(trim_prefix_suffix)]
#![feature(file_buffered)]
#![warn(clippy::pedantic)]

use std::{
    cell::LazyCell,
    collections::{BTreeMap, HashMap},
    ffi::CString,
    fs::{self, File, OpenOptions},
    io::{self, BufReader, BufWriter, Seek, SeekFrom},
    path::{Path, PathBuf},
    process,
    str::FromStr,
};

use directories::ProjectDirs;
use glob::glob;
use ordermap::{OrderMap, OrderSet};
use pcf::Pcf;
use relative_path::RelativePathBuf;
use single_instance::SingleInstance;
use thiserror::Error;
use typed_path::Utf8PlatformPathBuf;
use vpk::VPK;

use crate::addon::Sources;

pub mod addon;

struct App {
    _config_dir: Utf8PlatformPathBuf,
    _config_file: Utf8PlatformPathBuf,
    addons_dir: Utf8PlatformPathBuf,
    extracted_addons_dir: Utf8PlatformPathBuf,
    particles_working_dir: Utf8PlatformPathBuf,
    backup_dir: Utf8PlatformPathBuf,
    vanilla_pcf_paths: Vec<Utf8PlatformPathBuf>,
    pcf_to_particle_system: HashMap<String, Vec<CString>>,
    particle_system_to_pcf: HashMap<CString, String>,
}

mod paths {
    use std::{path::Path, str::Utf8Error};

    use typed_path::{PlatformPath, Utf8PlatformPath};

    pub fn std_to_typed(path: &Path) -> Result<&Utf8PlatformPath, Utf8Error> {
        Utf8PlatformPath::from_bytes_path(PlatformPath::new(path.as_os_str().as_encoded_bytes()))
    }
}

fn main() -> anyhow::Result<()> {
    /*
       TODO: if not already configured, detect/select a tf/ directory
       TODO: tui for configuring enabled/disabled custom particles found in addons
       TODO: tui for selecting addons to install/uninstall
       TODO: detect conflicts in selected addons
    */
    const TF2_VPK_NAME: &str = "tf2_misc_dir.vpk";

    // TODO: single_instance's macos implementation might not be desirable since this program is intended to be portable... maybe we just dont support macos (:
    let instance = SingleInstance::new("net.dresswithpockets.tf2preloader.lock")?;
    if !instance.is_single() {
        eprintln!("There is another instance of tf2-preloader running. Only one instance can run at a time.");
        process::exit(1);
    }

    let tf_dir: Utf8PlatformPathBuf = [
        "/",
        "home",
        "snale",
        ".local",
        "share",
        "Steam",
        "steamapps",
        "common",
        "Team Fortress 2",
        "tf",
    ]
    .iter()
    .collect();

    let Some(project_dirs) = ProjectDirs::from("net", "dresswithpockets", "tf2preloader") else {
        eprintln!(
            "Couldn't retrieve a home directory to store configurations in. Please ensure tf2-preloader can read and write into a $HOME directory."
        );
        process::exit(1);
    };

    let config_dir = project_dirs.config_local_dir();
    if let Err(err) = fs::create_dir_all(config_dir) {
        eprintln!("Couldn't create the config directory: {err}");
        process::exit(1);
    }

    let config_file = config_dir.join("config.toml");
    if let Err(err) = File::create_new(&config_file)
        && err.kind() != io::ErrorKind::AlreadyExists
    {
        eprintln!("Couldn't create config.toml: {err}");
        process::exit(1);
    }

    let working_dir = project_dirs.data_local_dir().join("working");

    let extracted_addons_dir = working_dir.join("extracted");
    if let Err(err) = fs::remove_dir_all(&extracted_addons_dir)
        && err.kind() != io::ErrorKind::NotFound
    {
        eprintln!("Couldn't clear the addon content cache: {err}");
        process::exit(1);
    }

    if let Err(err) = fs::create_dir_all(&extracted_addons_dir) {
        eprintln!("Couldn't create the addon content cache: {err}");
        process::exit(1);
    }

    let particles_working_dir = working_dir.join("particles");
    if let Err(err) = fs::remove_dir_all(&extracted_addons_dir)
        && err.kind() != io::ErrorKind::NotFound
    {
        eprintln!("Couldn't clear the particles working cache: {err}");
        process::exit(1);
    }

    if let Err(err) = fs::create_dir_all(&extracted_addons_dir) {
        eprintln!("Couldn't create the particles working cache: {err}");
        process::exit(1);
    }

    let addons_dir = working_dir.join("addons");
    if let Err(err) = fs::create_dir_all(&addons_dir) {
        eprintln!("Couldn't create the addons directory: {err}");
        process::exit(1);
    }

    let backup_dir = PathBuf::from_str("./backup")?;
    let backup_dir = paths::std_to_typed(&backup_dir)?.to_path_buf();

    let pcf_to_particle_system: HashMap<String, Vec<CString>> =
        serde_json::from_str(include_str!("particle_system_map.json"))?;
    let particle_system_to_pcf: HashMap<CString, String> = pcf_to_particle_system
        .iter()
        .flat_map(|(pcf_path, systems)| systems.iter().map(|system| (system.clone(), pcf_path.clone())))
        .collect();

    let mut vanilla_pcf_paths = Vec::new();
    for path in pcf_to_particle_system.keys() {
        let path = Utf8PlatformPathBuf::from_str(path)?;
        vanilla_pcf_paths.push(path);
    }

    let app = App {
        _config_dir: paths::std_to_typed(config_dir)?.to_path_buf(),
        _config_file: paths::std_to_typed(&config_file)?.to_path_buf(),
        extracted_addons_dir: paths::std_to_typed(&extracted_addons_dir)?.to_path_buf(),
        particles_working_dir: paths::std_to_typed(&particles_working_dir)?.to_path_buf(),
        addons_dir: paths::std_to_typed(&addons_dir)?.to_path_buf(),
        backup_dir,
        vanilla_pcf_paths,
        pcf_to_particle_system,
        particle_system_to_pcf,
    };

    // TODO: detect tf directory
    // TODO: prompt user to verify or provide their own tf directory after discovery attempt

    let vpk_path = tf_dir.join(TF2_VPK_NAME);
    let mut misc_vpk = match VPK::read(vpk_path) {
        Ok(vpk) => vpk,
        Err(err) => {
            eprintln!("Couldn't open tf/tf2_misc_dir.vpk: {err}");
            process::exit(1);
        }
    };

    let sources = match Sources::read_dir(&app.addons_dir) {
        Ok(sources) => sources,
        Err(err) => {
            eprintln!("Couldn't open some addons: {err}");
            process::exit(1);
        }
    };

    for (path, err) in &sources.failures {
        eprintln!(
            "There was an error reading the addon source '{}': {err}",
            path.display()
        );
    }

    // to simplify processing and copying data from addons, we extract it before hand.
    // this means the interface into each addon becomes effectively identical - we can just read/write to them as normal
    // files without modifying the original addon files.
    let mut extracted_addons = Vec::new();
    for source in sources.sources {
        let extracted = match source.extract_as_subfolder_in(&app.extracted_addons_dir) {
            Ok(extracted) => extracted,
            Err(err) => {
                eprintln!("Couldn't extract some mods: {err}");
                process::exit(1);
            }
        };

        extracted_addons.push(extracted);
    }

    let mut addons = Vec::new();
    for addon in extracted_addons {
        let content = match addon.parse_content() {
            Ok(content) => content,
            Err(err) => {
                eprintln!("Couldn't parse content of some mods: {err}");
                process::exit(1);
            }
        };

        addons.push(content);
    }

    // TODO: evaluate the contents of each extracted addon to ensure they're valid
    // TODO: evaluate if there are any conflicting particles in each addon, and warn the user
    //       for now we're just assuming there are no conflicts

    let mut vanilla_particles = HashMap::new();
    for pcf_path in app.pcf_to_particle_system.keys() {
        vanilla_particles.insert(
            pcf_path,
            LazyCell::new(|| -> anyhow::Result<Pcf> {
                let pcf_path = app.backup_dir.join_checked(pcf_path.clone())?;
                let mut reader = File::open_buffered(pcf_path)?;
                Ok(Pcf::decode(&mut reader)?)
            }),
        );
    }

    // create intermediary split-up PCF files by cross referencing our addon PCFs with the particle_system_map.json
    for addon in &addons {
        /*
            in a copy of vanilla tf2, there are many PCFs containing particle system definitions. Except in a couple
            cases, each particle system is only defined once across all PCFs. particle_system_map.json maps the path to
            a PCF to a list of all particle systems defined in that PCF.

            the goal of the following code is to produce new versions of the vanilla PCFs with any modified particle
            system definitions overwritten in each PCF.
        */

        let mut processed_target_pcf_paths: HashMap<&String, Vec<Pcf>> = HashMap::new();
        for (file_path, pcf) in &addon.particle_files {
            // dx80 and dx90 are a special case that we skip over. TODO: i think we generate them later?
            let file_name: &str = file_path.file_name().expect("there should always be a file name");
            if file_name.contains("dx80") || file_name.contains("dx90") {
                continue;
            }

            let Some(definitions_name_idx) = pcf.strings.iter().position(|el| el.0 == c"particleSystemDefinitions")
            else {
                eprintln!(
                    "couldn't find the 'particleSystemDefinitions' string in '{file_name}'. This could mean that the source PCF was malformed. Addon: {}",
                    addon.source_path
                );
                continue;
            };

            #[allow(clippy::cast_possible_truncation)]
            let definitions_name_idx = definitions_name_idx as pcf::NameIndex;

            let mut elements_by_vanilla_pcf_path = HashMap::<&String, OrderSet<&pcf::Element>>::new();
            for element in &pcf.elements {
                let Some(pcf_path) = app.particle_system_to_pcf.get(&element.name) else {
                    continue;
                };

                elements_by_vanilla_pcf_path
                    .entry(pcf_path)
                    .or_default()
                    .insert(element);
            }

            for (target_pcf_path, matched_elements) in elements_by_vanilla_pcf_path {
                // matched_elements contains a subset of the original elements in the pcf. As a result, any
                // Element or ElementArray attributes may not point to the correct index - the order is
                // retained but the indices aren't. So, we need to reindex any references to other elements in the set.
                let mut new_elements = reindex_elements(pcf, matched_elements);

                // the root element always stores an attribute "particleSystemDefinitions" which stores an ElementArray
                // containing the index of every DmeParticleSystemDefinition-type element. We've changed the indices of
                // our particle system definitions, so we need to update the root element's list with the new indices.
                let mut particle_system_indices = Vec::new();
                for (element_idx, element) in new_elements.iter().enumerate().skip(1) {
                    let Some((type_name, ())) = pcf.strings.get_index(element.type_idx as usize) else {
                        continue;
                    };

                    if type_name != c"DmeParticleSystemDefinition" {
                        continue;
                    }

                    #[allow(clippy::cast_possible_truncation)]
                    particle_system_indices.push(element_idx as u32);
                }

                // we've got the new indices now, so we can update the root element's list in-place
                let root_definitions = new_elements[0].attributes.entry(definitions_name_idx).or_default();
                *root_definitions = particle_system_indices.into_boxed_slice().into();

                // this new in-memory PCF has only the elements listed in elements_to_extract, with element references
                // fixed to match any changes in indices.
                let new_pcf = pcf::Pcf::builder()
                    .version(pcf.version)
                    .strings(pcf.strings.iter().map(|el| el.0.clone()).collect())
                    .elements(new_elements)
                    .build();

                processed_target_pcf_paths
                    .entry(target_pcf_path)
                    .or_default()
                    .push(new_pcf);
            }
        }

        // TODO: create lazy-loaded PCFs of the PCF files keyed in particle_system_map.json

        //
        for (target_pcf_path, mut pcf_files) in processed_target_pcf_paths {
            let target_pcf_elements = app
                .pcf_to_particle_system
                .get(target_pcf_path)
                .expect("The target_pcf_path is sourced from the particle system map, so this should never happen");
            let target_pcf_elements: OrderSet<&CString> = target_pcf_elements.iter().collect();

            // TODO: get duplicate elements among pcf_files

            let mut merged_pcf = pcf_files.pop().expect("therre should be at least one pcf in the group");
            for pcf in pcf_files {
                merged_pcf = merged_pcf.merge(pcf).expect("failed to merge pcf");
            }

            // Our merged PCF may be missing some elements in present in the vanilla PCF, so we lazily decode the
            // target vanilla PCF and merge it in.
            let target_pcf = vanilla_particles
                .get(target_pcf_path)
                .expect("The target_pcf_path is sourced from the particle system map, so this should never happen");
            let target_pcf = &**target_pcf;
            let target_pcf = match target_pcf {
                Ok(pcf) => pcf.to_owned(),
                Err(err) => {
                    eprintln!("Error retrieving decoded PCF for a vanilla PCF file: {err}");
                    continue;
                }
            };

            merged_pcf = merged_pcf
                .merge(target_pcf)
                .expect("failed to merge the vanilla PCF into the modified PCF");
        }
    }

    // ensure we start from a consistent state by restoring the particles in the tf misc vpk back to vanilla content.
    if let Err(err) = restore_particles_from_backup(&mut misc_vpk, &app.backup_dir) {
        eprintln!("There was an error restoring some or all particles to the vanilla state: {err}");
        process::exit(1);
    }

    // TODO: query each particle file for all particle systems (DmeParticleSystemDefinition)
    //       then, query each particle system for its material,
    //       then, add material to a list of "required materials" (ignoring vgui/white)
    // TODO: if required materials are provided by the addon, then copy them to the to-be-vpk directory
    // TODO: if addon-provided required materials reference addon-provided textures, then copy those textures to the to-be-vpk directory
    // TODO: merge particle files that were previously split
    // TODO: "fill in missing vanilla elements for reconstructed split files"

    // TODO: figure out how particle_system_map.json is generated. Is it just a map of vanilla PCF paths to named particle system definition elements?

    // TODO: process and patch particles into main VPK, handling duplicate effects

    Ok(())
}

fn reindex_elements<'a>(
    source_pcf: &'a Pcf,
    elements: impl IntoIterator<Item = &'a pcf::Element>,
) -> Vec<pcf::Element> {
    let mut buf = Vec::new();
    reindex_elements_onto_vec(source_pcf, elements, &mut buf);
    buf
}

fn reindex_elements_onto_vec<'a>(
    source_pcf: &'a Pcf,
    elements: impl IntoIterator<Item = &'a pcf::Element>,
    vec: &mut Vec<pcf::Element>,
) {
    let offset = vec.len();

    let mut original_elements: BTreeMap<u32, &pcf::Element> = BTreeMap::new();
    for element in elements {
        let Some(dependent_indices) = source_pcf.get_dependent_indices(&element.name) else {
            continue;
        };

        for idx in dependent_indices {
            let Some(element) = source_pcf.elements.get(idx as usize) else {
                continue;
            };

            original_elements.insert(idx, element);
        }
    }

    #[allow(clippy::cast_possible_truncation)]
    let old_to_new_idx: HashMap<u32, u32> = original_elements
        .iter()
        .enumerate()
        .map(|(new_idx, (old_idx, _))| (*old_idx, (new_idx + offset) as u32))
        .collect();

    for (_, element) in original_elements {
        let mut attributes = OrderMap::new();

        // this monstrosity is re-mapping old element references to new ones using the new indices mapped
        // in old_to_new_idx
        for (name_idx, attribute) in &element.attributes {
            let new_attribute = match attribute {
                pcf::Attribute::Element(old_idx) if *old_idx != u32::MAX => {
                    pcf::Attribute::Element(*old_to_new_idx.get(old_idx).unwrap_or(old_idx))
                }
                pcf::Attribute::ElementArray(old_indices) => pcf::Attribute::ElementArray(
                    old_indices
                        .iter()
                        .map(|old_idx| {
                            if *old_idx == u32::MAX {
                                *old_idx
                            } else {
                                *old_to_new_idx.get(old_idx).unwrap_or(old_idx)
                            }
                        })
                        .collect(),
                ),
                attribute => attribute.clone(),
            };

            attributes.insert(*name_idx, new_attribute);
        }

        vec.push(pcf::Element {
            type_idx: element.type_idx,
            name: element.name.clone(),
            signature: element.signature,
            attributes,
        });
    }
}

fn restore_particles_from_backup(misc_vpk: &mut VPK, backup_dir: impl AsRef<Path>) -> anyhow::Result<()> {
    let backup_dir = backup_dir.as_ref();
    let particles_glob = backup_dir.to_str().expect("this should never happen").to_string() + "/particles/**/*.pcf";
    let backup_particle_paths = glob(&particles_glob)?
        .map(|path| -> anyhow::Result<RelativePathBuf> {
            let mut path: &Path = &path?;
            if path.is_absolute() {
                path = path.strip_prefix(backup_dir)?;
            }

            Ok(RelativePathBuf::from_path(path)?)
        })
        .collect::<anyhow::Result<Vec<RelativePathBuf>>>()?;

    // restore the particles in the misc vpk with our backup, to ensure we're at a clean state
    for particle_file in backup_particle_paths {
        // given ./particles/example.pcf, we should map to:

        //   /particles/example.pcf - the path to the file in the VPK
        let path_in_vpk = particle_file.to_path("/");
        let path_in_vpk = path_in_vpk.to_str().expect("this should never happen");

        //   /path/to/backup/particles/example.pcf - the actual on-disk path of the backup particle file
        let path_on_disk = particle_file.to_path(backup_dir);

        if let Err(err) = patch_file(misc_vpk, path_in_vpk, &path_on_disk) {
            eprintln!("Error patching particle file '{particle_file}': {err}");
        }
    }

    Ok(())
}

#[derive(Debug, Error)]
enum PatchError {
    #[error("file not found in vpk")]
    NotFound,

    #[error("can't patch file that has preload data")]
    HasPreloadData,

    #[error("the input file's size ({0} bytes) does not match the file in the vpk archive ({1} bytes)")]
    MismatchedSizes(u64, u64),

    #[error("only wrote {0} of the expected {1} bytes")]
    PartialWrite(u64, u64),

    #[error(transparent)]
    IoError(#[from] io::Error),
}

/// patches data over an existing entry in the vpk's tree
fn patch_file(vpk: &mut VPK, path_in_vpk: &str, path_on_disk: &Path) -> Result<(), PatchError> {
    let entry = vpk.tree.get(path_in_vpk).ok_or(PatchError::NotFound)?;

    if entry.dir_entry.preload_length > 0 {
        return Err(PatchError::HasPreloadData);
    }

    let Some(archive_path) = &entry.archive_path else {
        return Err(PatchError::HasPreloadData);
    };

    // TODO: what about preload_length? does patch_file need to ever handle preloaded files?
    let entry_size = u64::from(entry.dir_entry.file_length);
    let new_file_size = path_on_disk.symlink_metadata()?.len();

    if entry_size != new_file_size {
        return Err(PatchError::MismatchedSizes(new_file_size, entry_size));
    }

    let new_file = File::open(path_on_disk)?;
    let mut new_file = BufReader::new(new_file);

    let archive_file = OpenOptions::new().write(true).open(archive_path.as_ref())?;
    let mut archive_file = BufWriter::new(archive_file);
    archive_file.seek(SeekFrom::Start(u64::from(entry.dir_entry.archive_offset)))?;

    let copied = io::copy(&mut new_file, &mut archive_file)?;
    if copied != entry_size {
        return Err(PatchError::PartialWrite(copied, entry_size));
    }

    Ok(())
}
