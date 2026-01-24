#![feature(file_buffered)]
#![feature(cstr_display)]

use std::{env, fmt::Display, fs::File, process};

use dmx::SymbolIdx;
use pcf::Attribute;
use pcf::new::{Operator, ParticleSystem, Pcf};
use ptree::{TreeBuilder, print_tree};

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        process::exit(1);
    }

    let path = &args[1];
    let mut file = File::open_buffered(path).unwrap();
    let dmx = dmx::decode(&mut file).unwrap();
    let pcf = Pcf::try_from(dmx).unwrap();

    let mut tree = TreeBuilder::new(path.clone());
    tree.add_empty_child(format!("Version: {}", pcf.version()));

    {
        let symbols = tree.begin_child("Symbols".to_string());

        let mut strings: Vec<_> = pcf.symbols().base.iter().collect();
        strings.sort();

        for string in strings {
            symbols.add_empty_child(string.clone());
        }

        symbols.end_child();
    }

    {
        let root_text = format!("{} (Dme?Element, {:x?})", pcf.root().name(), pcf.root().signature());

        let mut particle_systems: Vec<_> = pcf.root().particle_systems().iter().enumerate().collect();
        // particle_systems.sort_by(|a, b| a.1.name.cmp(&b.1.name));

        let root = tree.begin_child(root_text);
        create_element_children(root, &pcf, particle_systems.into_iter());
        root.end_child();
    }

    let tree = tree.build();

    print_tree(&tree).unwrap();
}

fn create_element_children<'a, 't>(
    node: &'t mut TreeBuilder,
    pcf: &'a Pcf,
    particle_systems: impl Iterator<Item = (usize, &'a ParticleSystem)>,
) -> &'t mut TreeBuilder {
    for (system_idx, particle_system) in particle_systems {
        let label = format!(
            "#{system_idx} {} ({}, {:x?})",
            particle_system.name,
            pcf.symbols()
                .base
                .get_index(pcf.symbols().particle_system_definition as usize)
                .unwrap(),
            particle_system.signature,
        );

        let node = node.begin_child(label);

        if !particle_system.attributes.is_empty() {
            let mut attributes: Vec<_> = particle_system.attributes.iter().collect();
            attributes.sort_by_key(|(name_idx, _)| pcf.symbols().base.get_index(**name_idx as usize).unwrap());

            for (name_idx, attribute) in attributes {
                create_attribute_child(node, pcf, *name_idx, attribute);
            }
        }

        if !particle_system.children.is_empty() {
            let mut children: Vec<_> = particle_system.children.iter().collect();
            children.sort_by(|a, b| a.name.cmp(&b.name));

            let node = node.begin_child("children".to_string());
            for child in children {
                let label = format!(
                    "{} ({}, {:x?})",
                    child.name,
                    pcf.symbols()
                        .base
                        .get_index(pcf.symbols().particle_child.unwrap() as usize)
                        .unwrap(),
                    child.signature,
                );

                let node = node.begin_child(label);
                node.add_empty_child(format!("child: {}", child.child));
                for (name_idx, attribute) in &child.attributes {
                    create_attribute_child(node, pcf, *name_idx, attribute);
                }
                node.end_child();
            }

            node.end_child();
        }

        create_operator_children(node, pcf, &particle_system.constraints, "constraints");
        create_operator_children(node, pcf, &particle_system.emitters, "emitters");
        create_operator_children(node, pcf, &particle_system.forces, "forces");
        create_operator_children(node, pcf, &particle_system.initializers, "initializers");
        create_operator_children(node, pcf, &particle_system.operators, "operators");
        create_operator_children(node, pcf, &particle_system.renderers, "renderers");

        node.end_child();
    }

    node
}

fn create_operator_children(node: &mut TreeBuilder, pcf: &Pcf, operators: &[Operator], name: &str) {
    if !operators.is_empty() {
        let mut operators: Vec<_> = operators.iter().collect();
        operators.sort_by(|a, b| a.name.cmp(&b.name));

        let node = node.begin_child(name.to_string());
        for operator in operators {
            let label = format!(
                "{} ({}, {:x?})",
                operator.name,
                pcf.symbols()
                    .base
                    .get_index(pcf.symbols().particle_operator.unwrap() as usize)
                    .unwrap(),
                operator.signature,
            );

            let node = node.begin_child(label);
            node.add_empty_child(format!("function_name: {}", operator.function_name.clone()));
            for (name_idx, attribute) in &operator.attributes {
                create_attribute_child(node, pcf, *name_idx, attribute);
            }
            node.end_child();
        }
        node.end_child();
    }
}

fn create_attribute_child(node: &mut TreeBuilder, pcf: &Pcf, name_idx: SymbolIdx, attribute: &Attribute) {
    fn add_array<'a>(name: &str, node: &'a mut TreeBuilder, items: &[impl Display]) -> &'a mut TreeBuilder {
        let child = node.begin_child(name.to_owned());
        for item in items {
            child.add_empty_child(item.to_string());
        }
        child.end_child()
    }

    let name = pcf.symbols().base.get_index(name_idx as usize).unwrap();
    match attribute {
        Attribute::Integer(value) => node.add_empty_child(format!("{name}: {value}")),
        Attribute::Float(value) => node.add_empty_child(format!("{name}: {value:.2}")),
        Attribute::Bool(value) => node.add_empty_child(format!("{name}: {value}")),
        Attribute::String(value) => node.add_empty_child(format!("{name}: {value}")),
        Attribute::Binary(value) => node.add_empty_child(format!("{name}: {value:#?}")),
        Attribute::Color(value) => node.add_empty_child(format!("{name}: {value}")),
        Attribute::Vector2(value) => node.add_empty_child(format!("{name}: {value}")),
        Attribute::Vector3(value) => node.add_empty_child(format!("{name}: {value}")),
        Attribute::Vector4(value) => node.add_empty_child(format!("{name}: {value}")),
        Attribute::Matrix(value) => node.add_empty_child(format!("{name}: {value}")),
        Attribute::IntegerArray(items) => add_array(name, node, items),
        Attribute::FloatArray(items) => {
            let child = node.begin_child(name.clone());
            for item in items {
                child.add_empty_child(format!("{item:.2}"));
            }
            child.end_child()
        }
        Attribute::BoolArray(items) => add_array(name, node, items),
        Attribute::StringArray(items) => {
            let child = node.begin_child(name.clone());
            for item in items {
                child.add_empty_child(item.clone());
            }
            child.end_child()
        }
        Attribute::BinaryArray(items) => {
            let child = node.begin_child(name.clone());
            for item in items {
                child.add_empty_child(format!("{item:#?}"));
            }
            child.end_child()
        }
        Attribute::ColorArray(items) => add_array(name, node, items),
        Attribute::Vector2Array(items) => add_array(name, node, items),
        Attribute::Vector3Array(items) => add_array(name, node, items),
        Attribute::Vector4Array(items) => add_array(name, node, items),
        Attribute::MatrixArray(items) => add_array(name, node, items),
    };
}
