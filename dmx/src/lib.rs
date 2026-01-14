#![feature(buf_read_has_data_left)]
#![feature(read_array)]
#![feature(trim_prefix_suffix)]
#![feature(associated_type_defaults)]
#![feature(error_generic_member_access)]

pub mod attribute;
pub mod dmx;
pub mod index;

use std::ffi::CString;

use ordermap::OrderSet;

pub type Signature = [u8; 16];
pub type SymbolIdx = u16;
pub type Symbols = OrderSet<CString>;
pub use index::ElementIdx;
