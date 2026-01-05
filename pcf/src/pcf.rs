use std::{collections::{HashMap, HashSet}, ffi::{CStr, CString}, fmt::Display, hash::Hash, marker::PhantomData, vec};

use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use either::Either::{self, Left, Right};
use ordermap::{OrderMap, OrderSet};
use thiserror::Error;

use crate::{attribute::{self, Attribute, AttributeReader, AttributeWriter, NameIndex, ReadError}};

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Version {
    Binary2Dmx1,
    Binary2Pcf1,
    Binary3Pcf1,
}

impl Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
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

pub type TypeIndex = u16;

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct Element {
    pub type_idx: TypeIndex,
    pub name: CString,
    pub signature: [u8; 16],
    pub attributes: OrderMap<NameIndex, Attribute>,
}

#[derive(Debug, Clone)]
/// A Valve Particles Config File.
pub struct Pcf {
    pub version: Version,
    pub strings: Vec<CString>,
    pub elements: Vec<Element>,
    elements_by_name: HashMap<CString, u32>,
}

#[derive(Debug, Error)]
pub enum MergeError {
    #[error("can't merge PCF with version {0} into PCF with version {1}")]
    VersionMismatch(Version, Version),

    #[error("our strings list is missing 'particleSystemDefinitions'. Adding this string may require elements to be fixed up.")]
    SelfIsMissingRootDefinitionsString,

    #[error("our strings list is missing 'DmeParticleSystemDefinition'. Adding this string may require elements to be fixed up.")]
    SelfIsMissingSystemDefinitionString,

    #[error("other's strings list is missing 'particleSystemDefinitions'. Adding this string may require elements to be fixed up.")]
    OtherIsMissingRootDefinitionsString,

    #[error("other's strings list is missing 'DmeParticleSystemDefinition'. Adding this string may require elements to be fixed up.")]
    OtherIsMissingSystemDefinitionString,
}

impl Pcf {
    pub fn new(version: Version) -> Pcf {
        Pcf {
            version,
            strings: Vec::new(),
            elements: Vec::new(),
            elements_by_name: HashMap::new(),
        }
    }

    pub fn get_element_type(&self, element: &Element) -> Option<&CString> {
        self.strings.get(element.type_idx as usize)
    }

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
                            if *value == u32::MAX {
                                continue;
                            }

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

    pub fn merge(self, other: Pcf) -> Result<Self, MergeError> {
        if other.version != self.version {
            return Err(MergeError::VersionMismatch(other.version, self.version));
        }

        let mut strings: OrderMap<CString, ()> = self.strings.into_iter().map(|value| (value, ())).collect();
        let other_strings: OrderSet<CString> = other.strings.into_iter().collect();

        // The PCF format is based on DMX, so there are no guarantees that the strings list will be identical between 
        // two PCF files. Its possible to have new strings, or strings that have changed position. So, we create a
        // map here to convert from incoming string index to merged string index.
        //
        // We also add any new strings from `other` into `self.strings` here.
        let mut other_to_self_string_idx: HashMap<TypeIndex, TypeIndex> = HashMap::new();
        for (other_idx, other_string) in other_strings.into_iter().enumerate() {
            let mapped_idx = strings.entry(other_string).insert_entry(()).index();
            other_to_self_string_idx.insert(other_idx as TypeIndex, mapped_idx as TypeIndex);
        }

        let Some(root_definitions_name_idx) = strings.iter().position(|(el, _)| el == c"particleSystemDefinitions") else {
            return Err(MergeError::SelfIsMissingRootDefinitionsString);
        };

        let Some(system_name_idx) = strings.iter().position(|(el, _)| el == c"DmeParticleSystemDefinition") else {
            return Err(MergeError::SelfIsMissingSystemDefinitionString);
        };

        let root_definitions_name_idx = root_definitions_name_idx as NameIndex;
        let system_name_idx = system_name_idx as NameIndex;
        
        let mut new_elements = Vec::with_capacity(self.elements.len() + other.elements.len());
        let mut new_system_indices = Vec::new();
        for other_element in other.elements.into_iter().skip(1) {
            // string indices may have changed, so we're remapping to the new index
            let new_type_idx = *other_to_self_string_idx.get(&other_element.type_idx).expect("the element's type_idx should always match a value in the Pcf's string list");

            // when we merge in another PCF's elements, we are basically just appending all new elements to our elements.
            // the incoming PCF's elements references will be incorrect - because the indices have been offset by the 
            // elements already in our list. So, we have to fixup every name index and element reference for each 
            // attribute in each incoming element.
            let new_attributes = other_element.attributes.into_iter()
                .map(|(name_idx, attribute)| {
                    let name_idx = *other_to_self_string_idx.get(&name_idx).expect("the attribute's name_idx should always match a value in the Pcf's string list");

                    let element_offset = self.elements.len() as u32;
                    let attribute = match attribute {
                        Attribute::Element(value) if value != u32::MAX => Attribute::Element(value + element_offset),
                        Attribute::ElementArray(mut items) => {
                            for item in items.iter_mut() {
                                if *item == u32::MAX {
                                    continue;
                                }

                                *item += element_offset;
                            }

                            Attribute::ElementArray(items)
                        },
                        attribute => attribute,
                    };

                    (name_idx, attribute)
                });

            let new_element = Element {
                type_idx: new_type_idx,
                attributes: new_attributes.collect(),
                ..other_element
            };

            new_elements.push(new_element);

            // when adding a new DmeParticleSystemDefinition element, we need to make sure the root node's
            // particleSystemDefinitions list is updated with the new element references
            if new_type_idx == system_name_idx {
                new_system_indices.push(new_elements.len() as u32 - 1)
            }
        }

        // making sure that our merged PCF contains references for all new particle system definitions
        let root_element = new_elements.get_mut(0).expect("there should always be a root element");
        root_element.attributes.entry(root_definitions_name_idx).and_modify(|root_definitions| {
            let Attribute::ElementArray(root_definitions) = root_definitions else {
                panic!("the root particle systems definitions attribute must always be an ElementArray");
            };

            *root_definitions = [root_definitions.as_ref(), new_system_indices.as_slice()].concat().into_boxed_slice();
        });

        Ok(Pcf {
            version: self.version,
            strings: strings.into_iter().map(|(string, _)| string).collect(),
            elements_by_name: new_elements.iter().enumerate().map(|(idx, el)| (el.name.clone(), idx as u32)).collect(),
            elements: new_elements,
        })
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
                attributes: OrderMap::new(),
            });
        }

        for element in &mut elements {
            let reader = AttributeReader::try_from(file, element_count)?.into_iter();
            let attributes: Result<OrderMap<NameIndex, Attribute>, ReadError> = reader.collect();
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