use std::{
    ffi::{CStr, CString},
    fmt::Display,
    str::FromStr,
};

use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use itertools::Itertools;
use ordermap::OrderMap;
use thiserror::Error;

use crate::{
    Signature, SymbolIdx, Symbols,
    attribute::{Attribute, AttributeReader, AttributeWriter},
};

#[derive(Debug, Clone, Default)]
/// A Valve Particles Config File. These are DMX files with certain constraints:
///
/// - there is always a root element with an Element Array referencing every partical system
/// - all elements are either a particle system, or a child element of a particle system's definition tree
pub struct Dmx {
    pub version: Version,
    pub strings: Symbols,
    pub elements: Vec<Element>,
}

#[derive(Debug, Clone)]
pub struct Element {
    pub type_idx: SymbolIdx,
    pub name: CString,
    pub signature: Signature,
    pub attributes: OrderMap<SymbolIdx, Attribute>,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy, Default)]
pub enum Version {
    Binary2Dmx1,
    #[default]
    Binary2Pcf1,
    Binary3Pcf1,
}

impl Version {
    fn as_cstr_with_nul_terminator(&self) -> &'static CStr {
        match self {
            Version::Binary2Dmx1 => c"<!-- dmx encoding binary 2 format dmx 1 -->\x0A",
            Version::Binary2Pcf1 => c"<!-- dmx encoding binary 2 format pcf 1 -->\x0A",
            Version::Binary3Pcf1 => c"<!-- dmx encoding binary 3 format pcf 1 -->\x0A",
        }
    }
}

impl From<Version> for &CStr {
    fn from(value: Version) -> Self {
        value.as_cstr_with_nul_terminator()
    }
}

impl Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(match self {
            Version::Binary2Dmx1 => "Binary2Dmx1",
            Version::Binary2Pcf1 => "Binary2Pcf1",
            Version::Binary3Pcf1 => "Binary3Pcf1",
        })
    }
}

#[derive(Error, Debug)]
pub enum ParseVersionError {
    #[error("the version string was invalid: '{0}'")]
    Invalid(String),
}

impl FromStr for Version {
    type Err = ParseVersionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        const BINARY2_DMX1: &str = "<!-- dmx encoding binary 2 format dmx 1 -->\x0A";
        const BINARY2_PCF1: &str = "<!-- dmx encoding binary 2 format pcf 1 -->\x0A";
        const BINARY3_PCF1: &str = "<!-- dmx encoding binary 3 format pcf 1 -->\x0A";
        if s.eq(BINARY2_DMX1) {
            Ok(Self::Binary2Dmx1)
        } else if s.eq(BINARY2_PCF1) {
            Ok(Self::Binary2Pcf1)
        } else if s.eq(BINARY3_PCF1) {
            Ok(Self::Binary3Pcf1)
        } else {
            Err(Self::Err::Invalid(s.to_string()))
        }
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    CStringFromVec(#[from] std::ffi::FromVecWithNulError),

    #[error(transparent)]
    CStrFromBytes(#[from] std::ffi::FromBytesWithNulError),

    #[error(transparent)]
    ParseVersionError(#[from] ParseVersionError),

    #[error(transparent)]
    AttributeReadError(#[from] crate::attribute::ReadError),
}

impl Dmx {
    pub fn decode(buf: &mut impl std::io::BufRead) -> Result<Dmx, Error> {
        let version = Self::read_magic_version(buf)?;
        let strings = Self::read_strings(buf)?;
        let elements = Self::read_elements(buf)?;

        Ok(Self {
            version,
            strings,
            elements,
        })
    }

    fn read_terminated_string(file: &mut impl std::io::BufRead) -> Result<CString, Error> {
        let mut header_buf = Vec::new();
        file.read_until(0, &mut header_buf)?;

        Ok(CString::from_vec_with_nul(header_buf)?)
    }

    fn read_magic_version(file: &mut impl std::io::BufRead) -> Result<Version, Error> {
        let mut header_buf = Vec::new();
        file.read_until(0, &mut header_buf)?;

        let version = CStr::from_bytes_with_nul(&header_buf)?
            .to_string_lossy()
            .parse::<Version>()?;

        Ok(version)
    }

    fn read_strings(file: &mut impl std::io::BufRead) -> Result<Symbols, Error> {
        let symbol_count = file.read_u16::<LittleEndian>()? as usize;

        let mut symbols = Symbols::with_capacity(symbol_count);
        for _ in 0..symbol_count {
            symbols.insert(Self::read_terminated_string(file)?);
        }

        Ok(symbols)
    }

    fn read_elements(file: &mut impl std::io::BufRead) -> Result<Vec<Element>, Error> {
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
                attributes: OrderMap::new(),
            });
        }

        // we add one to element_count since AttributeReader will read root's attributes + elements' attributes
        let attributes: Result<Vec<_>, _> = AttributeReader::try_from(file, element_count)?.into_iter().collect();
        let attributes = attributes?.into_iter().chunk_by(|el| el.0);

        for (element_idx, group) in attributes.into_iter() {
            // the element_idx returned by the attribute reader includes root at 0, but we took root out of the list
            // so we need to subtract 1 from the idx
            let element = elements.get_mut(element_idx).expect("this should never happen");
            element.attributes = group.map(|el| (el.1, el.2)).collect();
        }

        Ok(elements)
    }
}

impl Dmx {
    pub fn encode(&self, file: &mut impl std::io::Write) -> anyhow::Result<()> {
        self.write_magic_version(file)?;
        self.write_strings(file)?;
        self.write_elements(file)?;
        self.write_element_attributes(file)?;

        Ok(())
    }

    fn write_magic_version(&self, file: &mut impl std::io::Write) -> anyhow::Result<()> {
        let version: &CStr = self.version.into();
        file.write_all(version.to_bytes_with_nul())?;

        Ok(())
    }

    fn write_strings(&self, file: &mut impl std::io::Write) -> anyhow::Result<()> {
        file.write_u16::<LittleEndian>(self.strings.len() as u16)?;

        for string in &self.strings {
            file.write_all(string.to_bytes_with_nul())?;
        }

        Ok(())
    }

    fn write_elements(&self, file: &mut impl std::io::Write) -> anyhow::Result<()> {
        file.write_u32::<LittleEndian>(self.elements.len() as u32)?;
        for element in &self.elements {
            file.write_u16::<LittleEndian>(element.type_idx)?;
            file.write_all(element.name.to_bytes_with_nul())?;
            file.write_all(&element.signature)?;
        }

        Ok(())
    }

    fn write_element_attributes(&self, file: &mut impl std::io::Write) -> anyhow::Result<()> {
        AttributeWriter::from(file).write_attributes(&self.elements)?;

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

        let pcf = Dmx::decode(&mut reader).unwrap();
        assert_eq!(pcf.version, Version::Binary2Pcf1);
        assert_eq!(pcf.strings.len(), 231);

        // spot checking a few random strings to ensure they're correct
        assert_eq!(pcf.strings[79], c"rotation_offset_max");
        assert_eq!(pcf.strings[160], c"end time max");
        assert_eq!(pcf.strings[220], c"warp max");

        assert_eq!(pcf.elements.len(), 2028);

        let buf = BytesMut::with_capacity(TEST_PCF.len());
        let mut writer = buf.writer();
        pcf.write_magic_version(&mut writer).expect("writing failed");

        let bytes = writer.get_ref();
        assert_eq!(bytes.len(), 45);

        pcf.write_strings(&mut writer).expect("writing failed");

        let bytes = writer.get_ref();
        assert_eq!(bytes.len(), 4627);

        pcf.write_elements(&mut writer).expect("writing failed");

        let bytes = writer.get_ref();
        assert_eq!(bytes.len(), 82526);

        pcf.write_element_attributes(&mut writer).expect("writing failed");
        let bytes = writer.get_ref();
        assert_eq!(TEST_PCF.len(), bytes.len());
        assert_eq!(
            TEST_PCF,
            &writer.get_ref()[..],
            "expected decoded buf and encoded buf to be identical."
        );
    }
}
