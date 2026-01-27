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
use pcf::{
    Attribute, Pcf,
    new::{Child, Operator},
};
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
                if fs::exists(format!("vanilla/{stem}_dx80.pcf"))?
                    || fs::exists(format!("vanilla/{stem}_dx90_slow.pcf"))?
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

fn write_graphs(writer: &mut BufWriter<File>, pcfs: Vec<VanillaPcf>) -> anyhow::Result<()> {
    writeln!(writer, "pub fn graphs() -> Box<[(String, Box<[pcf::Pcf]>)]> {{")?;
    writeln!(writer, "  use dmx::dmx::Version;")?;
    writeln!(writer, "  Vec::from([")?;
    for VanillaPcf { name, size: _, pcf } in pcfs {
        let graphs = pcf.into_connected();
        writeln!(writer, "    (")?;
        writeln!(writer, "      \"{name}\".to_string(),")?;
        writeln!(writer, "      Vec::from([")?;
        for graph in graphs {
            writeln!(writer, "        pcf::Pcf::new(")?;

            writeln!(writer, "          Version::{},", graph.version())?;

            write!(writer, "          pcf::Symbols::try_from(dmx::Symbols::from([")?;
            for symbol in &graph.symbols().base {
                write!(writer, "c\"{symbol}\".to_owned(), ")?;
            }
            writeln!(writer, "])).unwrap(),")?;

            writeln!(writer, "          pcf::Root::new(")?;
            writeln!(writer, "            \"{}\".to_string(),", graph.root().name())?;
            writeln!(writer, "            {:?},", graph.root().signature())?;

            // root particle system definitions
            write!(writer, "            ")?;
            write_vec(
                writer,
                graph.root().particle_systems(),
                |writer, system| -> anyhow::Result<()> {
                    writeln!(writer, "\n              pcf::ParticleSystem {{")?;
                    writeln!(writer, "                name: \"{}\".to_string(),", system.name)?;
                    writeln!(writer, "                signature: {:?},", system.signature)?;

                    write!(writer, "                children: ")?;
                    write_vec(writer, &system.children, |writer, child| {
                        write_child(writer, "                  ", child)
                    })?;
                    writeln!(writer, ",")?;

                    write!(writer, "                constraints: ")?;
                    write_vec(writer, &system.constraints, |writer, op| {
                        write_operator(writer, "                  ", op)
                    })?;
                    writeln!(writer, ",")?;

                    write!(writer, "                emitters: ")?;
                    write_vec(writer, &system.emitters, |writer, op| {
                        write_operator(writer, "                  ", op)
                    })?;
                    writeln!(writer, ",")?;

                    write!(writer, "                forces: ")?;
                    write_vec(writer, &system.forces, |writer, op| {
                        write_operator(writer, "                  ", op)
                    })?;
                    writeln!(writer, ",")?;

                    write!(writer, "                initializers: ")?;
                    write_vec(writer, &system.initializers, |writer, op| {
                        write_operator(writer, "                  ", op)
                    })?;
                    writeln!(writer, ",")?;

                    write!(writer, "                operators: ")?;
                    write_vec(writer, &system.operators, |writer, op| {
                        write_operator(writer, "                  ", op)
                    })?;
                    writeln!(writer, ",")?;

                    write!(writer, "                renderers: ")?;
                    write_vec(writer, &system.renderers, |writer, op| {
                        write_operator(writer, "                  ", op)
                    })?;
                    writeln!(writer, ",")?;

                    if system.attributes.is_empty() {
                        writeln!(writer, "                attributes: pcf::AttributeMap::new(),")?;
                    } else {
                        writeln!(writer, "                attributes: pcf::AttributeMap::from([")?;
                        for (name, attr) in &system.attributes {
                            write!(writer, "                  ({name}, ")?;
                            write_attribute(writer, attr)?;
                            writeln!(writer, "),")?;
                        }
                        writeln!(writer, "                ]),")?;
                    }

                    write!(writer, "              }}")?;

                    Ok(())
                },
            )?;

            writeln!(writer, ",")?;

            // root attributes
            if graph.root().attributes().is_empty() {
                writeln!(writer, "            pcf::AttributeMap::new(),")?;
            } else {
                writeln!(writer, "            pcf::AttributeMap::from([")?;
                for (name, attr) in graph.root().attributes() {
                    write!(writer, "              ({name}, ")?;
                    write_attribute(writer, attr)?;
                    writeln!(writer, "),")?;
                }
                writeln!(writer, "            ]),")?;
            }

            // end pcf::Root::new
            writeln!(writer, "          ),")?;

            // end pcf::Pcf::new
            writeln!(writer, "        ),")?;
        }
        writeln!(writer, "      ]).into_boxed_slice(),")?;
        writeln!(writer, "    ),")?;
    }
    writeln!(writer, "  ]).into_boxed_slice()")?;
    writeln!(writer, "}}")?;
    Ok(())
}

fn write_vec<I, F>(writer: &mut BufWriter<File>, v: &[I], f: F) -> anyhow::Result<()>
where
    I: Sized,
    F: Fn(&mut BufWriter<File>, &I) -> anyhow::Result<()>,
{
    if v.is_empty() {
        write!(writer, "Vec::new().into_boxed_slice()")?;
    } else {
        write!(writer, "Vec::from([")?;
        v.iter().try_fold((), |(), x| -> anyhow::Result<()> {
            f(writer, x)?;
            write!(writer, ",")?;
            Ok(())
        })?;
        write!(writer, "]).into_boxed_slice()")?;
    }

    Ok(())
}

fn write_attribute(writer: &mut BufWriter<File>, attr: &Attribute) -> anyhow::Result<()> {
    match attr {
        Attribute::Integer(value) => write!(writer, "Attribute::Integer({value}i32)")?,
        Attribute::Float(value) => write!(writer, "Attribute::Float({value}f32.into())")?,
        Attribute::Bool(value) => write!(writer, "Attribute::Bool({value}u8.into())")?,
        Attribute::String(value) => write!(writer, "Attribute::String(r\"{value}\".to_string())")?,
        Attribute::Binary(value) => write!(writer, "Attribute::Binary(Box::<[u8]>::from({value:?}))")?,
        Attribute::Color(value) => write!(
            writer,
            "Attribute::Color(dmx::Color({}u8.into(), {}u8.into(), {}u8.into(), {}u8.into()))",
            value.0, value.1, value.2, value.3
        )?,
        Attribute::Vector2(value) => write!(
            writer,
            "Attribute::Vector2(dmx::Vector2({}f32.into(), {}f32.into()))",
            value.0, value.1
        )?,
        Attribute::Vector3(value) => write!(
            writer,
            "Attribute::Vector3(dmx::Vector3({}f32.into(), {}f32.into(), {}f32.into()))",
            value.0, value.1, value.2
        )?,
        Attribute::Vector4(value) => write!(
            writer,
            "Attribute::Vector4(dmx::Vector4({}f32.into(), {}f32.into(), {}f32.into(), {}f32.into()))",
            value.0, value.1, value.2, value.3
        )?,
        Attribute::Matrix(value) => {
            write!(writer, "Attribute::Matrix(")?;
            write!(
                writer,
                "dmx::Vector4({}f32.into(), {}f32.into(), {}f32.into(), {}f32.into()), ",
                value.0.0, value.0.1, value.0.2, value.0.3
            )?;
            write!(
                writer,
                "dmx::Vector4({}f32.into(), {}f32.into(), {}f32.into(), {}f32.into()), ",
                value.1.0, value.1.1, value.1.2, value.1.3
            )?;
            write!(
                writer,
                "dmx::Vector4({}f32.into(), {}f32.into(), {}f32.into(), {}f32.into()), ",
                value.2.0, value.2.1, value.2.2, value.2.3
            )?;
            write!(
                writer,
                "dmx::Vector4({}f32.into(), {}f32.into(), {}f32.into(), {}f32.into()), ",
                value.3.0, value.3.1, value.3.2, value.3.3
            )?;
            write!(writer, ")")?;
        }
        Attribute::IntegerArray(values) => write!(writer, "Attribute::IntegerArray(Box::<[i32]>::from({values:?}))")?,
        Attribute::FloatArray(values) => {
            write!(writer, "Attribute::FloatArray(Box::<[dmx::Float]>::from({values:?}))")?
        }
        Attribute::BoolArray(values) => write!(writer, "Attribute::BoolArray(Box::<[dmx::Bool8]>::from({values:?}))")?,
        Attribute::StringArray(values) => {
            write!(writer, "Attribute::StringArray(Box::<[String]>::from([")?;
            for value in values {
                write!(writer, "r\"{value}\".to_string(), ")?;
            }
            write!(writer, "]))")?;
        }
        Attribute::BinaryArray(values) => {
            write!(writer, "Attribute::BinaryArray(Box::<[Box<[u8]>]>::from([")?;
            for value in values {
                write!(writer, "Box::<[u8]>::from({value:?}), ")?;
            }
            write!(writer, "]))")?;
        }
        Attribute::ColorArray(values) => {
            write!(writer, "Attribute::ColorArray(Box::<[dmx::Color]>::from([")?;
            for value in values {
                write!(
                    writer,
                    "dmx::Color({}u8.into(), {}u8.into(), {}u8.into(), {}u8.into())",
                    value.0, value.1, value.2, value.3
                )?;
            }
            write!(writer, "]))")?;
        }
        Attribute::Vector2Array(values) => {
            write!(writer, "Attribute::Vector2Array(Box::<[dmx::Vector2]>::from([")?;
            for value in values {
                write!(writer, "dmx::Vector2({}f32.into(), {}f32.into())", value.0, value.1)?;
            }
            write!(writer, "]))")?;
        }
        Attribute::Vector3Array(values) => {
            write!(writer, "Attribute::Vector3Array(Box::<[dmx::Vector3]>::from([")?;
            for value in values {
                write!(
                    writer,
                    "dmx::Vector3({}f32.into(), {}f32.into(), {}f32.into())",
                    value.0, value.1, value.2
                )?;
            }
            write!(writer, "]))")?;
        }
        Attribute::Vector4Array(values) => {
            write!(writer, "Attribute::Vector4Array(Box::<[dmx::Vector4]>::from([")?;
            for value in values {
                write!(
                    writer,
                    "dmx::Vector4({}f32.into(), {}f32.into(), {}f32.into(), {}f32.into())",
                    value.0, value.1, value.2, value.3
                )?;
            }
            write!(writer, "]))")?;
        }
        Attribute::MatrixArray(values) => {
            write!(writer, "Attribute::MatrixArray(Box::<[dmx::Matrix]>::from([")?;
            for value in values {
                write!(writer, "Attribute::Matrix(")?;
                write!(
                    writer,
                    "dmx::Vector4({}f32.into(), {}f32.into(), {}f32.into(), {}f32.into()), ",
                    value.0.0, value.0.1, value.0.2, value.0.3
                )?;
                write!(
                    writer,
                    "dmx::Vector4({}f32.into(), {}f32.into(), {}f32.into(), {}f32.into()), ",
                    value.1.0, value.1.1, value.1.2, value.1.3
                )?;
                write!(
                    writer,
                    "dmx::Vector4({}f32.into(), {}f32.into(), {}f32.into(), {}f32.into()), ",
                    value.2.0, value.2.1, value.2.2, value.2.3
                )?;
                write!(
                    writer,
                    "dmx::Vector4({}f32.into(), {}f32.into(), {}f32.into(), {}f32.into()), ",
                    value.3.0, value.3.1, value.3.2, value.3.3
                )?;
                write!(writer, "), ")?;
            }
            write!(writer, "]))")?;
        }
    }
    Ok(())
}

fn write_child(writer: &mut BufWriter<File>, depth: &str, child: &Child) -> anyhow::Result<()> {
    writeln!(writer, "\n{depth}pcf::Child {{")?;
    writeln!(writer, "{depth}  name: \"{}\".to_string(),", child.name)?;
    writeln!(writer, "{depth}  signature: {:?},", child.signature)?;
    writeln!(writer, "{depth}  child: {}u32.into(),", child.child.inner())?;

    if child.attributes.is_empty() {
        writeln!(writer, "{depth}  attributes: pcf::AttributeMap::new(),")?;
    } else {
        writeln!(writer, "{depth}  attributes: pcf::AttributeMap::from([")?;
        for (name, attr) in &child.attributes {
            write!(writer, "{depth}    ({name}, ")?;
            write_attribute(writer, attr)?;
            writeln!(writer, "),")?;
        }
        writeln!(writer, "{depth}  ]),")?;
    }

    write!(writer, "{depth}}}")?;

    Ok(())
}

fn write_operator(writer: &mut BufWriter<File>, depth: &str, operator: &Operator) -> anyhow::Result<()> {
    writeln!(writer, "\n{depth}pcf::Operator {{")?;
    writeln!(writer, "{depth}  name: \"{}\".to_string(),", operator.name)?;
    writeln!(
        writer,
        "{depth}  function_name: \"{}\".to_string(),",
        operator.function_name
    )?;
    writeln!(writer, "{depth}  signature: {:?},", operator.signature)?;
    if operator.attributes.is_empty() {
        writeln!(writer, "{depth}  attributes: pcf::AttributeMap::new(),")?;
    } else {
        writeln!(writer, "{depth}  attributes: pcf::AttributeMap::from([")?;
        for (name, attr) in &operator.attributes {
            write!(writer, "{depth}    ({name}, ")?;
            write_attribute(writer, attr)?;
            writeln!(writer, "),")?;
        }
        writeln!(writer, "{depth}  ]),")?;
    }
    write!(writer, "{depth}}}")?;

    Ok(())
}
