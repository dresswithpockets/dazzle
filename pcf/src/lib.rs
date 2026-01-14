//! Encode and decode Valve's binary particle config file format.
//!
//! # Example
//!
//! Decode & modify a pcf file using a buffered reader:
//! ```
//!     use std::fs::{File, OpenOptions};
//!     use std::io::{BufReader};
//!
//!     let mut file = File::open("particles.pcf").unwrap();
//!     let mut file = BufReader::new(file);
//!     let mut pcf = pcf::decode(&mut file).unwrap();
//!     println!("particles.pcf has {} particle systems.", pcf.elements().len());
//!     // modify pcf elements or attributes...
//!     // ...
//!
//!     // Encode a pcf back into a file
//!     let mut file = OpenOptions::new().create(true).write(true).open("new_particles.pcf").unwrap();
//!     pcf.encode(&mut file).unwrap();
//! ```

#![feature(buf_read_has_data_left)]
#![feature(read_array)]
#![feature(trim_prefix_suffix)]
#![feature(associated_type_defaults)]
#![feature(error_generic_member_access)]

pub mod attribute;
pub mod index;
pub mod new;
pub mod pcf;

pub use attribute::{Attribute, NameIndex};
pub use pcf::{Element, ElementsExt, Pcf, Root, TypeIndex};

pub fn decode(buf: &mut impl std::io::BufRead) -> Result<Pcf, pcf::Error> {
    Pcf::decode(buf)
}
