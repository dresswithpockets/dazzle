//! Encode and decode Valve's binary particle config file format.
//!
//! # Example
//!
//! Decode & modify a pcf file using a buffered reader:
//! ```
//!     let file = File::open("particles.pcf")?;
//!     let mut reader = BufReader::new(file);
//!     let mut pcf = Pcf::decode(reader)?;
//!     println!("particles.pcf has {} particle systems.", pcf.elements.len());
//!     // modify pcf elements or attributes...
//!     // ...
//! ```
//!
//! Encode a pcf back into a file
//! ```
//!     let file = File::open("new_particles.pcf")?;
//!     let mut writer = BufWriter::new(file);
//!     pcf.encode(writer)?;
//! ```

#![feature(buf_read_has_data_left)]
#![feature(read_array)]
#![feature(trim_prefix_suffix)]
#![feature(associated_type_defaults)]

pub mod pcf;
pub mod attribute;

pub use pcf::{Pcf, Element, TypeIndex};
pub use attribute::{Attribute, NameIndex};

pub fn decode(buf: &mut impl std::io::BufRead) -> Result<Pcf, pcf::Error> {
    Pcf::decode(buf)
}
