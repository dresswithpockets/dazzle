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

use std::{
    collections::{BTreeMap, HashMap},
    ffi::CString,
    fs::{self, File, copy},
    io::{self},
    path::PathBuf,
    process,
    str::FromStr,
};

use directories::ProjectDirs;
use ordermap::OrderMap;
use pcf::{ElementsExt, Pcf};
use single_instance::SingleInstance;
use typed_path::Utf8PlatformPathBuf;

use crate::addon::{Addon, Sources};
use crate::patch::PatchVpkExt;

struct App {
    _config_dir: Utf8PlatformPathBuf,
    _config_file: Utf8PlatformPathBuf,
    addons_dir: Utf8PlatformPathBuf,
    extracted_addons_dir: Utf8PlatformPathBuf,
    particles_working_dir: Utf8PlatformPathBuf,
    vpk_working_dir: Utf8PlatformPathBuf,
    backup_dir: Utf8PlatformPathBuf,
    vanilla_pcf_paths: Vec<Utf8PlatformPathBuf>,
    pcf_to_particle_system: HashMap<String, Vec<CString>>,
    particle_system_to_pcf: HashMap<CString, String>,
}

impl App {
    fn merge_addon_particles(
        &self,
        particle_files: &HashMap<Utf8PlatformPathBuf, pcf::Pcf>,
    ) -> HashMap<&String, Vec<Pcf>> {
        let mut processed_target_pcf_paths: HashMap<&String, Vec<Pcf>> = HashMap::new();
        for (file_path, pcf) in particle_files {
            // dx80 and dx90 are a special case that we skip over. TODO: i think we generate them later?
            let file_name: &str = file_path.file_name().expect("there should always be a file name");
            if file_name.contains("dx80") || file_name.contains("dx90") {
                continue;
            }

            #[allow(clippy::cast_possible_truncation)]
            let system_definition_type_idx = pcf
                .index_of_string(c"DmeParticleSystemDefinition")
                .expect("DmeParticleSystemDefinition should always be present");

            // grouping the elements from our addon by the vanilla PCF they're mapped to in particle_system_map.json.
            let mut elements_by_vanilla_pcf_path = HashMap::<&String, OrderMap<&CString, &pcf::Element>>::new();
            for element in &pcf.elements {
                let Some(pcf_path) = self.particle_system_to_pcf.get(&element.name) else {
                    continue;
                };

                // we're also ridding ourselves of duplicate particle systems here. The first one always takes priority,
                // subsequent particle systems with the same name are skipped entirely.
                elements_by_vanilla_pcf_path
                    .entry(pcf_path)
                    .or_default()
                    .entry(&element.name)
                    .or_insert(element);
            }

            for (target_pcf_path, matched_elements) in elements_by_vanilla_pcf_path {
                // matched_elements contains a subset of the original elements in the pcf. As a result, any
                // Element or ElementArray attributes may not point to the correct index - the order is
                // retained but the indices aren't. So, we need to reindex any references to other elements in the set.
                let new_elements = Self::reindex_elements(pcf, matched_elements.into_values());

                // the root element always stores an attribute "particleSystemDefinitions" which stores an ElementArray
                // containing the index of every DmeParticleSystemDefinition-type element. We've changed the indices of
                // our particle system definitions, so we need to update the root element's list with the new indices.
                let particle_system_indices: Vec<_> = new_elements
                    .iter()
                    .map_particle_system_indices(&system_definition_type_idx)
                    .collect();

                // our filtered `new_elements` only contains particle systems, it does not contain a root element
                let root = pcf::Root {
                    type_idx: pcf.root.type_idx,
                    name: pcf.root.name.clone(),
                    signature: pcf.root.signature,
                    definitions: particle_system_indices.into_boxed_slice(),
                };

                // this new in-memory PCF has only the elements listed in elements_to_extract, with element references
                // fixed to match any changes in indices.
                let new_pcf = pcf::Pcf::builder()
                    .version(pcf.version)
                    .strings(pcf.strings.clone())
                    .root(root)
                    .elements(new_elements)
                    .build();

                processed_target_pcf_paths
                    .entry(target_pcf_path)
                    .or_default()
                    .push(new_pcf);
            }
        }

        processed_target_pcf_paths
    }

    fn reindex_elements<'a>(
        source_pcf: &'a Pcf,
        elements: impl IntoIterator<Item = &'a pcf::Element>,
    ) -> Vec<pcf::Element> {
        let mut new_elements = Vec::new();
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
            .map(|(new_idx, (old_idx, _))| (*old_idx, (new_idx) as u32))
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

            new_elements.push(pcf::Element {
                type_idx: element.type_idx,
                name: element.name.clone(),
                signature: element.signature,
                attributes,
            });
        }

        new_elements
    }

    #[cfg(not(feature = "split_item_fx_pcf"))]
    fn process_mapped_particles(
        target_pcf_path: &str,
        target_pcf: Pcf,
        mut pcf_files: Vec<Pcf>,
    ) -> anyhow::Result<(&str, Pcf)> {
        // We took care of duplicate elements from our addon when grouping addon elements by vanilla PCF, so we
        // don't do any special handling for duplicate elements here.
        let merged_pcf = pcf_files.pop().expect("there should be at least one pcf in the group");
        let merged_pcf = pcf_files.into_iter().try_fold(merged_pcf, Pcf::merge)?;

        let merged_pcf = merged_pcf
            .merge(target_pcf)
            .expect("failed to merge the vanilla PCF into the modified PCF");

        Ok((target_pcf_path, merged_pcf))
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

    let tf_dir: Utf8PlatformPathBuf = ["local_test", "tf"].iter().collect();

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

    let vpk_working_dir = working_dir.join("vpk");
    if let Err(err) = fs::remove_dir_all(&vpk_working_dir)
        && err.kind() != io::ErrorKind::NotFound
    {
        eprintln!("Couldn't clear the VPK working directory: {err}");
        process::exit(1);
    }

    if let Err(err) = fs::create_dir_all(&vpk_working_dir) {
        eprintln!("Couldn't create the VPK working directory: {err}");
        process::exit(1);
    }

    let particles_working_dir = working_dir.join("particles");
    if let Err(err) = fs::remove_dir_all(&particles_working_dir)
        && err.kind() != io::ErrorKind::NotFound
    {
        eprintln!("Couldn't clear the particles working cache: {err}");
        process::exit(1);
    }

    if let Err(err) = fs::create_dir_all(&particles_working_dir) {
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
        vpk_working_dir: paths::std_to_typed(&vpk_working_dir)?.to_path_buf(),
        addons_dir: paths::std_to_typed(&addons_dir)?.to_path_buf(),
        backup_dir,
        vanilla_pcf_paths,
        pcf_to_particle_system,
        particle_system_to_pcf,
    };

    // TODO: detect tf directory
    // TODO: prompt user to verify or provide their own tf directory after discovery attempt

    let vpk_path = tf_dir.join(TF2_VPK_NAME);
    let mut misc_vpk = match vpk::VPK::read(vpk_path) {
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
            let full_pcf_path = app.backup_dir.join_checked(target_pcf_path)?;
            let mut reader = File::open_buffered(full_pcf_path)?;
            let target_pcf = pcf::decode(&mut reader)?;

            let (new_pcf_path, new_pcf) = App::process_mapped_particles(target_pcf_path, target_pcf, pcf_files)?;
            processed_pcfs
                .entry(new_pcf_path)
                .or_insert(ProcessedPcf { addon, pcf: new_pcf });
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

    // ensuring that any non-vanilla materials required by our PCFs are copied over to our working directory
    for (material_name, (addon, material)) in materials {
        let from_materials_path = addon.content_path.join_checked("materials")?;
        let to_materials_path = app.vpk_working_dir.join_checked("materials")?;

        let from_path = from_materials_path.join_checked(&material.relative_path)?;
        let to_path = to_materials_path.join_checked(material_name)?;
        if let Err(err) = copy(&from_path, &to_path) {
            eprintln!(
                "There was an error copying the extracted material '{}' to '{to_path}': {err}",
                &material.relative_path
            );
            process::exit(1);
        }

        if let Some(texture_name) = &material.base_texture
            && let Some(from_path) = addon.texture_files.get(texture_name)
        {
            let to_path = to_materials_path.join_checked(texture_name)?;
            if let Err(err) = copy(from_path, &to_path) {
                eprintln!("There was an error copying the extracted texture '{from_path}' to '{to_path}': {err}");
                process::exit(1);
            }
        }

        if let Some(texture_name) = &material.detail
            && let Some(from_path) = addon.texture_files.get(texture_name)
        {
            let to_path = to_materials_path.join_checked(texture_name)?;
            if let Err(err) = copy(from_path, &to_path) {
                eprintln!("There was an error copying the extracted texture '{from_path}' to '{to_path}': {err}");
                process::exit(1);
            }
        }

        if let Some(texture_name) = &material.ramp_texture
            && let Some(from_path) = addon.texture_files.get(texture_name)
        {
            let to_path = to_materials_path.join_checked(texture_name)?;
            if let Err(err) = copy(from_path, &to_path) {
                eprintln!("There was an error copying the extracted texture '{from_path}' to '{to_path}': {err}");
                process::exit(1);
            }
        }

        if let Some(texture_name) = &material.normal_map
            && let Some(from_path) = addon.texture_files.get(texture_name)
        {
            let to_path = to_materials_path.join_checked(texture_name)?;
            if let Err(err) = copy(from_path, &to_path) {
                eprintln!("There was an error copying the extracted texture '{from_path}' to '{to_path}': {err}");
                process::exit(1);
            }
        }

        if let Some(texture_name) = &material.normal_map_2
            && let Some(from_path) = addon.texture_files.get(texture_name)
        {
            let to_path = to_materials_path.join_checked(texture_name)?;
            if let Err(err) = copy(from_path, &to_path) {
                eprintln!("There was an error copying the extracted texture '{from_path}' to '{to_path}': {err}");
                process::exit(1);
            }
        }
    }

    // ensure we start from a consistent state by restoring the particles in the tf misc vpk back to vanilla content.
    if let Err(err) = misc_vpk.restore_particles(&app.backup_dir) {
        eprintln!("There was an error restoring some or all particles to the vanilla state: {err}");
        process::exit(1);
    }

    // TODO: create a new VPK from our vpk working directory, we need to split VPKs into max-2GB VPKs.

    // TODO: de-duplicate elements in item_fx.pcf, halloween.pcf, bigboom.pcf, and dirty_explode.pcf.
    //       NB we dont need to do this if for any PCFs already in our present_pcfs map
    //       NBB we can just do the usual routine of: decode, filter by particle_system_map, and reindex
    //           once done, we can just add these PCFs to processed_pcfs

    // TODO: investigate blood_trail.pcf -> npc_fx.pc "hacky fix for blood_trail being so small"

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
