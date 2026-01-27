use std::{
    collections::HashMap,
    fs::{self, File},
    io,
    sync::Arc,
    thread::{self, JoinHandle},
};

use super::process::ProcessState;
use eframe::egui;
use pcf::Pcf;
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use thiserror::Error;

use crate::{
    app::{Paths, process::ProcessView},
    pcf_defaults,
};
use addon::{self, Addon, ExtractionError, Sources};

struct InitialLoader {
    paths: Paths,
    operator_defaults: Arc<HashMap<&'static str, pcf::Attribute>>,
    particle_system_defaults: Arc<HashMap<&'static str, pcf::Attribute>>,
}

#[derive(Debug, Error)]
pub(crate) enum LoadError {
    #[error(transparent)]
    Sources(#[from] addon::Error),

    #[error(transparent)]
    Extraction(#[from] ExtractionError),

    #[error(transparent)]
    Parse(#[from] addon::ParseError),
}

// A LoadOperation is an operation which processes some state and has a UI presentation to reflect the current state
// of the operation. Like
// - loading/processing setup files
// - handling new addons when theyre imported
// - installing addons to tf2

pub(crate) fn start_initial_load(
    ctx: &egui::Context,
    paths: Paths,
) -> (ProcessView, JoinHandle<Result<Vec<Addon>, LoadError>>) {
    let loader = InitialLoader::new(paths);
    let (load_state, load_view) =
        ProcessState::with_progress_bar(ctx, InitialLoader::operation_steps().try_into().unwrap());

    let handle = thread::spawn(move || -> Result<Vec<Addon>, LoadError> { loader.run(&load_state) });

    (load_view, handle)
}

impl InitialLoader {
    fn new(paths: Paths) -> Self {
        Self {
            paths,
            operator_defaults: Arc::new(pcf_defaults::get_default_operator_map()),
            particle_system_defaults: Arc::new(pcf_defaults::get_particle_system_defaults()),
        }
    }

    fn operation_steps() -> usize {
        90
    }

    fn run(&self, load_operation: &ProcessState) -> Result<Vec<Addon>, LoadError> {
        load_operation.push_status("Loading addons...");
        let sources = Sources::read_dir(&self.paths.addons)?;
        load_operation.add_progress(30);

        if !sources.failures.is_empty() {
            // TODO: we should present information about addons that failed to load to the user
            eprintln!("There were some errors reading some or all addon sources:");
            for (path, error) in sources.failures {
                eprintln!("  {path}: {error}");
            }
        }

        let extracted_addons: Result<Vec<_>, _> = sources
            .sources
            .into_par_iter()
            .map(|source| {
                load_operation.push_status(format!("Extracting addon {}", source.name().unwrap_or_default()));
                source.extract_as_subfolder_in(&self.paths.extracted_content)
            })
            .collect();
        load_operation.add_progress(30);

        let mut addons = Vec::new();
        for addon in extracted_addons? {
            load_operation.push_status(format!("Parsing contents of {}", addon.name().unwrap_or_default()));
            addons.push(addon.parse_content()?);
        }
        load_operation.add_progress(30);
        load_operation.push_status("Done!");

        Ok(addons)
    }

    fn get_vanilla_pcf_groups_from_manifest(
        load_operation: &ProcessState,
    ) -> Result<Vec<VanillaPcfGroup>, VanillaPcfError> {
        let mut dx80_names = HashMap::new();
        let mut dx90_slow_names = HashMap::new();

        let mut entries = Vec::new();
        for name in pcf_defaults::PARTICLES_MANIFEST {
            let file_path = format!("backup/tf_{name}");
            let size = fs::metadata(&file_path)?.len();
            entries.push((name, file_path, size));

            let dx80_name = format!("{}_dx80.pcf", name.trim_suffix(".pcf"));
            let file_path = format!("backup/tf_{dx80_name}");
            if fs::exists(&file_path)? {
                let size = fs::metadata(&file_path)?.len();
                dx80_names.insert(name.to_string(), (name, file_path, size));
            }

            let dx90_name = format!("{}_dx90_slow.pcf", name.trim_suffix(".pcf"));
            let file_path = format!("backup/tf_{dx90_name}");
            if fs::exists(&file_path)? {
                let size = fs::metadata(&file_path)?.len();
                dx90_slow_names.insert(name.to_string(), (name, file_path, size));
            }
        }

        entries
            .into_par_iter()
            .map(|(name, file_path, size)| -> Result<VanillaPcfGroup, VanillaPcfError> {
                load_operation.push_status(format!("Decoding {name}"));
                load_operation.increment_progress();
                println!("Found {name}. Decoding...");
                let mut reader = File::open_buffered(file_path)?;
                let pcf = pcf::decode(&mut reader)?;

                let mut group = VanillaPcfGroup {
                    default: VanillaPcf {
                        name: name.to_string(),
                        pcf,
                        size,
                    },
                    dx80: None,
                    dx90_slow: None,
                };

                if let Some((name, file_path, size)) = dx80_names.get(&group.default.name) {
                    println!("    Found dx80 variant, {name}. Decoding...");
                    let mut reader = File::open_buffered(file_path)?;
                    let pcf = pcf::decode(&mut reader)?;

                    group.dx80 = Some(VanillaPcf {
                        name: name.to_string(),
                        pcf,
                        size: *size,
                    });
                }

                if let Some((name, file_path, size)) = dx90_slow_names.get(&group.default.name) {
                    println!("    Found dx90 variant, {name}. Decoding...");
                    let mut reader = File::open_buffered(file_path)?;
                    let pcf = pcf::decode(&mut reader)?;

                    group.dx90_slow = Some(VanillaPcf {
                        name: name.to_string(),
                        pcf,
                        size: *size,
                    });
                }

                Ok(group)
            })
            .collect()
    }
}

// #[derive(Debug)]
// pub(crate) struct InitialLoader {
//     tf_dir: Utf8PlatformPathBuf,

//     setup_handle: JoinHandle<()>,
//     process_view: ProcessView,
// }

// impl InitialLoader {
//     pub(crate) fn new(ctx: &egui::Context, paths: &Paths, tf_dir: Utf8PlatformPathBuf) -> Self {
//         let operator_defaults = pcf_defaults::get_default_operator_map();
//         let particle_system_defaults = pcf_defaults::get_particle_system_defaults();

//         let setup = Setup::new(paths, operator_defaults, particle_system_defaults);
//         let (load_operation, load_view) = ProcessState::new(ctx, Setup::operation_steps().try_into().unwrap());

//         let worker_handle = thread::spawn(move || {
//             setup.run(&load_operation);
//         });

//         Self {
//             tf_dir,
//             setup_handle: worker_handle,
//             process_view: load_view,
//         }
//     }

//     pub(crate) fn ui(&mut self, ui: &egui::Ui) {
//         self.process_view.show("simpler installer load view", ui.ctx());
//     }
// }

struct VanillaPcfGroup {
    default: VanillaPcf,
    dx80: Option<VanillaPcf>,
    dx90_slow: Option<VanillaPcf>,
}

struct VanillaPcf {
    name: String,
    pcf: Pcf,
    size: u64,
}

#[derive(Debug, Error)]
enum VanillaPcfError {
    #[error(transparent)]
    Io(#[from] io::Error),

    #[error(transparent)]
    Decode(#[from] pcf::DecodeError),
}

fn default_bin_from(group: &VanillaPcfGroup) -> pcfpack::Bin {
    pcfpack::Bin::new(
        group.default.size,
        group.default.name.clone(),
        Pcf::new_empty_from(&group.default.pcf),
    )
}
