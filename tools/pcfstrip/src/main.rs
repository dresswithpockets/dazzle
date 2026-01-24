#![feature(file_buffered)]
#![feature(seek_stream_len)]

mod defaults;
mod patch;

use std::{
    collections::HashMap,
    env,
    fs::{File, OpenOptions},
    io::{BufWriter, Seek, Write, stdout},
    mem,
    path::PathBuf,
    str::FromStr,
};

use byteorder::WriteBytesExt;
use bytes::{Buf, BufMut, BytesMut};
use dmx::{
    Dmx,
    attribute::{Color, Vector3},
};
use pcf::{Attribute, Pcf};

use crate::patch::PatchVpkExt;

const DEFAULT_PCF_DATA: &[u8] = include_bytes!("default_values.pcf");

fn main() -> anyhow::Result<()> {
    let paths: Vec<_> = env::args()
        .skip(1)
        .map(|path| {
            (
                path.clone(),
                format!(
                    "particles/{}",
                    PathBuf::from_str(&path).unwrap().file_name().unwrap().to_string_lossy()
                ),
            )
        })
        .collect();
    // let output_path = env::args().nth(2).expect("output pcf file path not given");

    let particle_defaults = defaults::get_particle_system_defaults();
    // let default_attribute_map = get_default_attribute_map()?;
    let operator_defaults: HashMap<&str, Attribute> = HashMap::from([
        ("operator start fadein", 0.0.into()),
        ("operator end fadein", 0.0.into()),
        ("operator start fadeout", 0.0.into()),
        ("operator end fadeout", 0.0.into()),
        ("Visibility Proxy Input Control Point Number", (-1).into()),
        ("Visibility Proxy Radius", 1.0.into()),
        ("Visibility input minimum", 0.0.into()),
        ("Visibility input maximum", 1.0.into()),
        ("Visibility Alpha Scale minimum", 0.0.into()),
        ("Visibility Alpha Scale maximum", 1.0.into()),
        ("Visibility Radius Scale minimum", 1.0.into()),
        ("Visibility Radius Scale maximum", 1.0.into()),
        ("Visibility Camera Depth Bias", 0.0.into()),
    ]);

    print!("decoding PCFs... ");
    stdout().flush()?;
    let input_pcfs: anyhow::Result<Vec<_>> = paths
        .into_iter()
        .map(|(input, output)| -> anyhow::Result<(Pcf, String)> {
            let mut file = File::open_buffered(input)?;
            let dmx = dmx::decode(&mut file)?;
            Ok((dmx.try_into()?, output))
        })
        .collect();

    let input_pcfs = input_pcfs?;
    println!("done");

    print!("stripping PCFs... ");
    stdout().flush()?;
    let input_pcfs: Vec<_> = input_pcfs
        .into_iter()
        .map(|(pcf, output)| {
            (
                pcf.defaults_stripped_nth(1000, &particle_defaults, &operator_defaults),
                output,
            )
        })
        .collect();
    println!("done");

    // whats goin on with the 431st particle system? (0-indexed)
    // let pcf = pcf.defaults_stripped_nth(611, &operator_defaults, &particle_defaults);

    print!("reordering PCFs... ");
    stdout().flush()?;
    let input_pcfs: Vec<_> = input_pcfs
        .into_iter()
        .map(|(pcf, output)| {
            let mut pcfs = pcf.into_connected();
            let mut pcf = pcfs.pop().unwrap();
            for from in pcfs {
                pcf = pcf.merged(from).unwrap();
            }

            (pcf, output)
        })
        .collect();

    println!("done");

    let vpk_path = env::var("HOME").unwrap() + "/.local/share/Steam/steamapps/common/Team Fortress 2/tf/tf2_misc_dir.vpk";
    let mut vpk = vpk::from_path(vpk_path)?;

    println!("patching PCFs... ");
    for (pcf, output) in input_pcfs {
        print!("  {output}: encoding... ");
        stdout().flush()?;
        let dmx: Dmx = pcf.into();
        let buf = BytesMut::new();
        let mut writer = buf.writer();
        dmx.encode(&mut writer)?;

        let mut reader = writer.into_inner().reader();
        print!("patching {} bytes... ", reader.get_ref().len());
        stdout().flush()?;
        vpk.patch_file(&output, reader.get_ref().len() as u64, &mut reader)?;

        println!("done");
    }
    println!("done");

    Ok(())
}

/// Decodes [`DEFAULT_PCF_DATA`] and produces a map of `functionName`, to a default attribute value map.
fn get_default_attribute_map() -> anyhow::Result<HashMap<String, HashMap<String, pcf::Attribute>>> {
    let mut reader = DEFAULT_PCF_DATA.reader();
    let dmx = dmx::decode(&mut reader)?;
    let pcf: Pcf = dmx.try_into()?;

    let (_, symbols, root) = pcf.into_parts();
    let (_, _, particle_systems, _) = root.into_parts();

    let all_operators = particle_systems
        .into_iter()
        .flat_map(|system| {
            [
                system.constraints,
                system.emitters,
                system.forces,
                system.initializers,
                system.operators,
                system.renderers,
            ]
        })
        .flatten();

    let mut operator_map = HashMap::new();
    for operator in all_operators {
        let value_map: HashMap<_, _> = operator
            .attributes
            .into_iter()
            .filter(|(name_idx, _)| {
                symbols
                    .base
                    .get_index_of("distance_bias")
                    .is_none_or(|idx| *name_idx as usize != idx)
            })
            .map(|(name_idx, attribute)| {
                let name = symbols
                    .base
                    .get_index(name_idx as usize)
                    .expect("this should never happen");
                (name.clone(), attribute)
            })
            .collect();

        operator_map.insert(operator.function_name, value_map);
    }

    Ok(operator_map)
}

fn get_particle_system_defaults() -> HashMap<&'static str, pcf::Attribute> {
    HashMap::from([
        ("batch particle systems", false.into()),
        (
            "bounding_box_min",
            Vector3((-10.0).into(), (-10.0).into(), (-10.0).into()).into(),
        ),
        (
            "bounding_box_max",
            Vector3(10.0.into(), 10.0.into(), 10.0.into()).into(),
        ),
        ("color", Color(255, 255, 255, 255).into()),
        ("control point to disable rendering if it is the camera", (-1).into()),
        ("cull_control_point", 0.into()),
        ("cull_cost", 1.0.into()),
        ("cull_radius", 0.0.into()),
        ("cull_replacement_definition", String::new().into()),
        ("group id", 0.into()),
        ("initial_particles", 0i32.into()),
        ("material", "vgui/white".to_string().into()),
        ("max_particles", 1000i32.into()),
        ("maximum draw distance", 100_000.0.into()),
        ("maximum sim tick rate", 0.0.into()),
        ("maximum time step", 0.1.into()),
        ("minimum rendered frames", 0.into()),
        ("minimum sim tick rate", 0.0.into()),
        ("radius", 5.0.into()),
        ("rotation", 0.0.into()),
        ("rotation_speed", 0.0.into()),
        ("sequence_number", 0.into()),
        ("sequence_number1", 0.into()),
        ("Sort particles", true.into()),
        ("time to sleep when not drawn", 8.0.into()),
        ("view model effect", false.into()),
    ])
}
