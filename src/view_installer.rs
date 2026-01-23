use std::{collections::HashMap, fs::{self, File}, io, num::NonZero, sync::{Arc, mpsc::{self, }}, thread::{self, JoinHandle}};

use atomic_counter::{AtomicCounter};
use eframe::egui::{self, CentralPanel, text::{}};
use pcf::Pcf;
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use thiserror::Error;
use typed_path::Utf8PlatformPathBuf;

use crate::{addon::Sources, app::App, packing::{PcfBin, PcfBinMap}, pcf_defaults, process::{ProcessState, ProcessView}, styles};

struct LoadingVanillaPcfs {
    handle: JoinHandle<Result<Vec<VanillaPcfGroup>, VanillaPcfError>>,
}

struct ParsingAddons {

}

enum State {
    LoadingVanillaPcfs(LoadingVanillaPcfs),
    ErrorLoadingVanillaPcfs(VanillaPcfError),
    ParsingAddons()
}

struct Setup {
    app_dirs: App,
    operator_defaults: Arc<HashMap<&'static str, pcf::Attribute>>,
    particle_system_defaults: Arc<HashMap<&'static str, pcf::Attribute>>,
}

// A LoadOperation is an operation which processes some state and has a UI presentation to reflect the current state 
// of the operation. Like
// - loading/processing setup files
// - handling new addons when theyre imported
// - installing addons to tf2

impl Setup {
    fn new(
        app_dirs: App,
        operator_defaults: HashMap<&'static str, pcf::Attribute>,
        particle_system_defaults: HashMap<&'static str, pcf::Attribute>
    ) -> Self {
        Self {
            app_dirs,
            operator_defaults: Arc::new(operator_defaults),
            particle_system_defaults: Arc::new(particle_system_defaults),
        }
    }

    pub(crate) fn operation_steps() -> usize {
        (pcf_defaults::PARTICLES_MANIFEST.len() * 2) + 120
    }

    pub(crate) fn run(&self, load_operation: &ProcessState) {
        load_operation.push_status("Loading vanilla particle systems");
        let groups = Self::get_vanilla_pcf_groups_from_manifest(load_operation).unwrap();
        let vanilla_pcfs: Vec<_> = {
            groups
                .into_par_iter()
                .filter(|group| {
                    load_operation.increment_progress();
                    group.dx80.is_none() && group.dx90_slow.is_none()
                })
                .map(|group| {
                    load_operation.push_status(format!("stripping {} of unnecessary defaults", &group.default.name));
                    let pcf = group.default.pcf.defaults_stripped_nth(1000, &self.particle_system_defaults, &self.operator_defaults);
                    
                    VanillaPcfGroup {
                        default: VanillaPcf {
                            pcf,
                            ..group.default
                        },
                        ..group
                    }
                })
                .collect()
        };

        load_operation.push_status("Separating particle tree into connected graphs...");
        let bins = PcfBinMap::new(vanilla_pcfs.iter().map(default_bin_from).collect());
        let vanilla_graphs: Vec<_> = vanilla_pcfs
            .into_iter()
            .map(|group| (group.default.name, group.default.pcf.into_connected()))
            .collect();
        load_operation.add_progress(30);
        
        load_operation.push_status("Loading addons...");
        let sources = match Sources::read_dir(&self.app_dirs.addons_dir) {
            Ok(sources) => sources,
            Err(_) => todo!(),
        };
        load_operation.add_progress(30);

        if !sources.failures.is_empty() {
            todo!();
        }

        let extracted_addons: Vec<_> = sources.sources
            .into_par_iter()
            .map(|source| {
                load_operation.push_status(format!("Extracting addon {}", source.name().unwrap_or_default()));
                match source.extract_as_subfolder_in(&self.app_dirs.extracted_content_dir) {
                    Ok(extracted) => extracted,
                    Err(_) => todo!(),
                }
            })
            .collect();
        load_operation.add_progress(30);

        let mut addons = Vec::new();
        for addon in extracted_addons {
            load_operation.push_status(format!("Parsing contents of {}", addon.name().unwrap_or_default()));
            let content = match addon.parse_content() {
                Ok(content) => content,
                Err(_) => todo!(),
            };

            addons.push(content);
        }
        load_operation.add_progress(30);

        load_operation.push_status("Done!");
    }

    fn get_vanilla_pcf_groups_from_manifest(load_operation: &ProcessState) -> Result<Vec<VanillaPcfGroup>, VanillaPcfError> {
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
                    })
                }

                if let Some((name, file_path, size)) = dx90_slow_names.get(&group.default.name) {
                    println!("    Found dx90 variant, {name}. Decoding...");
                    let mut reader = File::open_buffered(file_path)?;
                    let pcf = pcf::decode(&mut reader)?;

                    group.dx90_slow = Some(VanillaPcf {
                        name: name.to_string(),
                        pcf,
                        size: *size,
                    })
                }

                Ok(group)
            })
            .collect()
    }
}

pub(crate) struct SimpleInstaller {
    tf_dir: Utf8PlatformPathBuf,

    setup_handle: JoinHandle<()>,
    load_view: ProcessView,
}

impl SimpleInstaller {
    pub(crate) fn new(ctx: &egui::Context, app_dirs: App, tf_dir: Utf8PlatformPathBuf) -> Self {
        styles::configure_fonts(ctx);
        styles::configure_text_styles(ctx);

        let operator_defaults = pcf_defaults::get_default_operator_map();
        let particle_system_defaults = pcf_defaults::get_particle_system_defaults();

        let setup = Setup::new(app_dirs, operator_defaults, particle_system_defaults);
        let (load_operation, load_view) = ProcessState::new(ctx, Setup::operation_steps().try_into().unwrap());

        let worker_handle = thread::spawn(move || {
            setup.run(&load_operation);
        });

        Self {
            tf_dir,
            setup_handle: worker_handle,
            load_view,
        }
    }

    pub(crate) fn ui(&mut self, ui: &egui::Ui) {
        self.load_view.show("simpler installer load view", ui.ctx());
    }
}

impl eframe::App for SimpleInstaller {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        CentralPanel::default().show(ctx, |ui| {
            self.ui(ui);
        });
    }
}


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

fn default_bin_from(group: &VanillaPcfGroup) -> PcfBin {
    PcfBin {
        capacity: group.default.size,
        name: group.default.name.clone(),
        pcf: Pcf::new_empty_from(&group.default.pcf),
    }
}
