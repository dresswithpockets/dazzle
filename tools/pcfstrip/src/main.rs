#![feature(file_buffered)]
#![feature(seek_stream_len)]

mod defaults;
mod patch;

use std::{
    collections::HashMap,
    env,
    fs::File,
    io::{Write, stdout},
    path::PathBuf,
    str::FromStr,
};

use bytes::{Buf, BufMut, BytesMut};
use dmx::{
    Dmx,
};
use pcf::{Attribute, Pcf};

use crate::patch::PatchVpkExt;

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

    let vpk_path =
        env::var("HOME").unwrap() + "/.local/share/Steam/steamapps/common/Team Fortress 2/tf/tf2_misc_dir.vpk";
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
