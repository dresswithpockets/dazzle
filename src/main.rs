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
pub mod app;
mod packing;
pub mod patch;
mod paths;
mod pcf_defaults;
mod vpk_writer;

use std::{
    fs::{self, File, Metadata},
    io::{self},
    process,
};

use bytes::{Buf, BufMut, BytesMut};
use dmx::Dmx;
use hashbrown::HashSet;
use rayon::prelude::*;
use typed_path::{Utf8PlatformPath, Utf8PlatformPathBuf};

use crate::addon::Sources;
use crate::app::AppBuilder;
use crate::{
    packing::{PcfBin, PcfBinMap},
    patch::PatchVpkExt,
};

const SPLIT_BY_2GB: u32 = 2 << 30;


const TF2_VPK_NAME: &str = "tf2_misc_dir.vpk";
const APP_INSTANCE_NAME: &str = "net.dresswithpockets.tf2dazzle.lock";
const APP_TLD: &str = "net";
const APP_ORG: &str = "dresswithpockets";
const APP_NAME: &str = "tf2dazzle";
const PARTICLE_SYSTEM_MAP: &str = include_str!("particle_system_map.json");


struct VanillaPcfInfo {
    vanilla_pcfs: Vec<VanillaPcf>,
    pcfs_with_dx80: HashSet<String>,
    pcfs_with_dx90: HashSet<String>,
}

struct VanillaPcf {
    name: String,
    pcf: pcf::new::Pcf,
    metadata: Metadata,
}

fn get_vanilla_pcf_info() -> Result<VanillaPcfInfo, io::Error> {
    let read_dir = fs::read_dir("backup/tf_particles")?;
    let mut with_dx80 = HashSet::new();
    let mut with_dx90 = HashSet::new();
    let mut with_high = HashSet::new();

    let mut entries = Vec::new();
    for entry in read_dir {
        let entry = entry?;
        let metadata = entry.metadata()?;
        if !metadata.is_file() {
            continue;
        }

        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();

        if let Some(stripped) = file_name.strip_suffix("_dx80.pcf") {
            with_dx80.insert("particles/".to_string() + stripped + ".pcf");
            continue;
        }

        if let Some(stripped) = file_name.strip_suffix("_dx90.pcf") {
            with_dx90.insert("particles/".to_string() + stripped + ".pcf");
            continue;
        }

        if let Some(stripped) = file_name.strip_suffix("_dx90_slow.pcf") {
            with_dx90.insert("particles/".to_string() + stripped + ".pcf");
            continue;
        }

        if let Some(stripped) = file_name.strip_suffix("_high.pcf") {
            with_high.insert("particles/".to_string() + stripped + ".pcf");
            continue;
        }

        if let Some(stripped) = file_name.strip_suffix("_mvm.pcf") {
            with_high.insert("particles/".to_string() + stripped + ".pcf");
            continue;
        }

        let name = "particles/".to_string() + &file_name;

        entries.push((name, entry.path(), metadata));
    }

    // try skipping all files with a matching dx80/dx90 pcf - so they remain identical to vanilla.
    // let entries: Vec<_> = entries.into_iter().filter(|(name, _, _)| !with_dx80.contains(name) && !with_dx90.contains(name)).collect();

    let pcfs: Result<Vec<VanillaPcf>, io::Error> = entries
        .into_par_iter()
        .map(|(name, file_path, metadata)| -> Result<VanillaPcf, io::Error> {
            println!("decoding {name} as DMX and converting to PCF");
            let mut reader = File::open_buffered(file_path)?;
            let dmx = dmx::decode(&mut reader).unwrap();
            let pcf = pcf::new::Pcf::try_from(dmx).unwrap();

            Ok(VanillaPcf { name, pcf, metadata })
        })
        .collect();

    Ok(VanillaPcfInfo {
        vanilla_pcfs: pcfs?,
        pcfs_with_dx80: with_dx80,
        pcfs_with_dx90: with_dx90,
    })
}

fn default_bin_from(vanilla_pcf: &VanillaPcf) -> PcfBin {
    let pcf = pcf::new::Pcf::new_empty_from(&vanilla_pcf.pcf);

    PcfBin {
        capacity: vanilla_pcf.metadata.len(),
        name: vanilla_pcf.name.clone(),
        pcf,
    }
}

fn next() -> anyhow::Result<()> {
    // TODO: open every vanilla PCF and create a list of every vanilla particle system definition that must exist

    // let operator_defaults = get_default_attribute_map()?;
    let operator_defaults = pcf_defaults::get_default_operator_map();
    let particle_system_defaults = pcf_defaults::get_particle_system_defaults();

    // vanilla PCFs have a set size, and we have to fit our particle systems into those PCFs. It doesn't matter which
    // PCF they land in so long as they fit. We're solving this using a best-fit bin packing algorithm.
    println!("loading vanilla pcf info");
    let vanilla_pcf_info = get_vanilla_pcf_info()?;
    let vanilla_pcfs: Vec<_> = vanilla_pcf_info.vanilla_pcfs
        .into_par_iter()
        .map(|vanilla_pcf| {
            println!("stripping {} of unecessary defaults", vanilla_pcf.name);
            let pcf = vanilla_pcf
                .pcf
                .defaults_stripped_nth(1000, &particle_system_defaults, &operator_defaults);
            VanillaPcf { pcf, ..vanilla_pcf }
        })
        .collect();

    println!("initializing PCF bins from the vanilla PCFs");
    let bins: Vec<PcfBin> = vanilla_pcfs.iter().map(default_bin_from).collect();
    let mut bins = PcfBinMap::new(bins);

    println!(
        "maximum PCF capacity: {}",
        bins.iter().map(|bin| bin.capacity).sum::<u64>()
    );
    println!(
        "stripped PCF load: {}",
        vanilla_pcfs
            .iter()
            .map(|vanilla_pcf| vanilla_pcf.pcf.encoded_size())
            .sum::<usize>()
    );

    // TODO: get vanilla PCF graphs, and map particle system name to PCF graph index for later lookup by vanilla system name
    println!("getting vanilla particle system map");
    let vanilla_graphs: Vec<_> = vanilla_pcfs
        .into_iter()
        .map(|vanilla_pcf| (vanilla_pcf.name, vanilla_pcf.pcf.into_connected()))
        .collect();

    println!("discovered {} vanilla particle systems", vanilla_graphs.len());

    println!("setting up app");
    let tf_dir: Utf8PlatformPathBuf = ["local_test", "tf"].iter().collect();
    let mut app = AppBuilder::with_tf_dir(tf_dir.clone()).build()?;

    // TODO: detect tf directory
    // TODO: prompt user to verify or provide their own tf directory after discovery attempt

    println!("loading addon sources");
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
    println!("extracting addon sources to working directory...");
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
    println!("parsing extracted addon content..");
    for addon in extracted_addons {
        let content = match addon.parse_content() {
            Ok(content) => content,
            Err(err) => {
                eprintln!("Couldn't parse content of some mods: {err}");
                process::exit(1);
            }
        };

        println!("parsed {}", content.source_path.file_name().unwrap());
        addons.push(content);
    }

    // first we bin-pack our addon's custom particles.
    println!("bin-packing addon particles...");
    // for addon in addons {
    //     for (path, pcf) in addon.particle_files {
    //         println!("stripping {path} of unecessary defaults");
    //         let graph = pcf.into_connected();
    //         for mut pcf in graph {
    //             println!("bin-packing a graph with '{}' elements", pcf.particle_systems().len());
    //             bins.pack_group(&mut pcf)?;
    //         }
    //     }
    // }

    // the bins don't contain any of the necessary particle systems by default, since they're supposed to be a blank
    // slate for our addons; so, we pack every vanilla particle system not present in the bins.
    println!("bin-packing missing vanilla addon particles...");
    for (name, graphs) in vanilla_graphs {
        println!("bin-packing {} graphs from {}.", graphs.len(), name);
        for mut graph in graphs {
            let missing_system: Vec<_> = graph
                .particle_systems()
                .iter()
                .filter(|system| !bins.has_system_name(&system.name))
                .map(|system| system.name.as_str())
                .collect();

            if !missing_system.is_empty() {

                // println!("{name}: bins are missing these particle systems: {}", missing_system.join(","));

                // println!("bin-packing a missing vanilla particle from {:?}", names.iter().map(|n|n.display()));
                if bins.pack_group(&mut graph).is_err() {
                    eprintln!("There wasn't enough space...");
                    let mut load = 0;
                    for bin in bins.iter() {
                        load += bin.pcf.encoded_size();
                        println!("{}: {} / {}", bin.name, bin.pcf.encoded_size(), bin.capacity);
                    }
                    println!("consumed load: {load}");
                    process::exit(1);
                }
            }
        }

        let load = bins.iter().map(|bin| bin.pcf.encoded_size()).sum::<usize>();
        println!("consumed load: {load}");
    }

    if let Err(err) = app.tf_misc_vpk.restore_particles(&app.backup_dir) {
        eprintln!("There was an error restoring some or all particles to the vanilla state: {err}");
        process::exit(1);
    }

    for bin in bins {
        println!("writing {} to vpk", bin.name);
        let dmx: Dmx = bin.pcf.into();

        let mut writer = BytesMut::new().writer();
        dmx.encode(&mut writer)?;

        let buffer = writer.into_inner();
        let size = buffer.len() as u64;
        let mut reader = buffer.reader();
        app.tf_misc_vpk.patch_file(&bin.name, size, &mut reader)?;

        if vanilla_pcf_info.pcfs_with_dx80.contains(&bin.name) {
            let name = bin.name.trim_suffix(".pcf").to_string() + "_dx80.pcf";
            let mut writer = BytesMut::new().writer();
            dmx.encode(&mut writer)?;

            let buffer = writer.into_inner();
            let size = buffer.len() as u64;
            let mut reader = buffer.reader();
            app.tf_misc_vpk.patch_file(&name, size, &mut reader)?;
        }

        if vanilla_pcf_info.pcfs_with_dx90.contains(&bin.name) {
            let name = bin.name.trim_suffix(".pcf").to_string() + "_dx90.pcf";
            let mut writer = BytesMut::new().writer();
            dmx.encode(&mut writer)?;

            let buffer = writer.into_inner();
            let size = buffer.len() as u64;
            let mut reader = buffer.reader();
            app.tf_misc_vpk.patch_file(&name, size, &mut reader)?;
        }
    }

    Ok(())
}

fn main() -> anyhow::Result<()> {
    /*
       TODO: if not already configured, detect/select a tf/ directory
       TODO: tui for configuring enabled/disabled custom particles found in addons
       TODO: tui for selecting addons to install/uninstall
       TODO: detect conflicts in selected addons
    */

    return next();

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
