use std::{
    collections::{HashMap, HashSet},
    ffi::{CStr, CString},
    fmt::Display,
    marker::PhantomData,
    vec,
};

use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use itertools::Itertools;
use ordermap::OrderMap;
use thiserror::Error;

use crate::attribute::{self, Attribute, AttributeReader, AttributeWriter, NameIndex};

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Version {
    Binary2Dmx1,
    Binary2Pcf1,
    Binary3Pcf1,
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

#[derive(Debug, Clone)]
pub struct Element {
    pub type_idx: TypeIndex,
    pub name: CString,
    pub signature: [u8; 16],
    pub attributes: OrderMap<NameIndex, Attribute>,
}

pub trait ElementsExt<'a>: Iterator<Item = &'a Element> + Sized {
    fn map_particle_system_indices(self, particle_system_type_idx: &NameIndex) -> impl Iterator<Item = u32> {
        self.enumerate().filter_map(|(element_idx, element)| {
            if element.type_idx == *particle_system_type_idx {
                Some(element_idx as u32)
            } else {
                None
            }
        })
    }
}

impl<'a, T: Iterator<Item = &'a Element> + Sized> ElementsExt<'a> for T {}

#[derive(Debug, Clone)]
pub struct Root {
    pub type_idx: TypeIndex,
    pub name: CString,
    pub signature: [u8; 16],
    pub definitions: Box<[u32]>,
}

#[derive(Debug, Clone)]
/// A Valve Particles Config File. These are DMX files with certain constraints:
///
/// - there is always a root element with an Element Array referencing every partical system
/// - all elements are either a particle system, or a child element of a particle system's definition tree
pub struct Pcf {
    pub version: Version,
    pub strings: Symbols,
    pub root: Root,
    pub elements: Vec<Element>,
    elements_by_name: HashMap<CString, u32>,
}

#[derive(Debug, Clone)]
pub struct Symbols {
    particle_system_definitions_name_idx: NameIndex,
    particle_system_definition_type_idx: NameIndex,
    material_name_idx: NameIndex,
    base: OrderMap<CString, ()>,
}

impl Symbols {
    pub fn iter(&self) -> ordermap::map::Iter<'_, std::ffi::CString, ()> {
        self.base.iter()
    }

    fn get_index(&self, idx: usize) -> Option<(&CString, &())> {
        self.base.get_index(idx)
    }

    fn entry(&mut self, key: CString) -> ordermap::map::Entry<'_, std::ffi::CString, ()> {
        self.base.entry(key)
    }

    #[allow(dead_code)]
    fn strings(&self) -> ordermap::map::Keys<'_, std::ffi::CString, ()> {
        self.base.keys()
    }

    #[inline]
    fn len(&self) -> usize {
        self.base.len()
    }
}

impl IntoIterator for Symbols {
    type Item = (CString, ());

    type IntoIter = ordermap::map::IntoIter<CString, ()>;

    fn into_iter(self) -> Self::IntoIter {
        self.base.into_iter()
    }
}

impl TryFrom<OrderMap<CString, ()>> for Symbols {
    type Error = Error;

    fn try_from(base: OrderMap<CString, ()>) -> Result<Self, Self::Error> {
        // particleSystemDefinitions
        // DmeParticleSystemDefinition

        let particle_system_definitions_name_idx = base
            .iter()
            .find_position(|el| el.0 == c"particleSystemDefinitions")
            .ok_or(Error::MissingRootDefinitionString)?
            .0 as NameIndex;

        let particle_system_definition_type_idx = base
            .iter()
            .find_position(|el| el.0 == c"DmeParticleSystemDefinition")
            .ok_or(Error::MissingSystemDefinitionString)?
            .0 as NameIndex;

        let material_name_idx = base
            .iter()
            .find_position(|el| el.0 == c"material")
            .ok_or(Error::MissingSystemDefinitionString)?
            .0 as NameIndex;

        Ok(Self {
            particle_system_definitions_name_idx,
            particle_system_definition_type_idx,
            material_name_idx,
            base,
        })
    }
}

#[derive(Debug, Error)]
pub enum GetError {
    #[error(
        "our strings list is missing 'DmeParticleSystemDefinition'. Adding this string may require elements to be fixed up."
    )]
    MissingSystemDefinitionString,
}

#[derive(Debug, Error)]
pub enum MergeError {
    #[error("can't merge PCF with version {0} into PCF with version {1}")]
    VersionMismatch(Version, Version),

    #[error(
        "our strings list is missing 'DmeParticleSystemDefinition'. Adding this string may require elements to be fixed up."
    )]
    SelfIsMissingSystemDefinitionString,
}

impl Pcf {
    #[allow(clippy::cast_possible_truncation)]
    pub fn index_of_string(&self, string: &CStr) -> Option<u16> {
        self.strings.iter().position(|el| el.0 == string).map(|el| el as u16)
    }

    pub fn get_element_type(&self, element: &Element) -> Option<&CString> {
        self.strings.get_index(element.type_idx as usize).map(|el| el.0)
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
                return;
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

    /// Merges the contents of `self` and `other` together.
    ///
    /// Strings and elements are moved into the merged [`Pcf`]. Duplicate strings are skipped and references to skipped
    /// strings will be updated to use the first one.
    ///
    /// If a particle system definition in `other` has a name that matches an element already in `self`, then it is
    /// skipped. Any references to that particle system definition are updated to point to the element in `self` instead.
    /// If an element in `other` has a name that matches an element already in `self`, then it is skipped. Any
    /// references to the element from `other` are updated to point to the element in `self` instead.
    ///
    /// ## Errors
    ///
    /// Errors if there was an issue merging the objects together. See [`MergeError`].
    pub fn merge(self, other: Pcf) -> Result<Self, MergeError> {
        if other.version != self.version {
            return Err(MergeError::VersionMismatch(other.version, self.version));
        }

        let mut strings = self.strings;

        // The PCF format is based on DMX, so there are no guarantees that the strings list will be identical between
        // two PCF files. Its possible to have new strings, or strings that have changed position. So, we create a
        // map here to convert from incoming string index to merged string index.
        //
        // We also add any new strings from `other` into `self.strings` here.
        let mut other_to_new_string_idx = HashMap::new();
        for (other_idx, (other_string, _)) in other.strings.into_iter().enumerate() {
            let mapped_idx = strings.entry(other_string).insert_entry(()).index();
            other_to_new_string_idx.insert(other_idx as TypeIndex, mapped_idx as TypeIndex);
        }

        let system_name_idx = strings
            .iter()
            .position(|(el, _)| el == c"DmeParticleSystemDefinition")
            .ok_or(MergeError::SelfIsMissingSystemDefinitionString)? as NameIndex;

        // other's elements may have attributes which refer to indices of other's elements. We'll sum those
        // references with this element_offset as we add them to the combined elements list, to make sure the
        // references stay intact.
        let element_offset = self.elements.len() as u32;

        let mut elements_by_name = self.elements_by_name;

        // we only want to add elements which aren't already present in self. This will break refernces to elements
        // that are filtered out, so later we'll reindex element references as we add them to our combined list
        let mut filtered_other_elements = Vec::new();
        let mut other_to_new_element_idx = HashMap::new();
        for (other_idx, other_element) in other.elements.into_iter().enumerate() {
            if let Some(new_idx) = elements_by_name.get(&other_element.name).copied() {
                other_to_new_element_idx.insert(other_idx as u32, new_idx);
                continue;
            }

            filtered_other_elements.push(other_element);
        }

        let mut new_system_indices = Vec::new();
        let mut elements = self.elements;
        elements.reserve_exact(filtered_other_elements.len());

        for other_element in filtered_other_elements {
            // string indices may have changed, so we're remapping to the new index
            let type_idx = *other_to_new_string_idx
                .get(&other_element.type_idx)
                .expect("the element's type_idx should always match a value in the Pcf's string list");

            let new_element_idx = elements.len() as u32;
            elements_by_name.insert(other_element.name.clone(), new_element_idx);

            // when adding a new DmeParticleSystemDefinition element, we need to make sure the root node's
            // particleSystemDefinitions list is updated with the new element references
            if type_idx == system_name_idx {
                new_system_indices.push(new_element_idx)
            }

            // when we merge in another PCF's elements, we are basically just appending all new elements to our elements.
            // the incoming PCF's elements references will be incorrect - because the indices have been offset by the
            // elements already in our list. So, we have to fixup every name index and element reference for each
            // attribute in each incoming element.
            let attributes: OrderMap<_, _> = other_element
                .attributes
                .into_iter()
                .map(|(name_idx, attribute)| {
                    Self::reindex_other_attribute(
                        element_offset,
                        &other_to_new_string_idx,
                        &other_to_new_element_idx,
                        name_idx,
                        attribute,
                    )
                })
                .collect();

            elements.push(Element {
                type_idx,
                attributes,
                ..other_element
            });
        }

        // making sure that our merged PCF contains references for all new particle system definitions
        let mut root = self.root;
        root.definitions = [root.definitions.as_ref(), new_system_indices.as_slice()]
            .concat()
            .into_boxed_slice();

        Ok(Pcf {
            version: self.version,
            strings,
            root,
            elements_by_name: elements
                .iter()
                .enumerate()
                .map(|(idx, el)| (el.name.clone(), idx as u32))
                .collect(),
            elements,
        })
    }

    pub fn builder() -> PcfBuilder<NoVersion, NoStrings, NoElements, NoRoot> {
        PcfBuilder {
            version: NoVersion,
            strings: NoStrings,
            root: NoRoot,
            elements: Vec::new(),
            elements_by_name: HashMap::new(),
            _phantom_elements: PhantomData,
            _phantom_strings: PhantomData,
        }
    }

    fn reindex_other_attribute(
        element_offset: u32,
        other_to_new_string_idx: &HashMap<u16, u16>,
        other_to_new_element_idx: &HashMap<u32, u32>,
        name_idx: NameIndex,
        attribute: Attribute,
    ) -> (NameIndex, Attribute) {
        let name_idx = other_to_new_string_idx
            .get(&name_idx)
            .copied()
            .expect("the attribute's name_idx should always match a value in the Pcf's string list");

        let attribute = match attribute {
            Attribute::Element(value) if value != u32::MAX => {
                let new_idx = other_to_new_element_idx
                    .get(&value)
                    .copied()
                    .unwrap_or(value + element_offset);

                Attribute::Element(new_idx)
            }
            Attribute::ElementArray(mut items) => {
                for item in items.iter_mut() {
                    if *item == u32::MAX {
                        continue;
                    }

                    *item = other_to_new_element_idx
                        .get(item)
                        .copied()
                        .unwrap_or(*item + element_offset);
                }

                Attribute::ElementArray(items)
            }
            attribute => attribute,
        };

        (name_idx, attribute)
    }
    
    pub fn get_particle_system_definitions(&self) -> impl Iterator<Item = &Element> {
        self.elements.iter().filter(|el| el.type_idx == self.strings.particle_system_definition_type_idx)
    }

    pub fn get_material<'a>(&'a self, element: &'a Element) -> Option<&'a CString> {
        match element.attributes.get(&self.strings.material_name_idx) {
            Some(Attribute::String(material)) => Some(material),
            _ => None,
        }
    }
}

#[derive(Default, Debug, PartialEq)]
pub struct NoVersion;
#[derive(Default, Debug, PartialEq)]
pub struct NoStrings;
#[derive(Default, Debug, PartialEq)]
pub struct NoElements;
#[derive(Default, Debug, PartialEq)]
pub struct NoRoot;
#[derive(Default, Debug, PartialEq)]
pub struct Elements;

#[derive(Default, Debug)]
pub struct PcfBuilder<A, B, C, D> {
    version: A,
    strings: B,
    root: D,
    elements: Vec<Element>,
    elements_by_name: HashMap<CString, u32>,

    _phantom_strings: PhantomData<B>,
    _phantom_elements: PhantomData<C>,
}

impl PcfBuilder<Version, Symbols, Elements, Root> {
    pub fn build(self) -> Pcf {
        Pcf {
            version: self.version,
            strings: self.strings,
            root: self.root,
            elements: self.elements,
            elements_by_name: self.elements_by_name,
        }
    }
}

impl<B, C, D> PcfBuilder<NoVersion, B, C, D> {
    pub fn version(self, version: Version) -> PcfBuilder<Version, B, C, D> {
        PcfBuilder {
            version,
            strings: self.strings,
            root: self.root,
            elements: self.elements,
            elements_by_name: self.elements_by_name,
            _phantom_elements: PhantomData,
            _phantom_strings: PhantomData,
        }
    }
}

impl<A, C, D> PcfBuilder<A, NoStrings, C, D> {
    pub fn strings(self, strings: Symbols) -> PcfBuilder<A, Symbols, C, D> {
        PcfBuilder {
            version: self.version,
            strings,
            root: self.root,
            elements: self.elements,
            elements_by_name: self.elements_by_name,
            _phantom_elements: PhantomData,
            _phantom_strings: PhantomData,
        }
    }
}

impl<A, B, D> PcfBuilder<A, B, NoElements, D> {
    pub fn elements(self, elements: Vec<Element>) -> PcfBuilder<A, B, Elements, D> {
        let mut elements_by_name = self.elements_by_name;
        for (idx, element) in elements.iter().enumerate() {
            elements_by_name.insert(element.name.clone(), idx as u32);
        }

        PcfBuilder {
            version: self.version,
            strings: self.strings,
            root: self.root,
            elements,
            elements_by_name,
            _phantom_elements: PhantomData,
            _phantom_strings: PhantomData,
        }
    }

    pub fn element(self, element: Element) -> PcfBuilder<A, B, Elements, D> {
        let mut elements_by_name = self.elements_by_name;
        elements_by_name.insert(element.name.clone(), 0);

        PcfBuilder {
            version: self.version,
            strings: self.strings,
            root: self.root,
            elements: vec![element],
            elements_by_name,
            _phantom_elements: PhantomData,
            _phantom_strings: PhantomData,
        }
    }
}

impl<A, B, D> PcfBuilder<A, B, Elements, D> {
    pub fn element(self, element: Element) -> PcfBuilder<A, B, Elements, D> {
        let element_name = element.name.clone();

        let mut elements = self.elements;
        elements.push(element);

        let mut elements_by_name = self.elements_by_name;
        elements_by_name.insert(element_name, (elements.len() - 1) as u32);

        PcfBuilder {
            version: self.version,
            strings: self.strings,
            root: self.root,
            elements,
            elements_by_name,
            _phantom_elements: PhantomData,
            _phantom_strings: PhantomData,
        }
    }
}

impl<A, B, C, NoRoot> PcfBuilder<A, B, C, NoRoot> {
    pub fn root(self, root: Root) -> PcfBuilder<A, B, C, Root> {
        PcfBuilder {
            version: self.version,
            strings: self.strings,
            root,
            elements: self.elements,
            elements_by_name: self.elements_by_name,
            _phantom_strings: PhantomData,
            _phantom_elements: PhantomData,
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
    ReadError(#[from] attribute::ReadError),

    #[error("The DMX string list does not contain 'particleSystemDefinitions', so it cant be a valid PCF")]
    MissingRootDefinitionString,

    #[error("The DMX string list does not contain 'DmeParticleSystemDefinition', so it cant be a valid PCF")]
    MissingSystemDefinitionString,

    #[error("The DMX string list does not contain 'material', so it cant be a valid PCF")]
    MissingMaterialString,

    #[error(
        "The DMX element list does not contain a valid root element with a particle systems definition list, so it cant be a valid PCF"
    )]
    MissingRootDefinitions,
}

// reading functions
impl Pcf {
    pub fn decode(buf: &mut impl std::io::BufRead) -> Result<Pcf, Error> {
        let version = Self::read_magic_version(buf)?;
        let strings = Self::read_strings(buf)?;

        let (root, elements) = Self::read_elements(strings.particle_system_definitions_name_idx, buf)?;

        let mut elements_by_name = HashMap::new();
        for (idx, element) in elements.iter().enumerate() {
            elements_by_name.insert(element.name.clone(), idx as u32);
        }

        Ok(Self {
            version,
            strings,
            root,
            elements,
            elements_by_name,
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

        let version = CStr::from_bytes_with_nul(&header_buf)?;
        let version = Version::try_from(version)?;
        Ok(version)
    }

    fn read_strings(file: &mut impl std::io::BufRead) -> Result<Symbols, Error> {
        let string_count = file.read_u16::<LittleEndian>()? as usize;
        let mut strings = OrderMap::with_capacity(string_count);
        for _ in 0..string_count {
            strings.insert(Self::read_terminated_string(file)?, ());
        }

        let strings = Symbols::try_from(strings)?;

        Ok(strings)
    }

    fn read_elements(
        definitions_name_idx: NameIndex,
        file: &mut impl std::io::BufRead,
    ) -> Result<(Root, Vec<Element>), Error> {
        let element_count = file.read_u32::<LittleEndian>()? as usize - 1;

        let root_type_idx = file.read_u16::<LittleEndian>()?;
        let root_name = Self::read_terminated_string(file)?;
        let root_signature = file.read_array::<16>()?;

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

        let attributes: Result<Vec<_>, _> = AttributeReader::try_from(file, element_count)?.into_iter().collect();
        let mut attributes = attributes?;

        let (0, name_idx, Attribute::ElementArray(root_definitions)) = attributes.remove(0) else {
            return Err(Error::MissingRootDefinitions);
        };

        if name_idx != definitions_name_idx {
            return Err(Error::MissingRootDefinitions);
        }

        let attributes = attributes.into_iter().chunk_by(|el| el.0);
        for (element_idx, group) in attributes.into_iter() {
            let element = elements.get_mut(element_idx).expect("this should never happen");
            element.attributes = group.map(|el| (el.1, el.2)).collect();
        }

        Ok((
            Root {
                type_idx: root_type_idx,
                name: root_name,
                signature: root_signature,
                definitions: root_definitions,
            },
            elements,
        ))
    }
}

// writing functions
impl Pcf {
    pub fn encode(&self, file: &mut impl std::io::Write) -> anyhow::Result<()> {
        Self::write_magic_version(&self.version, file)?;
        Self::write_strings(&self.strings, file)?;
        self.write_elements(file)?;

        Ok(())
    }

    fn write_magic_version(version: &Version, file: &mut impl std::io::Write) -> anyhow::Result<()> {
        let version = version.as_cstr_with_nul_terminator().to_bytes_with_nul();
        file.write_all(version)?;

        Ok(())
    }

    fn write_strings(strings: &Symbols, file: &mut impl std::io::Write) -> anyhow::Result<()> {
        file.write_u16::<LittleEndian>(strings.len() as u16)?;

        for (string, _) in &strings.base {
            file.write_all(string.to_bytes_with_nul())?;
        }

        Ok(())
    }

    fn write_elements(&self, file: &mut impl std::io::Write) -> anyhow::Result<()> {
        // we add 1 because root isn't accounted for in the elements vec
        file.write_u32::<LittleEndian>(self.elements.len() as u32 + 1)?;

        file.write_u16::<LittleEndian>(self.root.type_idx)?;
        file.write_all(self.root.name.to_bytes_with_nul())?;
        file.write_all(&self.root.signature)?;

        for element in &self.elements {
            file.write_u16::<LittleEndian>(element.type_idx)?;
            file.write_all(element.name.to_bytes_with_nul())?;
            file.write_all(&element.signature)?;
        }

        AttributeWriter::from(file).write_attributes(
            self.strings.particle_system_definitions_name_idx,
            &self.root.definitions,
            &self.elements
        )?;

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
        assert_eq!(pcf.strings.strings()[79], c"rotation_offset_max");
        assert_eq!(pcf.strings.strings()[160], c"end time max");
        assert_eq!(pcf.strings.strings()[220], c"warp max");

        assert_eq!(pcf.root.definitions.len(), 171);
        assert_eq!(pcf.elements.len(), 2027);

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
