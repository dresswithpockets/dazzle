#![feature(file_buffered)]
#![feature(cstr_display)]

use std::{env, fmt::Display, fs::File, process};

use pcf::{Attribute, Element, NameIndex, Pcf};
use ptree::{TreeBuilder, print_tree};

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        process::exit(1);
    }

    let path = &args[1];
    let mut file = File::open_buffered(path).unwrap();
    let pcf = Pcf::decode(&mut file).unwrap();

    let mut tree = TreeBuilder::new(path.clone());
    tree.add_empty_child(format!("Version: {}", pcf.version()));

    {
        let symbols = tree.begin_child("Symbols".to_string());

        let mut strings: Vec<_> = pcf.strings().iter().collect();
        strings.sort_by_key(|el| el.0);

        for (string, _) in strings {
            symbols.add_empty_child(string.to_string_lossy().into_owned());
        }

        symbols.end_child();
    }

    {
        let root_text = format!(
            "{} ({}, {:x?})",
            pcf.root().name.display(),
            pcf.strings()
                .get_index(pcf.root().type_idx as usize)
                .unwrap()
                .0
                .display(),
            pcf.root().signature
        );

        let mut root_systems: Vec<_> = pcf.root().definitions.iter().filter_map(|idx| pcf.get(*idx)).collect();

        root_systems.sort_by_key(|el| el.name.as_bytes());

        let root = tree.begin_child(root_text);
        create_element_children(&pcf, root, root_systems.into_iter());
        root.end_child();
    }

    let tree = tree.build();

    print_tree(&tree).unwrap();
}

fn create_element_children<'a, 't>(
    pcf: &'a Pcf,
    node: &'t mut TreeBuilder,
    elements: impl Iterator<Item = &'a Element>,
) -> &'t mut TreeBuilder {
    for element in elements {
        let label = format!(
            "{} ({}, {:x?})",
            element.name.display(),
            pcf.strings().get_index(element.type_idx as usize).unwrap().0.display(),
            element.signature
        );

        if element.attributes.is_empty() {
            node.add_empty_child(label);
        } else {
            let mut sorted_attributes: Vec<_> = element.attributes.iter().collect();
            sorted_attributes.sort_by_key(|(name_idx, _)| pcf.strings().get_index(**name_idx as usize).unwrap());

            let element_child = node.begin_child(label);
            for (name_idx, attribute) in sorted_attributes {
                create_attribute_child(pcf, element_child, *name_idx, attribute);
            }
            element_child.end_child();
        }
    }

    node
}

fn create_attribute_child(pcf: &Pcf, node: &mut TreeBuilder, name_idx: NameIndex, attribute: &Attribute) {
    fn add_array<'a>(name: String, node: &'a mut TreeBuilder, items: &[impl Display]) -> &'a mut TreeBuilder {
        let child = node.begin_child(name);
        for item in items {
            child.add_empty_child(item.to_string());
        }
        child.end_child()
    }

    let name = pcf.strings().get_name(name_idx).unwrap().to_string_lossy().into_owned();
    match attribute {
        Attribute::Element(element) => {
            let elements: [&Element; 1] = [pcf.get(*element).unwrap()];
            let child = node.begin_child(name);
            create_element_children(pcf, child, elements.into_iter());
            child.end_child()
        }
        Attribute::Integer(value) => node.add_empty_child(format!("{name}: {value}")),
        Attribute::Float(value) => node.add_empty_child(format!("{name}: {value:.2}")),
        Attribute::Bool(value) => node.add_empty_child(format!("{name}: {value}")),
        Attribute::String(value) => node.add_empty_child(format!("{name}: {}", value.display())),
        Attribute::Binary(value) => node.add_empty_child(format!("{name}: {value:#?}")),
        Attribute::Color(value) => node.add_empty_child(format!("{name}: {value}")),
        Attribute::Vector2(value) => node.add_empty_child(format!("{name}: {value}")),
        Attribute::Vector3(value) => node.add_empty_child(format!("{name}: {value}")),
        Attribute::Vector4(value) => node.add_empty_child(format!("{name}: {value}")),
        Attribute::Matrix(value) => node.add_empty_child(format!("{name}: {value}")),
        Attribute::ElementArray(items) => {
            let mut elements: Vec<_> = items.iter().map(|idx| pcf.get(*idx).unwrap()).collect();
            elements.sort_by_key(|el| el.name.as_bytes());
            let child = node.begin_child(name);
            create_element_children(pcf, child, elements.into_iter());
            child.end_child()
        }
        Attribute::IntegerArray(items) => add_array(name, node, items),
        Attribute::FloatArray(items) => {
            let child = node.begin_child(name);
            for item in items {
                child.add_empty_child(format!("{item:.2}"));
            }
            child.end_child()
        },
        Attribute::BoolArray(items) => add_array(name, node, items),
        Attribute::StringArray(items) => {
            let child = node.begin_child(name);
            for item in items {
                child.add_empty_child(item.to_string_lossy().into_owned());
            }
            child.end_child()
        }
        Attribute::BinaryArray(items) => {
            let child = node.begin_child(name);
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
