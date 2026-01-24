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
#![feature(lock_value_accessors)]

mod app;
mod packing;
mod patch;
mod pcf_defaults;
mod styles;
mod vpk_writer;

use core::f32;
use std::{
    collections::{BTreeMap, HashMap},
    env::{self, consts::OS},
    fs::{self, File},
    io::{self, ErrorKind},
    path::PathBuf,
    sync::Arc,
};

use addon::Sources;
use bytes::{Buf, BufMut, BytesMut};
use directories::ProjectDirs;
use dmx::Dmx;
use eframe::egui::{self, Align2, CentralPanel, FontFamily, RichText, TextEdit, TextStyle, Vec2b, Window};
use pcf::Pcf;
use rayon::prelude::*;
use thiserror::Error;
use typed_path::{Utf8PlatformPath, Utf8PlatformPathBuf};

use crate::app::{App, BuildError};
use crate::{
    packing::{PcfBin, PcfBinMap},
    patch::PatchVpkExt,
};

const SPLIT_BY_2GB: u32 = 2 << 30;

const TF2_VPK_NAME: &str = "tf2_misc_dir.vpk";
const APP_INSTANCE_NAME: &str = "net.dresswithpockets.dazzletf2.lock";
const APP_TLD: &str = "net";
const APP_ORG: &str = "dresswithpockets";
const APP_NAME: &str = "dazzletf2";
const PARTICLE_SYSTEM_MAP: &str = include_str!("particle_system_map.json");

// TODO:
//   - discover user's tf/ directory
//   - prompt user for confirmation that the tf/ directory is correct, or to choose their own
//   - display list of addons
//   - let user choose which addons to enable/disable
//   - simple conflict resolution mode:
//      - let user reorder addons - higher priority addons get priority first
//   - advanced conflict resolution mode:
//      - for particle systems, let user choose specific groups of particle systems that conflict with one another
//          - if two addon PCFs provide overrides for the same root vanilla particle system, then they are in conflict
//      - for all other mod files, let users choose the specific files

// impl Dazzle {
//         // let table = TableBuilder::new();
//     fn addon_table_ui(&mut self, ui: &mut egui::Ui) {
//         let available_height = ui.available_height();
//         let table = TableBuilder::new(ui)
//             .striped(true)
//             .resizable(true)
//             .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
//             .column(Column::auto())
//             .column(Column::remainder())
//             .min_scrolled_height(0.0)
//             .max_scroll_height(available_height);

//         table
//             .header(20.0, |mut header| {
//                 header.col(|ui| {
//                     ui.strong("Name");
//                 });
//                 header.col(|ui| {
//                     ui.strong("Enabled");
//                 });
//             })
//             .body(|mut body| {
//                 body.row(18.0, |mut row| {
//                     row.col(|ui| {
//                         ui.label("addon 1");
//                     });
//                     row.col(|ui| {
//                         ui.label("Yes");
//                     });
//                 });

//                 body.row(18.0, |mut row| {
//                     row.col(|ui| {
//                         ui.label("addon 2");
//                     });
//                     row.col(|ui| {
//                         ui.label("No");
//                     });
//                 });
//             });

//     }
// }

const APP_ID: &str = "net.dresswithpockets.dazzletf2";

fn present_fatal_error_dialogue(err: BuildError) {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_app_id(APP_ID)
            .with_inner_size([640.0, 360.0])
            .with_title("Error starting up Dazzle")
            .with_resizable(false),
        ..Default::default()
    };

    let _ = eframe::run_simple_native(APP_ID, native_options.clone(), move |ctx, _frame| {
        egui_extras::install_image_loaders(ctx);
        CentralPanel::default().show(ctx, |ui| {
            Window::new("fatal error")
                .title_bar(false)
                .min_size((426.0, 240.0))
                .resizable(false)
                .collapsible(false)
                .movable(false)
                .anchor(Align2::CENTER_CENTER, (0.0, 0.0))
                .show(ui.ctx(), |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.image(egui::include_image!("static/images/warning.png"));
                        ui.heading(format!(
                            "Dazzle tried to start up but there was an error preventing it from continuing: \n\n{err}"
                        ))
                    });
                    ui.vertical_centered(|ui| {
                        if ui.button("Close").clicked() {
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    })
                });
        });
    });
}

fn main() -> anyhow::Result<()> {
    /*
       TODO: if not already configured, detect/select a tf/ directory
       TODO: tui for configuring enabled/disabled custom particles found in addons
       TODO: tui for selecting addons to install/uninstall
       TODO: detect conflicts in selected addons
    */

    /*
       Setup/Ensure base data_local_dir

       Are we in first time setup? (config.toml must exist in data dir)
           show first time setup with tf/ dir chooser & save config.toml

       Are we configured?
           Is our version compatible with the version indicated by config.toml?
               if not, try upgrading. If upgrading fails, show first-time setup again

           Is the tf/ dir still valid?
               if not, show first time setup

           Create working directories if they dont exist (content, vpk, addons)
           Setup path strings (backup, tf/custom, tf/tf2_misc_dir.vpk)
           Show installer viewport
    */

    let app = match App::new() {
        Ok(app) => app,
        Err(err) => {
            present_fatal_error_dialogue(err);
            std::process::exit(1);
        }
    };

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_app_id(APP_ID)
            .with_inner_size([1280.0, 720.0])
            .with_drag_and_drop(true)
            .with_title("Welcome to Dazzle!")
            .with_resizable(false),
        ..Default::default()
    };

    let _ = eframe::run_native(
        APP_ID,
        native_options,
        Box::new(|cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            styles::configure_fonts(&cc.egui_ctx);
            styles::configure_text_styles(&cc.egui_ctx);
            Ok(Box::new(app))
        }),
    );

    // let mut tf_dir = None;
    // let _ = eframe::run_native(
    //     APP_ID,
    //     native_options.clone(),
    //     Box::new(|cc| {
    //         egui_extras::install_image_loaders(&cc.egui_ctx);
    //         Ok(Box::new(view_tf_dir::TfDirPicker::new(
    //             cc,
    //             &mut tf_dir,
    //             get_default_platform_tf_dir(),
    //         )))
    //     }),
    // );

    // let Some(tf_dir) = tf_dir else {
    //     eprintln!("the tf directory picker viewport was closed without choosing valid tf directory.");
    //     return Ok(());
    // };

    // let _ = eframe::run_native(
    //     APP_ID,
    //     native_options,
    //     Box::new(|cc| {
    //         egui_extras::install_image_loaders(&cc.egui_ctx);
    //         Ok(Box::new(view_installer::SimpleInstaller::new(
    //             &cc.egui_ctx,
    //             app_dirs,
    //             tf_dir,
    //         )))
    //     }),
    // );

    return Ok(());

    // return eframe::run_native("net.dresswithpockets.dazzle", native_options, Box::new(|_cc| {
    //     Ok(Box::from(Dazzle{
    //         view: View::TfDirPicker(TfDirPicker::new(tf_dir))
    //     }))
    // }));

    // return next();

    // TODO: detect tf directory
    // TODO: prompt user to verify or provide their own tf directory after discovery attempt

    // TODO: evaluate the contents of each extracted addon to ensure they're valid
    // TODO: evaluate if there are any conflicting particles in each addon, and warn the user
    //       for now we're just assuming there are no conflicts

    // TODO: filter out PCFs based on user selection, for now we'll just pick the first one in the list if there are conflicting PCFs

    // HACK: blood_trail.pcf is really small; even a minor change to it can cause it to be too big for VPK patching.
    // TF2 doesn't really care in which PCF the particle system is defined. So, we can just rename blood_trail.pcf to
    // npc_fx.pcf.

    // TODO: if feature = "split_item_fx_pcf" then we need to merge split-up particles - this may not even be necessary if we scrap item_fx splitting completely

    // TODO: de-duplicate elements in item_fx.pcf, halloween.pcf, bigboom.pcf, and dirty_explode.pcf.
    //       NB we dont need to do this for any PCFs already in our present_pcfs map
    //       NBB we can just do the usual routine of: decode, filter by particle_system_map, and reindex
    //           - once done, we can just add these PCFs to processed_pcfs

    let pcfs_with_duplicate_effects = [
        "particles/item_fx.pcf",
        "particles/halloween.pcf",
        "particles/bigboom.pcf",
        "particles/dirty_explode.pcf",
    ];

    // TODO: compute size without writing the entire PCF to a buffer in-memory
    // for (new_path, processed_pcf) in processed_pcfs {
    //     let mut writer = BytesMut::new().writer();
    //     processed_pcf.encode(&mut writer)?;

    //     let buffer = writer.into_inner();
    //     let size = buffer.len() as u64;
    //     let mut reader = buffer.reader();
    //     app.tf_misc_vpk.patch_file(&new_path, size, &mut reader)?;
    // }

    // we can finally generate our _dazzle_addons VPKs from our addon contents.
    // vpk_writer::pack_directory(&app.working_vpk_dir, &app.tf_custom_dir, "_dazzle_addons", SPLIT_BY_2GB)?;

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
                fs::create_dir(&new_out_dir).or_else(|result| {
                    if result.kind() == ErrorKind::AlreadyExists {
                        Ok(())
                    } else {
                        Err(result)
                    }
                })?;

                visit(&path, &new_out_dir)?;
            } else if metadata.is_file() {
                let path = entry.path();
                let path = paths::to_typed(&path).absolutize()?;
                let new_out_path = out_dir.join(path.strip_prefix(in_dir)?);
                fs::copy(&path, &new_out_path)?;
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
            if let Err(err) = fs::create_dir(&new_out_dir)
                && err.kind() != io::ErrorKind::AlreadyExists
            {
                return Err(err.into());
            }

            visit(&path, &new_out_dir)?;
        }
    }

    Ok(())
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

fn get_vanilla_pcf_groups_from_manifest() -> Result<Vec<VanillaPcfGroup>, VanillaPcfError> {
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

fn default_bin_from(group: &VanillaPcfGroup) -> PcfBin {
    PcfBin {
        capacity: group.default.size,
        name: group.default.name.clone(),
        pcf: Pcf::new_empty_from(&group.default.pcf),
    }
}

// fn next() -> anyhow::Result<()> {
//     // TODO: open every vanilla PCF and create a list of every vanilla particle system definition that must exist

//     // let operator_defaults = get_default_attribute_map()?;
//     let operator_defaults = pcf_defaults::get_default_operator_map();
//     let particle_system_defaults = pcf_defaults::get_particle_system_defaults();

//     // vanilla PCFs have a set size, and we have to fit our particle systems into those PCFs. It doesn't matter which
//     // PCF they land in so long as they fit. We're solving this using a best-fit bin packing algorithm.
//     println!("loading vanilla pcf info");
//     let vanilla_groups = get_vanilla_pcf_groups_from_manifest()?;
//     let vanilla_pcfs: Vec<_> = vanilla_groups
//         .into_par_iter()
//         .filter(|group| group.dx80.is_none() && group.dx90_slow.is_none())
//         .map(|group| {
//             if pcf_defaults::PARTICLES_MANIFEST.iter().all(|el| *el != group.default.name) {
//                 println!("warning! {} is not in particle manifest!", &group.default.name);
//             }

//             println!("stripping {} of unecessary defaults", &group.default.name);
//             let pcf = group.default.pcf.defaults_stripped_nth(1000, &particle_system_defaults, &operator_defaults);
//             VanillaPcfGroup {
//                 default: VanillaPcf {
//                     pcf,
//                     ..group.default
//                 },
//                 ..group
//             }
//         })
//         .collect();

//     println!("initializing PCF bins from the vanilla PCFs");
//     let bins: Vec<PcfBin> = vanilla_pcfs.iter().map(default_bin_from).collect();
//     let mut bins = PcfBinMap::new(bins);

//     println!(
//         "maximum PCF capacity: {}",
//         bins.iter().map(|bin| bin.capacity).sum::<u64>()
//     );
//     println!(
//         "stripped PCF load: {}",
//         vanilla_pcfs
//             .iter()
//             .map(|group| group.default.pcf.encoded_size())
//             .sum::<usize>()
//     );

//     // TODO: get vanilla PCF graphs, and map particle system name to PCF graph index for later lookup by vanilla system name
//     println!("getting vanilla particle system map");
//     let vanilla_graphs: Vec<_> = vanilla_pcfs
//         .into_iter()
//         .map(|group| (group.default.name, group.default.pcf.into_connected()))
//         .collect();

//     println!("discovered {} vanilla particle systems", vanilla_graphs.len());

//     println!("setting up app");
//     let tf_dir: Utf8PlatformPathBuf = ["local_test", "tf"].iter().collect();
//     let mut app = AppBuilder::with_tf_dir(tf_dir.clone()).build()?;

//     // TODO: detect tf directory
//     // TODO: prompt user to verify or provide their own tf directory after discovery attempt

//     println!("loading addon sources");
//     let sources = match Sources::read_dir(&app.addons_dir) {
//         Ok(sources) => sources,
//         Err(err) => {
//             eprintln!("Couldn't open some addons: {err}");
//             process::exit(1);
//         }
//     };

//     for (path, err) in &sources.failures {
//         eprintln!(
//             "There was an error reading the addon source '{}': {err}",
//             path.display()
//         );
//     }

//     // to simplify processing and copying data from addons, we extract it before hand.
//     // this means the interface into each addon becomes effectively identical - we can just read/write to them as normal
//     // files without modifying the original addon files.
//     println!("extracting addon sources to working directory...");
//     let mut extracted_addons = Vec::new();
//     for source in sources.sources {
//         let extracted = match source.extract_as_subfolder_in(&app.extracted_content_dir) {
//             Ok(extracted) => extracted,
//             Err(err) => {
//                 eprintln!("Couldn't extract some mods: {err}");
//                 process::exit(1);
//             }
//         };

//         extracted_addons.push(extracted);
//     }

//     let mut addons = Vec::new();
//     println!("parsing extracted addon content..");
//     for addon in extracted_addons {
//         let content = match addon.parse_content() {
//             Ok(content) => content,
//             Err(err) => {
//                 eprintln!("Couldn't parse content of some mods: {err}");
//                 process::exit(1);
//             }
//         };

//         println!("parsed {}", content.source_path.file_name().unwrap());
//         addons.push(content);
//     }

//     // first we bin-pack our addon's custom particles.
//     println!("bin-packing addon particles...");
//     for addon in addons {
//         for (path, pcf) in addon.particle_files {
//             // println!("stripping {path} of unecessary defaults");
//             let graph = pcf.into_connected();
//             for mut pcf in graph {
//                 bins.pack_group(&mut pcf)?;
//             }
//         }

//         copy_addon_structure(&addon.content_path, &app.working_vpk_dir)?;
//     }

//     // the bins don't contain any of the necessary particle systems by default, since they're supposed to be a blank
//     // slate for our addons; so, we pack every vanilla particle system not present in the bins.
//     println!("bin-packing missing vanilla addon particles...");
//     for (name, graphs) in vanilla_graphs {
//         println!("bin-packing {} graphs from {}.", graphs.len(), name);
//         for mut graph in graphs {
//             let missing_system: Vec<_> = graph
//                 .particle_systems()
//                 .iter()
//                 .filter(|system| !bins.has_system_name(&system.name))
//                 .map(|system| system.name.as_str())
//                 .collect();

//             if !missing_system.is_empty() {

//                 // println!("{name}: bins are missing these particle systems: {}", missing_system.join(","));

//                 // println!("bin-packing a missing vanilla particle from {:?}", names.iter().map(|n|n.display()));
//                 if bins.pack_group(&mut graph).is_err() {
//                     eprintln!("There wasn't enough space...");
//                     let mut load = 0;
//                     for bin in bins.iter() {
//                         load += bin.pcf.encoded_size();
//                         println!("{}: {} / {}", bin.name, bin.pcf.encoded_size(), bin.capacity);
//                     }
//                     println!("consumed load: {load}");
//                     process::exit(1);
//                 }
//             }
//         }

//         let load = bins.iter().map(|bin| bin.pcf.encoded_size()).sum::<usize>();
//         println!("consumed load: {load}");
//     }

//     if let Err(err) = app.tf_misc_vpk.restore_particles(&app.backup_dir) {
//         eprintln!("There was an error restoring some or all particles to the vanilla state: {err}");
//         process::exit(1);
//     }

//     for bin in bins {
//         println!("writing {} to vpk", bin.name);
//         let dmx: Dmx = bin.pcf.into();

//         let mut writer = BytesMut::new().writer();
//         dmx.encode(&mut writer)?;

//         let buffer = writer.into_inner();
//         let size = buffer.len() as u64;
//         let mut reader = buffer.reader();
//         app.tf_misc_vpk.patch_file(&bin.name, size, &mut reader)?;
//     }

//     println!("creating _dazzle_addons.vpk...");
//     // we can finally generate our _dazzle_addons VPKs from our addon contents.
//     vpk_writer::pack_directory(&app.working_vpk_dir, &app.tf_custom_dir, "_dazzle_addons", SPLIT_BY_2GB)?;

//     Ok(())
// }
