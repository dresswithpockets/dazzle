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

    // return;

    // println!("{path}");
    // println!("│  Version: {}", pcf.version);

    // {
    //     let mut strings: Vec<_> = pcf.strings.iter().collect();
    //     strings.sort_by_key(|el| el.0);

    //     println!("├─ Symbols");
    //     for (string, _) in &strings[..strings.len()-1] {
    //         println!("│  │  {}", string.display());
    //     }

    //     println!("│  └─ {}", strings[strings.len()-1].0.display());
    // }

    // {
    //     println!(
    //         "└─ {} ({}, {:x?})",
    //         pcf.root.name.display(),
    //         pcf.strings.get_index(pcf.root.type_idx as usize).unwrap().0.display(),
    //         pcf.root.signature
    //     );

    //     // println!("   │  type: {}", pcf.strings.get_index(pcf.root.type_idx as usize).unwrap().0.display());
    //     // println!("   │  signature: {:x?}", pcf.root.signature);
    //     // println!("   └─ root particle systems ({})", pcf.root.definitions.len());

    //     let mut root_systems: Vec<_> = pcf.root.definitions
    //         .iter()
    //         .filter_map(|idx| pcf.elements.get(*idx as usize - 1))
    //         .collect();

    //     root_systems.sort_by_key(|el| el.name.as_bytes());

    //     if root_systems.is_empty() {
    //         return;
    //     }

    //     if root_systems.len() > 1 {
    //         for element in &root_systems[..root_systems.len()-1] {
    //             println!(
    //                 "   │  {} ({}, {:x?})",
    //                 element.name.display(),
    //                 pcf.strings.get_index(element.type_idx as usize).unwrap().0.display(),
    //                 hex_fmt::HexFmt(element.signature),
    //             );
    //         }
    //     }

    //     let element = root_systems[root_systems.len() - 1];
    //     println!(
    //         "   └─ {} ({}, {:x?})",
    //         element.name.display(),
    //         pcf.strings.get_index(element.type_idx as usize).unwrap().0.display(),
    //         hex_fmt::HexFmt(element.signature),
    //     );
    // }
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
        Attribute::Float(value) => node.add_empty_child(format!("{name}: {value}")),
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
        Attribute::FloatArray(items) => add_array(name, node, items),
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

// fn print_branch(prefix: u64, terminal: bool) {
//     print!("      ");
//     for _ in 0..depth {

//     }

//     if terminal {
//         print!("└─ ");
//     } else {
//         print!("│  ");
//     }
// }

// fn print_element_tree(pcf: &Pcf, prefix: &str, elements: &[Element]) {
//     fn print_element(pcf: &Pcf, element: &Element) {
//         println!(
//             "{} ({}, {:x?})",
//             element.name.display(),
//             pcf.strings.get_index(element.type_idx as usize).unwrap().0.display(),
//             hex_fmt::HexFmt(element.signature),
//         );
//     }

//     if elements.is_empty() {
//         return;
//     }

//     if elements.len() > 1 {
//         for element in &elements[..elements.len()-1] {
//             print_branch(prefix, false);
//             print_element(pcf, element);

//             let mut sorted_attributes: Vec<_> = element.attributes.iter().collect();
//             sorted_attributes.sort_by_key(|(name_idx, _)| pcf.strings.get_index(**name_idx as usize).unwrap());
//             print_attributes(pcf, depth + 1, &sorted_attributes[..]);
//         }
//     }

//     let element = &elements[elements.len() - 1];
//     print_branch(depth + 1, true);
//     print_element(pcf, element);
// }

// fn print_attributes(pcf: &Pcf, depth: u64, attributes: &[(&NameIndex, &Attribute)]) {
//     if attributes.is_empty() {
//         return;
//     }

//     if attributes.len() > 1 {
//         for (name_idx, attribute) in &attributes[..attributes.len()-1] {
//             print_attribute(pcf, depth, false, **name_idx, attribute);
//         }
//     }

//     let (name_idx, attribute) = &attributes[attributes.len() - 1];
//     print_attribute(pcf, depth, true, **name_idx, attribute);
// }

// fn print_attribute(pcf: &Pcf, depth: u64, terminal: bool, name_idx: NameIndex, attribute: &Attribute) {
//     print_branch(depth, terminal);
//     match attribute {
//         Attribute::Element(element) => {
//             let idx = *element as usize;
//             print_element_tree(pcf, depth + 1, &pcf.elements[idx..idx+1])
//         },
//         Attribute::Integer(integer) => todo!(),
//         Attribute::Float(float) => todo!(),
//         Attribute::Bool(bool8) => todo!(),
//         Attribute::String(cstring) => todo!(),
//         Attribute::Binary(binary) => todo!(),
//         Attribute::Color(color) => todo!(),
//         Attribute::Vector2(vector2) => todo!(),
//         Attribute::Vector3(vector3) => todo!(),
//         Attribute::Vector4(vector4) => todo!(),
//         Attribute::Matrix(matrix) => todo!(),
//         Attribute::ElementArray(elements) => todo!(),
//         Attribute::IntegerArray(integers) => todo!(),
//         Attribute::FloatArray(floats) => todo!(),
//         Attribute::BoolArray(bool8s) => todo!(),
//         Attribute::StringArray(cstrings) => todo!(),
//         Attribute::BinaryArray(binaries) => todo!(),
//         Attribute::ColorArray(colors) => todo!(),
//         Attribute::Vector2Array(vector2s) => todo!(),
//         Attribute::Vector3Array(vector3s) => todo!(),
//         Attribute::Vector4Array(vector4s) => todo!(),
//         Attribute::MatrixArray(matrices) => todo!(),
//     }
// }
