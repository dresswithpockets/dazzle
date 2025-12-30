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

use std::ffi::{CStr, CString};

use anyhow::anyhow;
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use derive_more::{From, Into};
use thiserror::Error;

#[derive(Debug)]
/// A Valve Particles Config File.
pub struct Pcf {
    pub version: Version,
    pub strings: Vec<CString>,
    pub elements: Vec<Element>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Version {
    Binary2Dmx1,
    Binary2Pcf1,
    Binary3Pcf1,
}

#[derive(Error, Debug)]
pub enum ParseVersionError {
    #[error("the version string was invalid: '{0}'")]
    Invalid(String),
}

impl TryFrom<&CStr> for Version {
    type Error = ParseVersionError;

    fn try_from(s: &CStr) -> Result<Self, Self::Error> {
        const BINARY2_DMX1: &CStr = c"<!-- dmx encoding binary 2 format dmx 1 -->\x0A";
        const BINARY2_PCF1: &CStr = c"<!-- dmx encoding binary 2 format pcf 1 -->\x0A";
        const BINARY3_PCF1: &CStr = c"<!-- dmx encoding binary 3 format pcf 1 -->\x0A";
        if s.eq(BINARY2_DMX1) {
            Ok(Self::Binary2Dmx1)
        } else if s.eq(BINARY2_PCF1) {
            Ok(Self::Binary2Pcf1)
        } else if s.eq(BINARY3_PCF1) {
            Ok(Self::Binary3Pcf1)
        } else {
            Err(Self::Error::Invalid(s.to_str().unwrap_or("").to_string()))
        }
    }
}

impl Version {
    fn to_cstr_with_nul_terminator(&self) -> &'static CStr {
        match self {
            Version::Binary2Dmx1 => c"<!-- dmx encoding binary 2 format dmx 1 -->\x0A",
            Version::Binary2Pcf1 => c"<!-- dmx encoding binary 2 format pcf 1 -->\x0A",
            Version::Binary3Pcf1 => c"<!-- dmx encoding binary 3 format pcf 1 -->\x0A",
        }
    }
}

#[derive(Debug)]
pub struct Element {
    pub type_idx: u16,
    pub name: CString,
    pub signature: [u8; 16],
    pub attributes: Vec<(NameIndex, Attribute)>,
}

type NameIndex = u16;

#[derive(Debug, From)]
pub enum Attribute {
    Element(u32),
    Integer(i32),
    Float(f32),
    Bool(Bool8),
    String(CString),
    Binary(Vec<u8>),
    Color(Color),
    Vector2(Vector2),
    Vector3(Vector3),
    Vector4(Vector4),
    Matrix(Matrix),
    Array(u8, Vec<Attribute>),
}

impl Attribute {
    fn as_type(&self) -> u8 {
        match self {
            Attribute::Element(_) => 1,
            Attribute::Integer(_) => 2,
            Attribute::Float(_) => 3,
            Attribute::Bool(_) => 4,
            Attribute::String(_) => 5,
            Attribute::Binary(_) => 6,
            Attribute::Color(_) => 8,
            Attribute::Vector2(_) => 9,
            Attribute::Vector3(_) => 10,
            Attribute::Vector4(_) => 11,
            Attribute::Matrix(_) => 14,
            Attribute::Array(element_type, _) => 14 + element_type,
        }
    }
}

#[derive(Debug, From, Into)]
/// An 8-bit boolean value. 0 is false, all other values are truthy.
pub struct Bool8(u8);

impl From<Bool8> for bool {
    fn from(value: Bool8) -> Self {
        value.0 != 0
    }
}

impl From<bool> for Bool8 {
    fn from(value: bool) -> Self {
        if value {
            Self(1)
        } else {
            Self(0)
        }
    }
}

#[derive(Debug)]
pub struct Color(pub u8, pub u8, pub u8, pub u8);
#[derive(Debug)]
pub struct Vector2(pub f32, pub f32);
#[derive(Debug)]
pub struct Vector3(pub f32, pub f32, pub f32);
#[derive(Debug)]
pub struct Vector4(pub f32, pub f32, pub f32, pub f32);
#[derive(Debug)]
pub struct Matrix(pub Vector4, pub Vector4, pub Vector4, pub Vector4);

// reading functions
impl Pcf {
    pub fn decode(buf: &mut impl std::io::BufRead) -> anyhow::Result<Pcf> {
        Ok(Self {
            version: Self::read_magic_version(buf)?,
            strings: Self::read_strings(buf)?,
            elements: Self::read_elements(buf)?,
        })
    }

    fn read_terminated_string(file: &mut impl std::io::BufRead) -> anyhow::Result<CString> {
        let mut header_buf = Vec::new();
        file.read_until(0, &mut header_buf)?;

        Ok(CString::from_vec_with_nul(header_buf)?)
    }

    fn read_magic_version(file: &mut impl std::io::BufRead) -> anyhow::Result<Version> {
        let mut header_buf = Vec::new();
        file.read_until(0, &mut header_buf)?;

        let version = CStr::from_bytes_with_nul(&header_buf)?;
        let version = Version::try_from(version)?;
        Ok(version)
    }

    fn read_strings(file: &mut impl std::io::BufRead) -> anyhow::Result<Vec<CString>> {
        let string_count = file.read_u16::<LittleEndian>()? as usize;
        let mut strings = Vec::with_capacity(string_count);
        for _ in 0..string_count {
            strings.push(Self::read_terminated_string(file)?)
        }

        Ok(strings)
    }

    fn read_elements(file: &mut impl std::io::BufRead) -> anyhow::Result<Vec<Element>> {
        let element_count = file.read_u32::<LittleEndian>()? as usize;
        let mut elements = Vec::with_capacity(element_count);
        for _idx in 0..element_count {
            let type_idx = file.read_u16::<LittleEndian>()?;
            let name = Self::read_terminated_string(file)?;
            let signature = file.read_array::<16>()?;
            
            elements.push(Element {
                type_idx,
                name,
                signature,
                attributes: Vec::new(),
            });
        }

        for element in &mut elements {
            let attribute_count = file.read_u32::<LittleEndian>()? as usize;

            for _ in 0..attribute_count {
                let name_idx = file.read_u16::<LittleEndian>()?;
                let type_idx = file.read_u8()?;
                let attribute = Self::read_attribute_data(file, type_idx)?;
                element.attributes.push((name_idx, attribute));
            }
        }

        Ok(elements)
    }

    fn read_attribute_data(file: &mut impl std::io::BufRead, type_idx: u8) -> anyhow::Result<Attribute> {
        fn read_vector4(file: &mut impl std::io::BufRead) -> anyhow::Result<Vector4> {
            Ok(Vector4(
                file.read_f32::<LittleEndian>()?,
                file.read_f32::<LittleEndian>()?,
                file.read_f32::<LittleEndian>()?,
                file.read_f32::<LittleEndian>()?,
            ))
        }

        match type_idx {
            1 => Ok(file.read_u32::<LittleEndian>()?.into()),
            2 => Ok(file.read_i32::<LittleEndian>()?.into()),
            3 => Ok(file.read_f32::<LittleEndian>()?.into()),
            4 => Ok(Bool8::from(file.read_u8()? != 0).into()),
            5 => Ok(Self::read_terminated_string(file)?.into()),
            6 => {
                let count = file.read_u32::<LittleEndian>()? as usize;
                let mut buf = vec![0; count];
                file.read_exact(&mut buf)?;
                Ok(buf.into())
            }
            8 => Ok(Color(file.read_u8()?, file.read_u8()?, file.read_u8()?, file.read_u8()?).into()),
            9 => Ok(Vector2(file.read_f32::<LittleEndian>()?, file.read_f32::<LittleEndian>()?).into()),
            10 => Ok(Vector3(
                file.read_f32::<LittleEndian>()?,
                file.read_f32::<LittleEndian>()?,
                file.read_f32::<LittleEndian>()?,
            )
            .into()),
            11 => Ok(read_vector4(file)?.into()),
            14 => Ok(Matrix(
                read_vector4(file)?,
                read_vector4(file)?,
                read_vector4(file)?,
                read_vector4(file)?,
            )
            .into()),
            15..=20 | 21..=24 | 27 => {
                let count = file.read_u32::<LittleEndian>()? as usize;
                let type_idx = type_idx - 14;
                let mut buf = Vec::with_capacity(count);
                for _idx in 0..count {
                    buf.push(Self::read_attribute_data(file, type_idx)?)
                }
                Ok((type_idx, buf).into())
            }
            _ => Err(anyhow!("unsupported attribute type: {type_idx}")),
        }
    }
}

// writing functions
impl Pcf {
    pub fn encode(&self, file: &mut impl std::io::Write) -> anyhow::Result<()> {
        Self::write_magic_version(&self.version, file)?;
        Self::write_strings(&self.strings, file)?;
        Self::write_elements(&self.elements, file)?;

        Ok(())
    }

    fn write_magic_version(version: &Version, file: &mut impl std::io::Write) -> anyhow::Result<()> {
        let version = version.to_cstr_with_nul_terminator().to_bytes_with_nul();
        file.write_all(version)?;

        Ok(())
    }

    fn write_strings(strings: &Vec<CString>, file: &mut impl std::io::Write) -> anyhow::Result<()> {
        file.write_u16::<LittleEndian>(strings.len() as u16)?;

        for string in strings {
            file.write_all(string.to_bytes_with_nul())?;
        }

        Ok(())
    }

    fn write_elements(elements: &Vec<Element>, file: &mut impl std::io::Write) -> anyhow::Result<()> {
        file.write_u32::<LittleEndian>(elements.len() as u32)?;
        for element in elements {
            file.write_u16::<LittleEndian>(element.type_idx)?;
            file.write_all(element.name.to_bytes_with_nul())?;
            file.write_all(&element.signature)?;
        }

        for element in elements {
            file.write_u32::<LittleEndian>(element.attributes.len() as u32)?;
            for (name_idx, attribute) in &element.attributes {
                file.write_u16::<LittleEndian>(*name_idx)?;
                file.write_u8(attribute.as_type())?;
                Self::write_attribute_data(attribute, file)?;
            }
        }

        Ok(())
    }

    fn write_attribute_data(attribute: &Attribute, file: &mut impl std::io::Write) -> anyhow::Result<()> {
        fn write_vector4(vector4: &Vector4, file: &mut impl std::io::Write) -> anyhow::Result<()> {
            file.write_f32::<LittleEndian>(vector4.0)?;
            file.write_f32::<LittleEndian>(vector4.1)?;
            file.write_f32::<LittleEndian>(vector4.2)?;
            file.write_f32::<LittleEndian>(vector4.3)?;
            Ok(())
        }

        match attribute {
            Attribute::Element(element) => file.write_u32::<LittleEndian>(*element)?,
            Attribute::Integer(integer) => file.write_i32::<LittleEndian>(*integer)?,
            Attribute::Float(float) => file.write_f32::<LittleEndian>(*float)?,
            Attribute::Bool(bool) => file.write_u8(bool.0)?,
            Attribute::String(cstring) => file.write_all(cstring.as_bytes_with_nul())?,
            Attribute::Binary(items) => {
                file.write_u32::<LittleEndian>(items.len() as u32)?;
                file.write_all(items.as_slice())?;
            },
            Attribute::Color(color) => {
                file.write_u8(color.0)?;
                file.write_u8(color.1)?;
                file.write_u8(color.2)?;
                file.write_u8(color.3)?;
            },
            Attribute::Vector2(vector2) => {
                file.write_f32::<LittleEndian>(vector2.0)?;
                file.write_f32::<LittleEndian>(vector2.1)?;
            },
            Attribute::Vector3(vector3) => {
                file.write_f32::<LittleEndian>(vector3.0)?;
                file.write_f32::<LittleEndian>(vector3.1)?;
                file.write_f32::<LittleEndian>(vector3.2)?;
            },
            Attribute::Vector4(vector4) => write_vector4(vector4, file)?,
            Attribute::Matrix(matrix) => {
                write_vector4(&matrix.0, file)?;
                write_vector4(&matrix.1, file)?;
                write_vector4(&matrix.2, file)?;
                write_vector4(&matrix.3, file)?;
            },
            Attribute::Array(_, attributes) => {
                file.write_u32::<LittleEndian>(attributes.len() as u32)?;
                for attribute in attributes {
                    Self::write_attribute_data(attribute, file)?;
                }
            },
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use bytes::{Buf, BufMut, Bytes, BytesMut};

    use super::*;

    const TEST_PCF: &[u8] = include_bytes!("rankup.pcf");

    #[test]
    fn encodes_and_decodes_valid_pcf() {
        let mut reader = Bytes::from(TEST_PCF).reader();
        
        let pcf = Pcf::decode(&mut reader).unwrap();
        assert_eq!(pcf.version, Version::Binary2Pcf1);
        assert_eq!(pcf.strings.len(), 231);

        // spot checking a few random strings to ensure they're correct
        assert_eq!(pcf.strings[79], CString::from(c"rotation_offset_max"));
        assert_eq!(pcf.strings[160], CString::from(c"end time max"));
        assert_eq!(pcf.strings[220], CString::from(c"warp max"));

        assert_eq!(pcf.elements.len(), 2028);

        let buf = BytesMut::with_capacity(TEST_PCF.len());
        let mut writer = buf.writer();
        pcf.encode(&mut writer).expect("writing failed");

        let bytes = writer.get_ref();
        assert_eq!(TEST_PCF.len(), bytes.len());
        assert_eq!(TEST_PCF, &writer.get_ref()[..], "expected decoded buf and encoded buf to be identical.");
    }
}
