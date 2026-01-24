#![feature(file_buffered)]
#![feature(cstr_display)]

use std::{
    any::Any,
    collections::HashMap,
    env,
    fmt::{self, Write},
    fs::File,
    process,
};

use nanoserde::SerJson;
use pcf::Pcf;

type NameIndexString = String;
type FunctionName = String;

const TYPE_MAP: [&str; 28] = [
    "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15", "16", "17", "18", "19", "20",
    "21", "22", "23", "24", "25", "26", "27", "28",
];

pub struct Attribute(pcf::Attribute);
impl SerJson for Attribute {
    fn ser_json(&self, d: usize, s: &mut nanoserde::SerJsonState) {
        s.st_pre();

        s.field(d, "type");
        s.out.push_str(TYPE_MAP[self.0.as_type() as usize]);
        s.conl();

        match &self.0 {
            pcf::Attribute::Element(element_idx) => s.string(element_idx.inner()),
            pcf::Attribute::Integer(integer) => s.string(integer),
            pcf::Attribute::Float(float) => _ = write!(&mut s.out, "{float:.2}"),
            pcf::Attribute::Bool(bool8) => s.string(bool8),
            pcf::Attribute::String(cstring) => s.string(cstring.display()),
            pcf::Attribute::Binary(items) => items.ser_json(d, s),
            pcf::Attribute::Color(color) => todo!(),
            pcf::Attribute::Vector2(vector2) => todo!(),
            pcf::Attribute::Vector3(vector3) => todo!(),
            pcf::Attribute::Vector4(vector4) => todo!(),
            pcf::Attribute::Matrix(matrix) => todo!(),
            pcf::Attribute::ElementArray(items) => todo!(),
            pcf::Attribute::IntegerArray(items) => todo!(),
            pcf::Attribute::FloatArray(ordered_floats) => todo!(),
            pcf::Attribute::BoolArray(bool8s) => todo!(),
            pcf::Attribute::StringArray(cstrings) => todo!(),
            pcf::Attribute::BinaryArray(items) => todo!(),
            pcf::Attribute::ColorArray(colors) => todo!(),
            pcf::Attribute::Vector2Array(vector2s) => todo!(),
            pcf::Attribute::Vector3Array(vector3s) => todo!(),
            pcf::Attribute::Vector4Array(vector4s) => todo!(),
            pcf::Attribute::MatrixArray(items) => todo!(),
        };

        s.st_post(d);
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        process::exit(1);
    }

    let path = &args[1];
    let mut file = File::open_buffered(path).unwrap();
    let pcf = Pcf::decode(&mut file).unwrap();

    let function_defaults: HashMap<FunctionName, HashMap<NameIndexString, Attribute>> = pcf
        .elements()
        .iter()
        .filter_map(|el| {
            if el.type_idx == pcf.strings().particle_operator_type_idx {
                let function_name = el.attributes.get(&pcf.strings().function_name_name_idx)?;
                let non_function_name_attributes = el
                    .attributes
                    .iter()
                    .filter(|(name_idx, _)| **name_idx != pcf.strings().function_name_name_idx);
                Some((function_name, non_function_name_attributes.collect()))
            } else {
                None
            }
        })
        .collect();

    let result = SerJson::serialize_json(&function_defaults);
}
