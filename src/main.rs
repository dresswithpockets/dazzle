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
#![feature(cstr_display)]
#![warn(clippy::pedantic)]
#![feature(push_mut)]

pub mod addon;
pub mod patch;
mod vpk_writer;

use std::{
    collections::{BTreeMap, HashMap},
    ffi::{CStr, CString},
    fs::{self, File, copy},
    io::{self},
    process,
    str::FromStr,
};

use bytes::{Buf, BufMut, BytesMut};
use directories::ProjectDirs;
use nanoserde::DeJson;
use ordermap::OrderMap;
use pcf::{Attribute, ElementsExt, Pcf, attribute::{Color, Vector3}, index::ElementIdx};
use single_instance::SingleInstance;
use thiserror::Error;
use typed_path::{Utf8PlatformPath, Utf8PlatformPathBuf, Utf8UnixPathBuf};
use vpk::VPK;

use crate::patch::PatchVpkExt;
use crate::addon::{Addon, Sources};

const SPLIT_BY_2GB: u32 = 2 << 30;

struct App {
    addons_dir: Utf8PlatformPathBuf,
    extracted_content_dir: Utf8PlatformPathBuf,
    backup_dir: Utf8PlatformPathBuf,
    working_vpk_dir: Utf8PlatformPathBuf,

    vanilla_pcf_paths: Vec<Utf8PlatformPathBuf>,
    vanilla_pcf_to_systems: HashMap<String, Vec<CString>>,
    vanilla_system_to_pcf: HashMap<CString, String>,

    tf_misc_vpk: VPK,
    tf_custom_dir: Utf8PlatformPathBuf,
}

impl App {
    fn merge_addon_particles(
        &self,
        particle_files: &HashMap<Utf8PlatformPathBuf, pcf::Pcf>,
    ) -> HashMap<String, Vec<Pcf>> {
        let mut processed_target_pcf_paths: HashMap<String, Vec<Pcf>> = HashMap::new();
        for (file_path, pcf) in particle_files {
            println!("merging systems from {file_path}");
            // dx80 and dx90 are a special case that we skip over. TODO: i think we generate them later?
            let file_name: &str = file_path.file_name().expect("there should always be a file name");
            if file_name.contains("dx80") || file_name.contains("dx90") {
                continue;
            }

            // grouping the elements from our addon by the vanilla PCF they're mapped to in particle_system_map.json.
            let mut systems_by_vanilla_pcf_path = HashMap::<&String, OrderMap<&CString, &pcf::Element>>::new();
            println!("  has {} elements", pcf.elements().len());
            for element in pcf.elements() {
                let Some(pcf_path) = self.vanilla_system_to_pcf.get(&element.name) else {
                    continue;
                };

                println!("  discovered element '{}' in map", element.name.display());

                // we're also ridding ourselves of duplicate particle systems here. The first one always takes priority,
                // subsequent particle systems with the same name are skipped entirely.
                systems_by_vanilla_pcf_path
                    .entry(pcf_path)
                    .or_default()
                    .entry(&element.name)
                    .or_insert(element);
            }

            for (target_pcf_path, matched_systems) in systems_by_vanilla_pcf_path {
                println!("reindexing discovered elements");
                // matched_elements contains a subset of the original elements in the pcf. As a result, any
                // Element or ElementArray attributes may not point to the correct index - the order is
                // retained but the indices aren't. So, we need to reindex any references to other elements in the set.
                let new_elements = Self::reindex_elements(pcf, matched_systems.into_values());

                // the root element always stores an attribute "particleSystemDefinitions" which stores an ElementArray
                // containing the index of every DmeParticleSystemDefinition-type element. We've changed the indices of
                // our particle system definitions, so we need to update the root element's list with the new indices.
                let particle_system_indices: Vec<ElementIdx> = new_elements
                    .iter()
                    .map_particle_system_indices(&pcf.strings().particle_system_definition_type_idx)
                    .collect();

                // our filtered `new_elements` only contains particle systems, it does not contain a root element
                let root = pcf::Root {
                    type_idx: pcf.root().type_idx,
                    name: pcf.root().name.clone(),
                    signature: pcf.root().signature,
                    definitions: particle_system_indices.into_boxed_slice(),
                    attributes: pcf.root().attributes.clone(), // TODO: do we need to reindex these?
                };

                // this new in-memory PCF has only the elements listed in elements_to_extract, with element references
                // fixed to match any changes in indices.
                let new_pcf = pcf::Pcf::builder()
                    .version(pcf.version())
                    .strings(pcf.strings().clone())
                    .root(root)
                    .elements(new_elements)
                    .build();

                processed_target_pcf_paths
                    .entry(target_pcf_path.clone())
                    .or_default()
                    .push(new_pcf);
            }
        }

        processed_target_pcf_paths
    }

    fn reindex_elements<'a>(
        source_pcf: &'a Pcf,
        systems: impl IntoIterator<Item = &'a pcf::Element>,
    ) -> Vec<pcf::Element> {
        let mut new_elements = Vec::new();
        let mut original_elements: BTreeMap<ElementIdx, &pcf::Element> = BTreeMap::new();
        for system in systems {
            let system_idx = source_pcf.get_element_index(&system.name).expect("this should never fail");
            let dependencies = source_pcf.get_dependencies(system_idx);

            original_elements.insert(system_idx, system);

            for child_idx in dependencies {
                let element = source_pcf.get(child_idx).expect("this should never happen");
                original_elements.entry(child_idx).or_insert(element);
            }
        }

        #[allow(clippy::cast_possible_truncation)]
        let old_to_new_idx: HashMap<ElementIdx, ElementIdx> = original_elements
            .iter()
            .enumerate()
            .map(|(new_idx, (old_idx, _))| (*old_idx, new_idx.into()))
            .collect();

        for (_, element) in original_elements {
            let mut attributes = OrderMap::new();

            // this monstrosity is re-mapping old element references to new ones using the new indices mapped
            // in old_to_new_idx
            for (name_idx, attribute) in &element.attributes {
                let new_attribute = match attribute {
                    pcf::Attribute::Element(old_idx) if old_idx.is_valid() => {
                        pcf::Attribute::Element(*old_to_new_idx.get(old_idx).unwrap_or(old_idx))
                    }
                    pcf::Attribute::ElementArray(old_indices) => pcf::Attribute::ElementArray(
                        old_indices
                            .iter()
                            .map(|old_idx| {
                                if old_idx.is_valid() {
                                    *old_to_new_idx.get(old_idx).unwrap_or(old_idx)
                                } else {
                                    *old_idx
                                }
                            })
                            .collect(),
                    ),
                    attribute => attribute.clone(),
                };

                attributes.insert(*name_idx, new_attribute);
            }

            new_elements.push(pcf::Element {
                type_idx: element.type_idx,
                name: element.name.clone(),
                signature: element.signature,
                attributes,
            });
        }

        new_elements
    }

    // fn strip_default_values(
    //     pcf: Pcf,
    //     particle_system_defaults: &HashMap<&'static CStr, Attribute>,
    //     operator_defaults: &HashMap<&'static CStr, Attribute>,
    // ) -> anyhow::Result<Pcf> {
    //     let particle_system_defaults: HashMap<NameIndex, &Attribute> = particle_system_defaults
    //         .iter()
    //         .filter_map(|(key, value)| {
    //             pcf.strings().iter()
    //                 .position(|s| s.0.as_bytes().eq_ignore_ascii_case(key.to_bytes()))
    //                 .map(|idx| (idx as NameIndex, value))
    //         })
    //         .collect();

    //     let operator_defaults: HashMap<NameIndex, &Attribute> = operator_defaults
    //         .iter()
    //         .filter_map(|(key, value)| {
    //             pcf.strings().iter()
    //                 .position(|s| s.0.as_bytes().eq_ignore_ascii_case(key.to_bytes()))
    //                 .map(|idx| (idx as NameIndex, value))
    //         })
    //         .collect();

    //     let mut elements = Vec::new();
    //     for element in pcf.elements() {
    //         let attributes = if element.type_idx == pcf.strings().particle_system_definition_type_idx {
    //             element.attributes
    //                 .into_iter()
    //                 .filter(|(name_idx, attribute)| {
    //                     if let Some(default) = particle_system_defaults.get(name_idx) && attribute == *default {
    //                         false
    //                     } else {
    //                         true
    //                     }
    //                 })
    //                 .collect()
    //         } else if element.type_idx == pcf.strings().particle_operator_type_idx {
    //             element.attributes
    //                 .into_iter()
    //                 .filter(|(name_idx, attribute)| {
    //                     if let Some(default) = operator_defaults.get(name_idx) && attribute == *default {
    //                         false
    //                     } else {
    //                         true
    //                     }
    //                 })
    //                 .collect()
    //         } else {
    //             element.attributes
    //         };

    //         elements.push(Element {
    //             attributes,
    //             ..element
    //         });
    //     }

    //     Ok(Pcf {
    //         elements,
    //         ..pcf
    //     })
    // }

    #[cfg(not(feature = "split_item_fx_pcf"))]
    fn process_mapped_particles(
        target_pcf: Pcf,
        mut pcf_files: Vec<Pcf>,
    ) -> anyhow::Result<Pcf> {
        // We took care of duplicate elements from our addon when grouping addon elements by vanilla PCF, so we
        // don't do any special handling for duplicate elements here.
        let merged_pcf = pcf_files.pop().expect("there should be at least one pcf in the group");
        let merged_pcf = pcf_files.into_iter().try_fold(merged_pcf, Pcf::merge)?;

        let merged_pcf = merged_pcf
            .merge(target_pcf)
            .expect("failed to merge the vanilla PCF into the modified PCF");

        Ok(merged_pcf)
    }

    #[cfg(feature = "split_item_fx_pcf")]
    fn process_mapped_particles(
        target_pcf_path: &str,
        target_pcf: Pcf,
        mut pcf_files: Vec<Pcf>,
    ) -> anyhow::Result<Vec<(&str, Pcf)>> {
        fn cstr_starts_with(string: &std::ffi::CStr, prefix: &std::ffi::CStr) -> bool {
            string.to_bytes().starts_with(prefix.to_bytes())
        }

        // We took care of duplicate elements from our addon when grouping addon elements by vanilla PCF, so we
        // don't do any special handling for duplicate elements here.
        let merged_pcf = pcf_files.pop().expect("there should be at least one pcf in the group");
        let merged_pcf = pcf_files.into_iter().try_fold(merged_pcf, Pcf::merge)?;

        let merged_pcf = merged_pcf
            .merge(target_pcf)
            .expect("failed to merge the vanilla PCF into the modified PCF");

        // item_fx.pcf is a special case, its elements will get split up into item_fx_unusuals.pcf and into
        // item_fx_gameplay.pcf
        let system_definition_type_idx = merged_pcf
            .index_of_string(c"DmeParticleSystemDefinition")
            .expect("DmeParticleSystemDefinition should always be present");
        let processed_pcfs = if target_pcf_path == "particles/item_fx.pcf" {
            let (unusual_elements, gameplay_elements): (Vec<_>, Vec<_>) = merged_pcf.elements.iter().partition(|el| {
                el.name == c"superare_balloon"
                    || cstr_starts_with(&el.name, c"superrare_")
                    || cstr_starts_with(&el.name, c"unusual_")
            });

            let unusual_elements = Self::reindex_elements(&merged_pcf, unusual_elements);
            let gameplay_elements = Self::reindex_elements(&merged_pcf, gameplay_elements);

            let unusual_system_indices: Vec<_> = unusual_elements
                .iter()
                .map_particle_system_indices(&system_definition_type_idx)
                .collect();
            let gameplay_system_indices: Vec<_> = gameplay_elements
                .iter()
                .map_particle_system_indices(&system_definition_type_idx)
                .collect();

            let unusual_root = pcf::Root {
                type_idx: merged_pcf.root.type_idx,
                name: merged_pcf.root.name.clone(),
                signature: merged_pcf.root.signature,
                definitions: unusual_system_indices.into_boxed_slice(),
            };

            let gameplay_root = pcf::Root {
                type_idx: merged_pcf.root.type_idx,
                name: merged_pcf.root.name.clone(),
                signature: merged_pcf.root.signature,
                definitions: gameplay_system_indices.into_boxed_slice(),
            };

            let unusual_pcf = pcf::Pcf::builder()
                .version(merged_pcf.version)
                .strings(merged_pcf.strings.iter().map(|el| el.0.clone()).collect())
                .root(unusual_root)
                .elements(unusual_elements)
                .build();

            let gameplay_pcf = pcf::Pcf::builder()
                .version(merged_pcf.version)
                .strings(merged_pcf.strings.iter().map(|el| el.0.clone()).collect())
                .root(gameplay_root)
                .elements(gameplay_elements)
                .build();

            vec![
                ("particles/item_fx_unusuals.pcf", unusual_pcf),
                ("particles/item_fx_gameplay.pcf", gameplay_pcf),
            ]
        } else {
            vec![(target_pcf_path, merged_pcf)]
        };

        Ok(processed_pcfs)
    }
}

struct ProcessedPcf<'a> {
    addon: &'a Addon,
    pcf: Pcf,
}

mod paths {
    use std::{borrow::Cow, path::Path, str::Utf8Error};

    use typed_path::{PlatformPath, Utf8PlatformPath, Utf8PlatformPathBuf};

    pub fn to_typed(path: &Path) -> Cow<'_, Utf8PlatformPath> {
        match path.as_os_str().to_string_lossy() {
            Cow::Borrowed(path) => Cow::Borrowed(Utf8PlatformPath::from_bytes_path(PlatformPath::new(path)).unwrap()),
            Cow::Owned(path) => Cow::Owned(Utf8PlatformPathBuf::from(path)),
        }
    }

    pub fn std_to_typed(path: &Path) -> Result<&Utf8PlatformPath, Utf8Error> {
        Utf8PlatformPath::from_bytes_path(PlatformPath::new(path.as_os_str().as_encoded_bytes()))
    }
}

const TF2_VPK_NAME: &str = "tf2_misc_dir.vpk";
const APP_INSTANCE_NAME: &str = "net.dresswithpockets.tf2dazzle.lock";
const APP_TLD: &str = "net";
const APP_ORG: &str = "dresswithpockets";
const APP_NAME: &str = "tf2dazzle";
const PARTICLE_SYSTEM_MAP: &str = include_str!("particle_system_map.json");

#[derive(Debug, Error)]
enum BuildError {
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

    #[error("couldn't find the custom directory in the tf dir specified: '{0}'")]
    MissingTfCustomDirectory(Utf8PlatformPathBuf),

    #[error("couldn't find the custom directory in the tf dir specified: '{0}', due to an IO error")]
    IoTfCustomDirectory(Utf8PlatformPathBuf, io::Error),

    #[error("couldn't read tf2_misc_dir.vpk: {0}")]
    CantReadMiscVpk(#[from] vpk::Error),
}

#[derive(Default)]
struct AppBuilder {
    tf_dir: Utf8PlatformPathBuf,
}

impl AppBuilder {
    fn with_tf_dir(path: Utf8PlatformPathBuf) -> Self {
        Self { tf_dir: path }
    }

    fn create_single_instance() -> Result<SingleInstance, BuildError> {
        // TODO: single_instance's macos implementation might not be desirable since this program is intended to be portable... maybe we just dont support macos (:
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

    fn get_working_dir(dirs: &ProjectDirs) -> Utf8PlatformPathBuf {
        let working_dir = dirs.data_local_dir().join("working");
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

    fn get_misc_vpk(&self) -> Result<VPK, BuildError> {
        let vpk_path = self.tf_dir.join(TF2_VPK_NAME);
        Ok(VPK::read(vpk_path)?)
    }

    fn get_tf_custom_dir(&self) -> Result<Utf8PlatformPathBuf, BuildError> {
        let custom_path = self.tf_dir.join("custom");

        match fs::metadata(&custom_path) {
            Ok(metadata) if metadata.is_dir() => Ok(custom_path),
            Err(err) if err.kind() != io::ErrorKind::NotFound => Err(BuildError::IoTfCustomDirectory(custom_path, err)),
            _ => Err(BuildError::MissingTfCustomDirectory(custom_path)),
        }
    }

    fn build(self) -> Result<App, BuildError> {
        _ = Self::create_single_instance()?;

        let project_dirs = Self::create_project_dirs()?;
        let working_dir = Self::get_working_dir(&project_dirs);
        let extracted_content_dir = Self::create_new_content_cache_dir(&working_dir)?;
        let working_vpk_dir = Self::create_new_working_vpk_dir(&working_dir)?;
        let addons_dir = Self::create_addons_dir(&working_dir)?;
        let backup_dir = Self::get_backup_dir()?;
        let tf_custom_dir = self.get_tf_custom_dir()?;
        let tf_misc_vpk = self.get_misc_vpk()?;

        let vanilla_pcf_to_systems = Self::get_vanilla_pcf_map();
        let vanilla_system_to_pcf: HashMap<CString, String> = vanilla_pcf_to_systems
            .iter()
            .flat_map(|(pcf_path, systems)| systems.iter().map(|system| (system.clone(), pcf_path.clone())))
            .collect();

        let mut vanilla_pcf_paths = Vec::new();
        for path in vanilla_pcf_to_systems.keys() {
            let path = Utf8UnixPathBuf::from_str(path).expect("the PCF map keys must always be valid unix paths");
            vanilla_pcf_paths.push(path.with_platform_encoding());
        }

        Ok(App {
            addons_dir,
            extracted_content_dir,
            backup_dir,
            working_vpk_dir,

            vanilla_pcf_paths,
            vanilla_pcf_to_systems,
            vanilla_system_to_pcf,

            tf_misc_vpk,
            tf_custom_dir,
        })
    }
}

fn main() -> anyhow::Result<()> {
    /*
       TODO: if not already configured, detect/select a tf/ directory
       TODO: tui for configuring enabled/disabled custom particles found in addons
       TODO: tui for selecting addons to install/uninstall
       TODO: detect conflicts in selected addons
    */

    let particle_system_defaults: HashMap<&'static CStr, Attribute> = HashMap::from([
        (c"max_particles", 1000i32.into()),
        (c"initial_particles", 0i32.into()),
        (c"material", "vgui/white".to_string().into_bytes().into_boxed_slice().into()),
        (c"bounding_box_min", Vector3((-10.0).into(), (-10.0).into(), (-10.0).into()).into()),
        (c"bounding_box_max", Vector3(10.0.into(), 10.0.into(), 10.0.into()).into()),
        (c"cull_radius", 0.0.into()),
        (c"cull_cost", 1.0.into()),
        (c"cull_control_point", 0.into()),
        (c"cull_replacement_definition", String::new().into_bytes().into_boxed_slice().into()),
        (c"radius", 5.0.into()),
        (c"color", Color(255, 255, 255, 255).into()),
        (c"rotation", 0.0.into()),
        (c"rotation_speed", 0.0.into()),
        (c"sequence_number", 0.into()),
        (c"sequence_number1", 0.into()),
        (c"group id", 0.into()),
        (c"maximum time step", 0.1.into()),
        (c"maximum sim tick rate", 0.0.into()),
        (c"minimum sim tick rate", 0.0.into()),
        (c"minimum rendered frames", 0.into()),
        (c"control point to disable rendering if it is the camera", (-1).into()),
        (c"maximum draw distance", 100000.0.into()),
        (c"time to sleep when not drawn", 8.0.into()),
        (c"Sort particles", true.into()),
        (c"batch particle systems", false.into()),
        (c"view model effect", false.into())
    ]);

    let operator_defaults: HashMap<&'static CStr, Attribute> = HashMap::from([
        (c"operator start fadein", 0.0.into()),
        (c"operator end fadein", 0.0.into()),
        (c"operator start fadeout", 0.0.into()),
        (c"operator end fadeout", 0.0.into()),
        (c"operator fade oscillate", 0.0.into()),
        (c"Visibility Proxy Input Control Point Number", (-1).into()),
        (c"Visibility Proxy Radius", 1.0.into()),
        (c"Visibility input minimum", 0.0.into()),
        (c"Visibility input maximum", 1.0.into()),
        (c"Visibility Alpha Scale minimum", 0.0.into()),
        (c"Visibility Alpha Scale maximum", 1.0.into()),
        (c"Visibility Radius Scale minimum", 1.0.into()),
        (c"Visibility Radius Scale maximum", 1.0.into()),
        (c"Visibility Camera Depth Bias", 0.0.into())
    ]);

    let tf_dir: Utf8PlatformPathBuf = ["local_test", "tf"].iter().collect();
    let mut app = AppBuilder::with_tf_dir(tf_dir.clone()).build()?;

    // let app = App {
    //     _config_dir: paths::to_typed(config_dir).to_path_buf(),
    //     _config_file: paths::to_typed(&config_file).to_path_buf(),
    //     extracted_content_dir: paths::to_typed(&extracted_addons_dir).to_path_buf(),
    //     particles_working_dir: paths::to_typed(&particles_working_dir).to_path_buf(),
    //     addons_dir: paths::to_typed(&addons_dir).to_path_buf(),
    //     backup_dir,
    //     vanilla_pcf_paths,
    //     pcf_to_particle_system,
    //     particle_system_to_pcf,

    //     vpk_working_dir: paths::to_typed(&vpk_working_dir).to_path_buf(),
    //     vpk_out_dir: paths::to_typed(&vpk_working_dir).join_checked("custom")?,
    // };

    // TODO: detect tf directory
    // TODO: prompt user to verify or provide their own tf directory after discovery attempt

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
        let extracted = match source.extract_as_subfolder_in(&app.extracted_content_dir) {
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

    // TODO: filter out PCFs based on user selection, for now we'll just pick the first one in the list if there are conflicting PCFs

    // create intermediary PCF objects by cross referencing our addon PCFs with the particle_system_map.json
    let mut processed_pcfs = HashMap::new();
    for addon in &addons {
        /*
            in a copy of vanilla tf2, there are many PCFs containing particle system definitions. Except in a couple
            cases, each particle system is only defined once across all PCFs. particle_system_map.json maps the path to
            a PCF to a list of all particle systems defined in that PCF.

            the goal of the following code is to produce new versions of the vanilla PCFs with any modified particle
            system definitions overwritten in each PCF.
        */
        let processed_target_pcf_paths = app.merge_addon_particles(&addon.particle_files);

        // Our merged PCF may be missing some elements in present in the vanilla PCF, so we lazily decode the
        // target vanilla PCF and merge it in.
        for (target_pcf_path, pcf_files) in processed_target_pcf_paths {
            let full_pcf_path = app.backup_dir.join_checked(&target_pcf_path)?;
            let mut reader = File::open_buffered(full_pcf_path)?;
            let target_pcf = pcf::decode(&mut reader)?;

            let new_pcf = App::process_mapped_particles(target_pcf, pcf_files)?;
            let stripped_pcf = new_pcf.strip_default_values(&particle_system_defaults, &operator_defaults);

            processed_pcfs
                .entry(target_pcf_path)
                .or_insert(ProcessedPcf { addon, pcf: stripped_pcf });
        }
    }

    // TODO: if feature = "split_item_fx_pcf" then we need to merge split-up particles - this may not even be necessary if we scrap item_fx splitting completely

    // Addon particles might refer to a custom or modified material provided by the addon; we also need to make sure
    // that materials & textures are copied over.
    let mut materials = HashMap::new();
    for (new_path, processed_pcf) in &processed_pcfs {
        // TODO: what if two addons provide materials with the same name?
        //       does the particle system's material need to exist in vanilla?
        //       if the neither material or texture need to be present in the base game: we can give each vmt and vtf
        //       a unique name - like a hash - then change the name of the material in the PCF to point to the new vmt.
        //
        //       for now, we just pick the first material.

        let particle_systems = processed_pcf.pcf.get_particle_system_definitions();
        for element in particle_systems {
            let Some(material_name) = processed_pcf.pcf.get_material(element) else {
                eprintln!(
                    "The PCF '{new_path}' contains a particle system '{}' with no material definition. Skipping",
                    element.name.display()
                );
                continue;
            };

            // we don't need to copy over vgui/white - its a special case in Source
            if material_name == c"vgui/white" {
                continue;
            }

            let material_name = material_name.to_string_lossy().clone().into_owned();

            // we only care about this material if the addon actually provides it.
            let Some(parsed_material) = processed_pcf.addon.relative_material_files.get(&material_name) else {
                continue;
            };

            materials
                .entry(material_name)
                .or_insert((processed_pcf.addon, parsed_material));
        }
    }

    for addon in &addons {
        copy_addon_structure(&addon.content_path, &app.working_vpk_dir)?;

        let from_materials_path = addon.content_path.join_checked("materials")?;
        let to_materials_path = app.working_vpk_dir.join_checked("materials")?;

        for (material_name, material) in &addon.relative_material_files {
            let from_path = from_materials_path.join_checked(&material.relative_path)?;
            let to_path = to_materials_path.join_checked(material_name)?;
            fs::create_dir_all(to_path.parent().unwrap())?;

            if let Err(err) = copy(&from_path, &to_path) {
                eprintln!(
                    "There was an error copying the extracted material '{}' to '{to_path}': {err}",
                    &material.relative_path
                );
                process::exit(1);
            }
        }

        for (texture_name, texture_path) in &addon.texture_files {
            let to_path = to_materials_path.join_checked(texture_name)?;
            if let Err(err) = copy(texture_path, &to_path) {
                eprintln!("There was an error copying the extracted texture '{texture_path}' to '{to_path}': {err}");
                process::exit(1);
            }
        }
    }

    // ensuring that any non-vanilla materials required by our PCFs are copied over to our working directory
    // for (material_name, (addon, material)) in materials {
    //     let from_materials_path = addon.content_path.join_checked("materials")?;
    //     let to_materials_path = app.working_vpk_dir.join_checked("materials")?;

    //     let from_path = from_materials_path.join_checked(&material.relative_path)?;
    //     let to_path = to_materials_path.join_checked(material_name)?;
    //     if let Err(err) = copy(&from_path, &to_path) {
    //         eprintln!(
    //             "There was an error copying the extracted material '{}' to '{to_path}': {err}",
    //             &material.relative_path
    //         );
    //         process::exit(1);
    //     }

    //     if let Some(texture_name) = &material.base_texture
    //         && let Some(from_path) = addon.texture_files.get(texture_name)
    //     {
    //         let to_path = to_materials_path.join_checked(texture_name)?;
    //         if let Err(err) = copy(from_path, &to_path) {
    //             eprintln!("There was an error copying the extracted texture '{from_path}' to '{to_path}': {err}");
    //             process::exit(1);
    //         }
    //     }

    //     if let Some(texture_name) = &material.detail
    //         && let Some(from_path) = addon.texture_files.get(texture_name)
    //     {
    //         let to_path = to_materials_path.join_checked(texture_name)?;
    //         if let Err(err) = copy(from_path, &to_path) {
    //             eprintln!("There was an error copying the extracted texture '{from_path}' to '{to_path}': {err}");
    //             process::exit(1);
    //         }
    //     }

    //     if let Some(texture_name) = &material.ramp_texture
    //         && let Some(from_path) = addon.texture_files.get(texture_name)
    //     
    //         let to_path = to_materials_path.join_checked(texture_name)?;
    //         if let Err(err) = copy(from_path, &to_path) {
    //             eprintln!("There was an error copying the extracted texture '{from_path}' to '{to_path}': {err}");
    //             process::exit(1);
    //         }
    //     }

    //     if let Some(texture_name) = &material.normal_map
    //         && let Some(from_path) = addon.texture_files.get(texture_name)
    //     {
    //         let to_path = to_materials_path.join_checked(texture_name)?;
    //         if let Err(err) = copy(from_path, &to_path) {
    //             eprintln!("There was an error copying the extracted texture '{from_path}' to '{to_path}': {err}");
    //             process::exit(1);
    //         }
    //     }

    //     if let Some(texture_name) = &material.normal_map_2
    //         && let Some(from_path) = addon.texture_files.get(texture_name)
    //     {
    //         let to_path = to_materials_path.join_checked(texture_name)?;
    //         if let Err(err) = copy(from_path, &to_path) {
    //             eprintln!("There was an error copying the extracted texture '{from_path}' to '{to_path}': {err}");
    //             process::exit(1);
    //         }
    //     }
    // }

    // ensure we start from a consistent state by restoring the particles in the tf misc vpk back to vanilla content.
    if let Err(err) = app.tf_misc_vpk.restore_particles(&app.backup_dir) {
        eprintln!("There was an error restoring some or all particles to the vanilla state: {err}");
        process::exit(1);
    }

    // TODO: de-duplicate elements in item_fx.pcf, halloween.pcf, bigboom.pcf, and dirty_explode.pcf.
    //       NB we dont need to do this for any PCFs already in our present_pcfs map
    //       NBB we can just do the usual routine of: decode, filter by particle_system_map, and reindex
    //           - once done, we can just add these PCFs to processed_pcfs

    // TODO: investigate blood_trail.pcf -> npc_fx.pc "hacky fix for blood_trail being so small"

    // TODO: compute size without writing the entire PCF to a buffer in-memory
    for (new_path, processed_pcf) in processed_pcfs {
        let mut writer = BytesMut::new().writer();
        processed_pcf.pcf.encode(&mut writer)?;

        let buffer = writer.into_inner();
        let size = buffer.len() as u64;
        let mut reader = buffer.reader();
        app.tf_misc_vpk.patch_file(&new_path, size, &mut reader)?;
    }

    // we can finally generate our _dazzle_addons VPKs from our addon contents.
    vpk_writer::pack_directory(
        &app.working_vpk_dir,
        &app.tf_custom_dir,
        "_dazzle_addons",
        SPLIT_BY_2GB,
    )?;

    // NOTE(dress) after packing everything, cueki does a full-scan of every VPK & file in tf/custom for $ignorez 1 then
    //             replaces each with spaces. This isn't necessary at all, so we just don't do it; anyone can bypass her
    //             code with a modicum of motivation and python knoweledge. Considering how easy it is to remove it from
    //             her preloader, I wouldn't be surprised if I frequently run into people using $ignorez trickfoolery in
    //             pubs.

    // TODO: install/restore modified gameinfo.txt VDF

    /*
       TODO/Spike:
           # if pcf_file = Path("particles/example.pcf"), then base_name = "example"
           base_name = pcf_file.name
           mod_pcf = PCFFile(pcf_file).decode()

           if base_name != folder_setup.base_default_pcf.input_file.name and check_parents(mod_pcf, folder_setup.base_default_parents):
               continue

           if base_name == folder_setup.base_default_pcf.input_file.name:
               mod_pcf = update_materials(folder_setup.base_default_pcf, mod_pcf)

           # process the mod PCF
           processed_pcf = remove_duplicate_elements(mod_pcf)

           if pcf_file.stem in DX8_LIST: # dx80 first
               dx_80_name = pcf_file.stem + "_dx80.pcf"
               file_handler.process_file(dx_80_name, processed_pcf)

           file_handler.process_file(base_name, processed_pcf)
    */

    // TODO: figure out how particle_system_map.json is generated. Is it just a map of vanilla PCF paths to named particle system definition elements?

    // TODO: process and patch particles into main VPK, handling duplicate effects


    Ok(())
}

fn copy_addon_structure(in_dir: &Utf8PlatformPath, out_dir: &Utf8PlatformPath) -> anyhow::Result<()> {
    fn visit(in_dir: &Utf8PlatformPath, out_dir: &Utf8PlatformPath) -> anyhow::Result<()> {
        // create the directory tree before we copy anything over
        for entry in fs::read_dir(in_dir)? {
            let entry = entry?;
            let metadata = entry.metadata()?;
            if metadata.is_dir() {
                let path = entry.path();
                let path = paths::to_typed(&path).absolutize()?;
                let new_out_dir = out_dir.join(path.strip_prefix(in_dir)?);
                fs::create_dir(&new_out_dir)?;

                visit(&path, &new_out_dir)?;
            }
        }

        Ok(())
    }

    // create the directory tree before we copy anything over
    for entry in fs::read_dir(in_dir)? {
        let entry = entry?;
        let metadata = entry.metadata()?;

        if entry.file_name().eq_ignore_ascii_case("particles") {
            continue;
        }

        if metadata.is_dir() {
            let path = entry.path();
            let path = paths::to_typed(&path).absolutize()?;
            let new_out_dir = out_dir.join(path.strip_prefix(in_dir)?);
            fs::create_dir(&new_out_dir)?;

            visit(&path, &new_out_dir)?;
        }
    }

    Ok(())
}