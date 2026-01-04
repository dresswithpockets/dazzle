use std::{collections::{HashMap, HashSet}, ffi::{CStr, CString}, hash::Hash, marker::PhantomData, vec};

use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use either::Either::{self, Left, Right};
use thiserror::Error;

use crate::attribute::{self, Attribute, AttributeReader, AttributeWriter, NameIndex, ReadError};

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
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
    fn as_cstr_with_nul_terminator(&self) -> &'static CStr {
        match self {
            Version::Binary2Dmx1 => c"<!-- dmx encoding binary 2 format dmx 1 -->\x0A",
            Version::Binary2Pcf1 => c"<!-- dmx encoding binary 2 format pcf 1 -->\x0A",
            Version::Binary3Pcf1 => c"<!-- dmx encoding binary 3 format pcf 1 -->\x0A",
        }
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct Element {
    pub type_idx: u16,
    pub name: CString,
    pub signature: [u8; 16],
    pub attributes: Vec<(NameIndex, Attribute)>,
}

#[derive(Debug)]
/// A Valve Particles Config File.
pub struct Pcf {
    pub version: Version,
    pub strings: Vec<CString>,
    pub elements: Vec<Element>,
    elements_by_name: HashMap<CString, u32>,
}

impl Pcf {
    pub fn get_element(&self, name: &CString) -> Option<&Element> {
        match self.elements_by_name.get(name) {
            Some(idx) => self.elements.get(*idx as usize),
            None => None,
        }
    }

    pub fn get_element_index(&self, name: &CString) -> Option<u32> {
        self.elements_by_name.get(name).cloned()
    }

    pub fn get_dependent_indices(&self, name: &CString) -> Option<HashSet<u32>> {
        fn visit(visited: &mut HashSet<u32>, elements: &Vec<Element>, idx: u32) {
            // NB insert returns false when insertion fails
            if !visited.insert(idx) {
                return
            }

            let Some(element) = elements.get(idx as usize) else {
                return;
            };

            // Element and Element Array attributes contain indices for other elements
            for (_, attribute) in &element.attributes {
                match attribute {
                    Attribute::Element(value) if *value != u32::MAX => {
                        visit(visited, elements, *value);
                    }
                    Attribute::ElementArray(values) => {
                        for value in values {
                            visit(visited, elements, *value);
                        }
                    }
                    _ => continue,
                }
            }
        }

        let idx = self.get_element_index(name)?;

        let mut visited = HashSet::new();
        visit(&mut visited, &self.elements, idx);

        Some(visited)
    }

    pub fn builder() -> PcfBuilder<NoVersion, NoStrings, NoElements> {
        PcfBuilder { version: NoVersion, strings: Vec::new(), elements: Vec::new(), elements_by_name: HashMap::new(), _phantom_elements: PhantomData, _phantom_strings: PhantomData }
    }

    pub fn builder_from(pcf: &Pcf) -> PcfBuilder<Version, IncompleteStrings, IncompleteElements> {
        PcfBuilder { version: pcf.version, strings: pcf.strings.clone(), elements: pcf.elements.clone(), elements_by_name: pcf.elements_by_name.clone(), _phantom_elements: PhantomData, _phantom_strings: PhantomData }
    }
}

#[derive(Default, Debug, PartialEq)]
pub struct NoVersion;
#[derive(Default, Debug, PartialEq)]
pub struct NoStrings;
#[derive(Default, Debug, PartialEq)]
pub struct IncompleteStrings;
#[derive(Default, Debug, PartialEq)]
pub struct Strings;
#[derive(Default, Debug, PartialEq)]
pub struct NoElements;
#[derive(Default, Debug, PartialEq)]
pub struct IncompleteElements;
#[derive(Default, Debug, PartialEq)]
pub struct Elements;

#[derive(Default, Debug)]
pub struct PcfBuilder<A, B, C> {
    version: A,
    strings: Vec<CString>,
    elements: Vec<Element>,
    elements_by_name: HashMap<CString, u32>,

    _phantom_strings: PhantomData<B>,
    _phantom_elements: PhantomData<C>,
}

impl PcfBuilder<Version, Strings, Elements> {
    pub fn build(self) -> Pcf {
        Pcf { version: self.version, strings: self.strings, elements: self.elements, elements_by_name: self.elements_by_name }
    }
}

impl<A, C> PcfBuilder<A, IncompleteStrings, C> {
    pub fn complete_strings(self) -> Result<PcfBuilder<A, Strings, C>, PcfBuilder<A, NoStrings, C>> {
        if self.strings.is_empty() {
            Err(PcfBuilder { version: self.version, strings: self.strings, elements: self.elements, elements_by_name: self.elements_by_name, _phantom_strings: PhantomData, _phantom_elements: PhantomData })
        } else {
            Ok(PcfBuilder { version: self.version, strings: self.strings, elements: self.elements, elements_by_name: self.elements_by_name, _phantom_strings: PhantomData, _phantom_elements: PhantomData })
        }
    }
}

impl <A, B> PcfBuilder<A, B, IncompleteElements> {
    pub fn complete_elements(self) -> Either<PcfBuilder<A, B, Elements>, PcfBuilder<A, B, NoElements>> {
        if self.elements.is_empty() {
            Right(PcfBuilder { version: self.version, strings: self.strings, elements: self.elements, elements_by_name: self.elements_by_name, _phantom_strings: PhantomData, _phantom_elements: PhantomData })
        } else {
            Left(PcfBuilder { version: self.version, strings: self.strings, elements: self.elements, elements_by_name: self.elements_by_name, _phantom_strings: PhantomData, _phantom_elements: PhantomData })
        }
    }
}

impl<B, C> PcfBuilder<NoVersion, B, C> {
    pub fn version(self, version: Version) -> PcfBuilder<Version, B, C> {
        PcfBuilder { version, strings: self.strings, elements: self.elements, elements_by_name: self.elements_by_name, _phantom_elements: PhantomData, _phantom_strings: PhantomData }
    }
}

impl<A, C> PcfBuilder<A, NoStrings, C> {
    pub fn strings(self, strings: Vec<CString>) -> PcfBuilder<A, Strings, C> {
        PcfBuilder { version: self.version, strings, elements: self.elements, elements_by_name: self.elements_by_name, _phantom_elements: PhantomData, _phantom_strings: PhantomData }
    }

    pub fn string(self, string: CString) -> PcfBuilder<A, Strings, C> {
        PcfBuilder { version: self.version, strings: vec![string], elements: self.elements, elements_by_name: self.elements_by_name, _phantom_elements: PhantomData, _phantom_strings: PhantomData }
    }
}

impl<A, C> PcfBuilder<A, Strings, C> {
    pub fn string(self, string: CString) -> PcfBuilder<A, Strings, C> {
        let mut strings = self.strings;
        strings.push(string);

        PcfBuilder { version: self.version, strings, elements: self.elements, elements_by_name: self.elements_by_name, _phantom_elements: PhantomData, _phantom_strings: PhantomData }
    }
}

impl<A, B> PcfBuilder<A, B, NoElements> {
    pub fn elements(self, elements: Vec<Element>) -> PcfBuilder<A, B, Elements> {
        let mut elements_by_name = self.elements_by_name;
        for (idx, element) in elements.iter().enumerate() {
            elements_by_name.insert(element.name.clone(), idx as u32);
        }

        PcfBuilder { version: self.version, strings: self.strings, elements, elements_by_name, _phantom_elements: PhantomData, _phantom_strings: PhantomData }
    }

    pub fn element(self, element: Element) -> PcfBuilder<A, B, Elements> {
        let mut elements_by_name = self.elements_by_name;
        elements_by_name.insert(element.name.clone(), 0);

        PcfBuilder { version: self.version, strings: self.strings, elements: vec![element], elements_by_name, _phantom_elements: PhantomData, _phantom_strings: PhantomData }
    }
}

impl<A, B> PcfBuilder<A, B, Elements> {
    pub fn element(self, element: Element) -> PcfBuilder<A, B, Elements> {
        let element_name = element.name.clone();

        let mut elements = self.elements;
        elements.push(element);

        let mut elements_by_name = self.elements_by_name;
        elements_by_name.insert(element_name, (elements.len() - 1) as u32);

        PcfBuilder { version: self.version, strings: self.strings, elements, elements_by_name, _phantom_elements: PhantomData, _phantom_strings: PhantomData }
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
    ReadError(#[from] attribute::ReadError)
}

// reading functions
impl Pcf {
    pub fn decode(buf: &mut impl std::io::BufRead) -> Result<Pcf, Error> {
        let version = Self::read_magic_version(buf)?;
        let strings = Self::read_strings(buf)?;
        let elements = Self::read_elements(buf)?; 
        
        let mut elements_by_name = HashMap::new();
        for (idx, element) in elements.iter().enumerate() {
            elements_by_name.insert(element.name.clone(), idx as u32);
        }

        Ok(Self { version, strings, elements, elements_by_name })
    }

    fn read_terminated_string(file: &mut impl std::io::BufRead) -> Result<CString, Error> {
        let mut header_buf = Vec::new();
        file.read_until(0, &mut header_buf)?;

        Ok(CString::from_vec_with_nul(header_buf)?)
    }

    fn read_magic_version(file: &mut impl std::io::BufRead) -> Result<Version, Error> {
        let mut header_buf = Vec::new();
        file.read_until(0, &mut header_buf)?;

        let version = CStr::from_bytes_with_nul(&header_buf)?;
        let version = Version::try_from(version)?;
        Ok(version)
    }

    fn read_strings(file: &mut impl std::io::BufRead) -> Result<Vec<CString>, Error> {
        let string_count = file.read_u16::<LittleEndian>()? as usize;
        let mut strings = Vec::with_capacity(string_count);
        for _ in 0..string_count {
            strings.push(Self::read_terminated_string(file)?)
        }

        Ok(strings)
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
                attributes: Vec::new(),
            });
        }

        for element in &mut elements {
            let reader = AttributeReader::try_from(file, element_count)?.into_iter();
            let attributes: Result<Vec<(NameIndex, Attribute)>, ReadError> = reader.collect();
            element.attributes = attributes?;
        }

        Ok(elements)
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
        let version = version.as_cstr_with_nul_terminator().to_bytes_with_nul();
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

        AttributeWriter::from(file).write_attributes(elements)?;

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
        assert_eq!(
            TEST_PCF,
            &writer.get_ref()[..],
            "expected decoded buf and encoded buf to be identical."
        );
    }
}