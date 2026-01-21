//! Convert to a type-safe [`Pcf`] from Valve's Data Model eXchange format.
//! 
//! See [`dmx`].
//!
//! # Example
//!
//! Parse a type-safe [`Pcf`].
//! ```
//! #![feature(file_buffered)]
//! # use bytes::Buf;
//! # 
//! # const EXAMPLE_PCF: &[u8] = include_bytes!("default_values.pcf");
//! #
//! # fn main() -> anyhow::Result<()> {
//!     # let mut reader = EXAMPLE_PCF.reader();
//!     let pcf = pcf::decode(&mut reader)?;
//!     println!("particles.pcf has {} particle systems.", pcf.root().particle_systems().len());
//!     // read/modify PCF data...
//! #    Ok(())
//! # }
//! ```
//! 
//! See [`decode`] to decode a buffer into a [`Pcf`] directly.
//! 
//! See [`dmx::Dmx::encode`] to encode a [`dmx::Dmx`] into a buffer. You can convert a [`Pcf`] into [`dmx::Dmx`] freely
//! with [`Pcf::into`].

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

pub use attribute::Attribute;
pub use new::{ParticleSystem, Pcf, Root};
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
