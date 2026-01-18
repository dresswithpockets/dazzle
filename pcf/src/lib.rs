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
#![feature(cstr_display)]
#![feature(ascii_char)]
#![feature(string_into_chars)]

pub mod attribute;
pub mod index;
pub mod new;
mod strings;

pub use new::{Pcf, ParticleSystem, Root};
pub use attribute::{Attribute};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DecodeError {
    #[error(transparent)]
    Dmx(#[from] dmx::dmx::Error),

    #[error(transparent)]
    Pcf(#[from] new::Error),
}

pub fn decode(buf: &mut impl std::io::BufRead) -> Result<Pcf, DecodeError> {
    let dmx = dmx::decode(buf)?;
    Ok(Pcf::try_from(dmx)?)
}
