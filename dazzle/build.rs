#![feature(trim_prefix_suffix)]
#![feature(file_buffered)]
#![feature(seek_stream_len)]

use std::{
    collections::HashMap,
    env,
    fs::{self, File, OpenOptions},
    io::{BufWriter, Seek, Write},
};

use byteorder::{LittleEndian, WriteBytesExt};
use dmx::Dmx;
use pcf::Pcf;
use typed_path::Utf8PlatformPathBuf;

struct VanillaPcf {
    name: String,
    size: u64,
    pcf: Pcf,
}

fn main() -> anyhow::Result<()> {
    println!("cargo:rerun-if-changed=vanilla/particles/");
    let manifest = keyvalues_parser::parse(include_str!("vanilla/particles/particles_manifest.txt"))?;
    assert_eq!("particles_manifest", manifest.key);

    let mut pcfs = Vec::new();
    for (key, values) in manifest.value.unwrap_obj().iter() {
        assert_eq!("file", key, "the manifest should only contain 'file' entries");

        for value in values {
            let value = value.get_str().expect("'file' entries must be strings");
            // we only want to handle files which are guaranteed to be loaded & cached on startup\
            // TODO: investigate if non-! can be loaded on preload
            if let Some(file) = value.strip_prefix('!') {
                if file == "particles/error.pcf" {
                    continue;
                }

                let stem = file.trim_suffix(".pcf");
                if cfg!(feature = "skip-with-dx80-dx90_slow")
                    && (fs::exists(format!("vanilla/{stem}_dx80.pcf"))?
                        || fs::exists(format!("vanilla/{stem}_dx90_slow.pcf"))?)
                {
                    continue;
                }

                let path = format!("vanilla/{file}");
                let mut reader = File::open_buffered(path)?;
                let size = reader.stream_len()?;
                let dmx = dmx::decode(&mut reader)?;
                let pcf: Pcf = dmx.try_into()?;

                pcfs.push(VanillaPcf {
                    name: file.to_string(),
                    size,
                    pcf,
                });
            }
        }
    }

    let out_dir = Utf8PlatformPathBuf::from(env::var("OUT_DIR")?);
    let manifest_rs = out_dir.join("particles_manifest.rs");
    let file = OpenOptions::new()
        .truncate(true)
        .create(true)
        .write(true)
        .open(manifest_rs)?;
    let mut writer = BufWriter::new(file);

    // `bins` returns a `Box<[pcfpack::Bin]>` for vanilla PCF bins.
    write_bins(&mut writer, &pcfs)?;
    writer.flush()?;

    {
        let particle_defaults = get_particle_system_defaults();
        let operator_defaults = get_default_operator_map();
        let graph_ron = out_dir.join("particles.graph");
        let graphs_file = OpenOptions::new()
            .truncate(true)
            .create(true)
            .write(true)
            .open(graph_ron)?;
        let mut writer = BufWriter::new(graphs_file);
        for VanillaPcf { name, size: _, pcf } in pcfs {
            writer.write_u64::<LittleEndian>(name.len() as u64)?;
            writer.write_all(name.as_bytes())?;

            let pcf = pcf.defaults_stripped_nth(1000, &particle_defaults, &operator_defaults);

            let graphs = pcf.into_connected();
            writer.write_u64::<LittleEndian>(graphs.len() as u64)?;
            for graph in graphs {
                let dmx: Dmx = graph.into();
                dmx.encode(&mut writer)?;
            }
        }

        writer.flush()?;
    }

    Ok(())
}

pub(crate) fn get_default_operator_map() -> HashMap<&'static str, pcf::Attribute> {
    HashMap::from([
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
    ])
}

pub(crate) fn get_particle_system_defaults() -> HashMap<&'static str, pcf::Attribute> {
    HashMap::from([
        // ("batch particle systems", false.into()),
        (
            "bounding_box_min",
            dmx::Vector3((-10.0).into(), (-10.0).into(), (-10.0).into()).into(),
        ),
        (
            "bounding_box_max",
            dmx::Vector3(10.0.into(), 10.0.into(), 10.0.into()).into(),
        ),
        ("color", dmx::Color(255, 255, 255, 255).into()),
        ("control point to disable rendering if it is the camera", (-1).into()),
        ("cull_control_point", 0.into()),
        ("cull_cost", 1.0.into()),
        ("cull_radius", 0.0.into()),
        ("cull_replacement_definition", String::new().into()),
        ("group id", 0.into()),
        ("initial_particles", 0i32.into()),
        ("max_particles", 1000i32.into()),
        ("material", "vgui/white".to_string().into()),
        ("max_particles", 1000.into()),
        ("maximum draw distance", 100_000.0.into()),
        ("maximum sim tick rate", 0.0.into()),
        ("maximum time step", 0.1.into()),
        ("minimum rendered frames", 0.into()),
        ("minimum sim tick rate", 0.0.into()),
        ("preventNameBasedLookup", false.into()),
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

fn write_bins(writer: &mut BufWriter<File>, pcfs: &[VanillaPcf]) -> anyhow::Result<()> {
    writeln!(writer, "pub fn bins() -> Box<[pcfpack::Bin]> {{")?;
    writeln!(writer, "  use dmx::dmx::Version;")?;
    writeln!(writer, "  Vec::from([")?;
    for VanillaPcf { name, size, pcf } in pcfs {
        writeln!(writer, "    pcfpack::Bin::new(")?;
        writeln!(writer, "      {size},")?;
        writeln!(writer, "      \"{name}\".to_string(),")?;

        write!(writer, "      pcf::Pcf::new(Version::{}, ", pcf.version())?;
        write!(writer, "pcf::Symbols::default(), ")?;

        write!(writer, "pcf::Root::new(\"{}\".to_string(), [", pcf.root().name())?;
        for value in pcf.root().signature() {
            write!(writer, "{value}, ")?;
        }
        writeln!(
            writer,
            "], Vec::new().into_boxed_slice(), pcf::new::AttributeMap::new())),"
        )?;

        writeln!(writer, "    ),")?;
    }
    writeln!(writer, "  ]).into_boxed_slice()")?;
    writeln!(writer, "}}")?;
    Ok(())
}
