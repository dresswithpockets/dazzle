use std::{
    collections::{BTreeMap, HashMap, HashSet},
    ffi::{CStr, CString},
    fmt::Display,
    marker::PhantomData,
    vec,
};

use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use itertools::Itertools;
use ordermap::{OrderMap, OrderSet};
use thiserror::Error;

use crate::{
    attribute::{
        self, Attribute, AttributeReader, AttributeWriter, Bool8, Color, Float, Matrix, NameIndex, Vector2, Vector3,
        Vector4,
    },
    index::ElementIdx,
};

#[derive(Debug, PartialEq, Eq, Clone, Copy, Default)]
pub enum Version {
    Binary2Dmx1,
    #[default]
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
    pub signature: Signature,
    pub attributes: OrderMap<NameIndex, Attribute>,
}

pub trait ElementsExt<'a>: Iterator<Item = &'a Element> + Sized {
    fn map_particle_system_indices(self, particle_system_type_idx: &NameIndex) -> impl Iterator<Item = ElementIdx> {
        self.enumerate().filter_map(|(element_idx, element)| {
            if element.type_idx == *particle_system_type_idx {
                Some(element_idx.into())
            } else {
                None
            }
        })
    }
}

impl<'a, T: Iterator<Item = &'a Element> + Sized> ElementsExt<'a> for T {}

pub type Signature = [u8; 16];

#[derive(Debug, Clone, Default)]
pub struct Root {
    pub type_idx: TypeIndex,
    pub name: CString,
    pub signature: Signature,
    pub definitions: Box<[ElementIdx]>,
    pub attributes: OrderMap<NameIndex, Attribute>,
}

#[derive(Debug, Clone, Default)]
/// A Valve Particles Config File. These are DMX files with certain constraints:
///
/// - there is always a root element with an Element Array referencing every partical system
/// - all elements are either a particle system, or a child element of a particle system's definition tree
pub struct Pcf {
    version: Version,
    strings: Symbols,
    root: Root,
    elements: Vec<Element>,

    last_encoded_size: u64,
}

#[derive(Debug, Clone, Default)]
pub struct Symbols {
    pub particle_system_definitions_name_idx: NameIndex,
    pub particle_system_definition_type_idx: NameIndex,
    pub particle_child_type_idx: NameIndex,
    pub particle_operator_type_idx: NameIndex,
    pub function_name_name_idx: NameIndex,
    pub material_name_idx: NameIndex,
    pub base: OrderMap<CString, ()>,
}

impl Symbols {
    pub fn iter(&self) -> ordermap::map::Iter<'_, std::ffi::CString, ()> {
        self.base.iter()
    }

    pub fn find_index(&self, string: &CStr) -> Option<NameIndex> {
        self.base
            .iter()
            .position(|el| el.0 == string)
            .map(|result| result as NameIndex)
    }

    pub fn get_index(&self, idx: usize) -> Option<(&CString, &())> {
        self.base.get_index(idx)
    }

    pub fn entry(&mut self, key: CString) -> ordermap::map::Entry<'_, std::ffi::CString, ()> {
        self.base.entry(key)
    }

    #[allow(dead_code)]
    pub fn strings(&self) -> ordermap::map::Keys<'_, std::ffi::CString, ()> {
        self.base.keys()
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.base.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn get_name(&self, name_idx: u16) -> Option<&CString> {
        self.get_index(name_idx as usize).map(|el| el.0)
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

        let particle_child_type_idx = base
            .iter()
            .find_position(|el| el.0 == c"DmeParticleChild")
            .map(|(idx, _)| idx as NameIndex)
            .unwrap_or(NameIndex::MAX);

        let particle_operator_type_idx = base
            .iter()
            .find_position(|el| el.0 == c"DmeParticleOperator")
            .map(|(idx, _)| idx as NameIndex)
            .unwrap_or(NameIndex::MAX);

        let function_name_name_idx = base
            .iter()
            .find_position(|el| el.0 == c"functionName")
            .map(|(idx, _)| idx as NameIndex)
            .unwrap_or(NameIndex::MAX);

        let material_name_idx = base
            .iter()
            .find_position(|el| el.0 == c"material")
            .ok_or(Error::MissingSystemDefinitionString)?
            .0 as NameIndex;

        Ok(Self {
            particle_system_definitions_name_idx,
            particle_system_definition_type_idx,
            particle_child_type_idx,
            particle_operator_type_idx,
            function_name_name_idx,
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

#[derive(Debug, Error)]
pub enum PushParticleSystemError {
    #[error("can't find element {0} in specified pcf")]
    MissingElement(ElementIdx),

    #[error("the element is not a particle system definition")]
    NonParticleSystemDefinitionElement,
}

impl Pcf {
    pub fn version(&self) -> Version {
        self.version
    }

    pub fn strings(&self) -> &Symbols {
        &self.strings
    }

    pub fn root(&self) -> &Root {
        &self.root
    }

    pub fn elements(&self) -> &Vec<Element> {
        &self.elements
    }

    pub fn into_parts(self) -> (Symbols, Vec<Element>) {
        (self.strings, self.elements)
    }

    pub fn get(&self, idx: ElementIdx) -> Option<&Element> {
        self.elements.get(usize::from(idx))
    }

    #[allow(clippy::cast_possible_truncation)]
    pub fn index_of_string(&self, string: &CStr) -> Option<u16> {
        self.strings.iter().position(|el| el.0 == string).map(|el| el as u16)
    }

    pub fn get_element_type(&self, element: &Element) -> Option<&CString> {
        self.strings.get_index(element.type_idx as usize).map(|el| el.0)
    }

    pub fn get_dependencies(&self, element_idx: ElementIdx) -> OrderSet<ElementIdx> {
        fn visit(pcf: &Pcf, visited: &mut OrderSet<ElementIdx>, element_idx: ElementIdx) {
            // NB insert returns false when insertion fails
            if !visited.insert(element_idx) {
                return;
            }

            let Some(element) = pcf.get(element_idx) else {
                return;
            };

            // Element and Element Array attributes contain indices for other elements
            for (_, attribute) in &element.attributes {
                match attribute {
                    Attribute::Element(value) => {
                        if !value.is_valid() {
                            eprintln!("attribute has ElementIdx::INVALID value");
                            continue;
                        }

                        visit(pcf, visited, *value);
                    }
                    Attribute::ElementArray(values) => {
                        for value in values {
                            if !value.is_valid() {
                                eprintln!("attribute has ElementIdx::INVALID value");
                                continue;
                            }

                            visit(pcf, visited, *value);
                        }
                    }
                    _ => continue,
                }
            }
        }

        let mut visited = OrderSet::new();
        visit(self, &mut visited, element_idx);
        visited.remove(&element_idx);

        visited
    }

    pub fn new_from_elements<'a>(&'a self, elements: impl IntoIterator<Item = &'a ElementIdx>) -> Self {
        let new_elements = Self::reindex_elements(self, elements);

        let particle_system_indices: Vec<ElementIdx> = new_elements
            .iter()
            .map_particle_system_indices(&self.strings().particle_system_definition_type_idx)
            .collect();

        // our filtered `new_elements` only contains particle systems, it does not contain a root element
        let root = Root {
            type_idx: self.root().type_idx,
            name: self.root().name.clone(),
            signature: self.root().signature,
            definitions: particle_system_indices.into_boxed_slice(),
            attributes: self.root().attributes.clone(), // TODO: do we need to reindex these?
        };

        // this new in-memory PCF has only the elements listed in elements_to_extract, with element references
        // fixed to match any changes in indices.
        Self::builder()
            .version(self.version())
            .strings(self.strings().clone())
            .root(root)
            .elements(new_elements)
            .build()
    }

    // pub fn get_dependent_indices(&self, name: &CString) -> Option<HashSet<u32>> {
    //     fn visit(visited: &mut HashSet<u32>, elements: &Vec<Element>, idx: u32) {
    //         // NB insert returns false when insertion fails
    //         if !visited.insert(idx) {
    //             return;
    //         }

    //         let Some(element) = elements.get(idx as usize) else {
    //             return;
    //         };

    //         // Element and Element Array attributes contain indices for other elements
    //         for (_, attribute) in &element.attributes {
    //             match attribute {
    //                 Attribute::Element(value) if *value != u32::MAX => {
    //                     visit(visited, elements, *value);
    //                 }
    //                 Attribute::ElementArray(values) => {
    //                     for value in values {
    //                         if *value == u32::MAX {
    //                             continue;
    //                         }

    //                         visit(visited, elements, *value);
    //                     }
    //                 }
    //                 _ => continue,
    //             }
    //         }
    //     }

    //     let idx = self.get_element_index(name)?;

    //     let mut visited = HashSet::new();
    //     visit(&mut visited, &self.elements, idx);

    //     Some(visited)
    // }

    // pub fn push_particle_system(&mut self, from: &Pcf, element_idx: ElementIdx) -> Result<(), PushParticleSystemError> {
    //     let element = from.get(element_idx).ok_or(PushParticleSystemError::MissingElement(element_idx))?;

    //     if element.type_idx != from.strings.particle_system_definition_type_idx {
    //         return Err(PushParticleSystemError::NonParticleSystemDefinitionElement)
    //     }

    //     let new_system_indices = vec![element_idx];
    //     let element = from.get(element_idx).unwrap();

    //     self.elements.push(Element {
    //         type_idx: self.strings.particle_system_definition_type_idx,
    //         name: element.name.clone(),
    //         signature: element.signature,
    //         attributes: element.attributes.iter().map(|name_idx, attribute| ()),
    //     });

    //     for element_idx in from.get_dependencies(element_idx) {
    //         let new_idx = self.elements.len();
    //         self.elements.push(*element);
    //     }

    //     self.root.definitions = [self.root.definitions.as_ref(), &[element_idx]]
    //         .concat()
    //         .into_boxed_slice();

    //     Ok(())
    // }

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

        // other's elements may have attributes which refer to indices of other's elements. We'll sum those
        // references with this element_offset as we add them to the combined elements list, to make sure the
        // references stay intact.
        let element_offset = self.elements.len();

        let mut new_system_indices: Vec<ElementIdx> = Vec::new();
        let mut elements = self.elements;
        elements.reserve_exact(other.elements.len());

        for other_element in other.elements.into_iter() {
            // string indices may have changed, so we're remapping to the new index
            let type_idx = *other_to_new_string_idx
                .get(&other_element.type_idx)
                .expect("the element's type_idx should always match a value in the Pcf's string list");

            // the new element is getting pushed at thie end of this loop, so the current len() will be its index
            let new_element_idx: ElementIdx = elements.len().into();

            // when adding a new DmeParticleSystemDefinition element, we need to make sure the root node's
            // particleSystemDefinitions list is updated with the new element references
            if type_idx == strings.particle_system_definition_type_idx {
                new_system_indices.push(new_element_idx)
            }

            // when we merge in another PCF's elements, we're basically just appending all new elements to our elements.
            // the incoming PCF's elements references will be incorrect - because the indices have been offset by the
            // elements already in our list. So, we have to fixup every name index and element reference for each
            // attribute in each incoming element.
            let attributes: OrderMap<NameIndex, Attribute> = other_element
                .attributes
                .into_iter()
                .map(|(name_idx, attribute)| {
                    Self::reindex_other_attribute(element_offset, &other_to_new_string_idx, name_idx, attribute)
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

        let mut new_pcf = Pcf {
            version: self.version,
            strings,
            root,
            elements,
            last_encoded_size: 0,
        };

        new_pcf.recompute_encoded_size();

        Ok(new_pcf)
    }

    pub fn merge_in(&mut self, other: Pcf) -> Result<(), MergeError> {
        *self = std::mem::take(self).merge(other)?;

        Ok(())
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
        element_offset: usize,
        other_to_new_string_idx: &HashMap<u16, u16>,
        name_idx: NameIndex,
        attribute: Attribute,
    ) -> (NameIndex, Attribute) {
        let name_idx = other_to_new_string_idx
            .get(&name_idx)
            .copied()
            .expect("the attribute's name_idx should always match a value in the Pcf's string list");

        let attribute = match attribute {
            Attribute::Element(value) if value.is_valid() => Attribute::Element(value + element_offset),
            Attribute::ElementArray(mut items) => {
                for item in items.iter_mut() {
                    if !item.is_valid() {
                        continue;
                    }

                    *item += element_offset
                }

                Attribute::ElementArray(items)
            }
            attribute => attribute,
        };

        (name_idx, attribute)
    }

    pub fn get_particle_system_definitions(&self) -> impl Iterator<Item = (ElementIdx, &Element)> {
        self.root.definitions.iter().map(|idx| (*idx, self.get(*idx).unwrap()))

        // self.elements
        //     .iter()
        //     .enumerate()
        //     .filter_map(|(idx, el)| {
        //         if el.type_idx == self.strings.particle_system_definition_type_idx {
        //             Some((ElementIdx::from(idx), el))
        //         } else {
        //             None
        //         }
        //     })
    }

    pub fn get_root_particle_systems(&self) -> HashMap<ElementIdx, &Element> {
        let mut definitions: HashMap<_, _> = self
            .root
            .definitions
            .iter()
            .map(|idx| (*idx, self.get(*idx).unwrap()))
            .collect();

        for system_idx in &self.root.definitions {
            for dependency in self.get_dependencies(*system_idx) {
                definitions.remove(&dependency);
            }
        }

        definitions
    }

    // pub fn is_dependent(&self, parent: ElementIdx, child: ElementIdx) -> bool {
    //     fn visit(pcf: &Pcf, visited: &mut OrderSet<ElementIdx>, parent: ElementIdx, child: ElementIdx) -> bool {
    //         assert_ne!(parent, child);

    //         // NB insert returns false when insertion fails
    //         if !visited.insert(parent) {
    //             return false;
    //         }

    //         let Some(element) = pcf.get(parent) else {
    //             return false;
    //         };

    //         // Element and Element Array attributes contain indices for other elements
    //         for (_, attribute) in &element.attributes {
    //             match attribute {
    //                 Attribute::Element(value) => {
    //                     if *value == ElementIdx::INVALID {
    //                         eprintln!("attribute has ElementIdx::INVALID value");
    //                         continue
    //                     }

    //                     visit(pcf, visited, *value);
    //                 }
    //                 Attribute::ElementArray(values) => {
    //                     for value in values {
    //                         if *value == ElementIdx::INVALID {
    //                             eprintln!("attribute has ElementIdx::INVALID value");
    //                             continue;
    //                         }

    //                         visit(pcf, visited, *value);
    //                     }
    //                 }
    //                 _ => continue,
    //             }
    //         }

    //         return false;
    //     }

    //     let mut visited = OrderSet::new();
    //     visit(self, &mut visited, element_idx);
    //     visited.remove(&element_idx);

    //     visited
    // }

    pub fn get_material<'a>(&'a self, element: &'a Element) -> Option<&'a CString> {
        match element.attributes.get(&self.strings.material_name_idx) {
            Some(Attribute::String(material)) => Some(material),
            _ => None,
        }
    }

    pub fn reindex_elements<'a>(
        source_pcf: &'a Pcf,
        systems: impl IntoIterator<Item = &'a ElementIdx>,
    ) -> Vec<Element> {
        let mut new_elements = Vec::new();
        let mut original_elements: BTreeMap<ElementIdx, &Element> = BTreeMap::new();
        for system_idx in systems {
            let system = source_pcf.get(*system_idx).expect("this should never fail");
            let dependencies = source_pcf.get_dependencies(*system_idx);

            original_elements.insert(*system_idx, system);

            for child_idx in dependencies {
                let element = source_pcf.get(child_idx).expect("this should never happen");
                original_elements.entry(child_idx).or_insert(element);
            }
        }

        #[allow(clippy::cast_possible_truncation)]
        let old_to_new_idx: HashMap<ElementIdx, ElementIdx> = original_elements
            .iter()
            .enumerate()
            .map(|(new_idx, (old_idx, _))| (*old_idx, new_idx.into()))
            .collect();

        for (_, element) in original_elements {
            let mut attributes = OrderMap::new();

            // this monstrosity is re-mapping old element references to new ones using the new indices mapped
            // in old_to_new_idx
            for (name_idx, attribute) in &element.attributes {
                let new_attribute = match attribute {
                    Attribute::Element(old_idx) if old_idx.is_valid() => {
                        Attribute::Element(*old_to_new_idx.get(old_idx).unwrap_or(old_idx))
                    }
                    Attribute::ElementArray(old_indices) => Attribute::ElementArray(
                        old_indices
                            .iter()
                            .map(|old_idx| {
                                if old_idx.is_valid() {
                                    *old_to_new_idx.get(old_idx).unwrap_or(old_idx)
                                } else {
                                    *old_idx
                                }
                            })
                            .collect(),
                    ),
                    attribute => attribute.clone(),
                };

                attributes.insert(*name_idx, new_attribute);
            }

            new_elements.push(Element {
                type_idx: element.type_idx,
                name: element.name.clone(),
                signature: element.signature,
                attributes,
            });
        }

        new_elements
    }

    /// Consumes a [`Pcf`], iterating over all of its elements to strip unecessary default values.
    pub fn stripped(
        mut self,
        particle_system_defaults: &HashMap<&'static CStr, Attribute>,
        operator_defaults: &HashMap<CString, HashMap<CString, Attribute>>,
    ) -> Self {
        let particle_system_defaults: HashMap<NameIndex, &Attribute> = particle_system_defaults
            .iter()
            .filter_map(|(key, value)| {
                self.strings
                    .iter()
                    .position(|s| s.0.as_bytes().eq_ignore_ascii_case(key.to_bytes()))
                    .map(|idx| (idx as NameIndex, value))
            })
            .collect();

        let operator_defaults: HashMap<_, _> = operator_defaults
            .iter()
            .map(|(function_name, defaults)| {
                let map: HashMap<_, _> = defaults
                    .iter()
                    .filter_map(|(attribute_name, attribute)| {
                        let name_idx = self.strings.find_index(attribute_name)?;
                        Some((name_idx, attribute))
                    })
                    .collect();

                (function_name, map)
            })
            .collect();

        self.elements = self
            .elements
            .into_iter()
            .map(|element| {
                let attributes = if element.type_idx == self.strings.particle_system_definition_type_idx {
                    element
                        .attributes
                        .into_iter()
                        .filter(|(name_idx, attribute)| {
                            if let Some(default) = particle_system_defaults.get(name_idx)
                                && attribute == *default
                            {
                                false
                            } else {
                                true
                            }
                        })
                        .collect()
                } else if element.type_idx == self.strings.particle_operator_type_idx {
                    if let Some(Attribute::String(function_name)) =
                        element.attributes.get(&self.strings.function_name_name_idx)
                    {
                        if let Some(default_attributes) = operator_defaults.get(function_name) {
                            element
                                .attributes
                                .into_iter()
                                .filter(|(name_idx, attribute)| {
                                    if let Some(default) = default_attributes.get(name_idx)
                                        && *attribute == **default
                                    {
                                        false
                                    } else {
                                        true
                                    }
                                })
                                .collect()
                        } else {
                            element.attributes
                        }
                    } else {
                        element.attributes
                    }
                } else {
                    element.attributes
                };

                Element { attributes, ..element }
            })
            .collect();

        self.recompute_encoded_size();

        self
    }

    pub fn strip_default_values(
        mut self,
        particle_system_defaults: &HashMap<&'static CStr, Attribute>,
        operator_defaults: &HashMap<&'static CStr, Attribute>,
    ) -> Pcf {
        let particle_system_defaults: HashMap<NameIndex, &Attribute> = particle_system_defaults
            .iter()
            .filter_map(|(key, value)| {
                self.strings
                    .iter()
                    .position(|s| s.0.as_bytes().eq_ignore_ascii_case(key.to_bytes()))
                    .map(|idx| (idx as NameIndex, value))
            })
            .collect();

        let operator_defaults: HashMap<NameIndex, &Attribute> = operator_defaults
            .iter()
            .filter_map(|(key, value)| {
                self.strings
                    .iter()
                    .position(|s| s.0.as_bytes().eq_ignore_ascii_case(key.to_bytes()))
                    .map(|idx| (idx as NameIndex, value))
            })
            .collect();

        self.elements = self
            .elements
            .into_iter()
            .map(|element| {
                let attributes = if element.type_idx == self.strings.particle_system_definition_type_idx {
                    element
                        .attributes
                        .into_iter()
                        .filter(|(name_idx, attribute)| {
                            if let Some(default) = particle_system_defaults.get(name_idx)
                                && attribute == *default
                            {
                                false
                            } else {
                                true
                            }
                        })
                        .collect()
                } else if element.type_idx == self.strings.particle_operator_type_idx {
                    element
                        .attributes
                        .into_iter()
                        .filter(|(name_idx, attribute)| {
                            if let Some(default) = operator_defaults.get(name_idx)
                                && attribute == *default
                            {
                                false
                            } else {
                                true
                            }
                        })
                        .collect()
                } else {
                    element.attributes
                };

                Element { attributes, ..element }
            })
            .collect();

        self
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
    elements_by_name: HashMap<CString, ElementIdx>,

    _phantom_strings: PhantomData<B>,
    _phantom_elements: PhantomData<C>,
}

impl PcfBuilder<Version, Symbols, Elements, Root> {
    pub fn build(self) -> Pcf {
        let mut pcf = Pcf {
            version: self.version,
            strings: self.strings,
            root: self.root,
            elements: self.elements,
            last_encoded_size: 0,
        };

        pcf.recompute_encoded_size();

        pcf
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
            elements_by_name.insert(element.name.clone(), idx.into());
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
        elements_by_name.insert(element.name.clone(), 0usize.into());

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

        let element_idx: ElementIdx = self.elements.len().into();
        let mut elements = self.elements;
        elements.push(element);

        let mut elements_by_name = self.elements_by_name;
        elements_by_name.insert(element_name, element_idx);

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

    #[error("The DMX string list does not contain 'DmeParticleChild', so it cant be a valid PCF")]
    MissingParticleChildString,

    #[error("The DMX string list does not contain 'DmeParticleOperator', so it cant be a valid PCF")]
    MissingParticleOperatorString,

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

        let mut pcf = Self {
            version,
            strings,
            root,
            elements,
            last_encoded_size: 0,
        };

        pcf.recompute_encoded_size();

        Ok(pcf)
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

        // we add one to element_count since AttributeReader will read root's attributes + elements' attributes
        let attributes: Result<Vec<_>, _> = AttributeReader::try_from(file, element_count + 1)?
            .into_iter()
            .collect();
        let mut all_attributes = attributes?;

        // TODO: some PCFs have roots that contain more attributes.

        if let Some(first_non_root_idx) = all_attributes.iter().position(|el| el.0 == 1) {
            let attributes = all_attributes
                .split_off(first_non_root_idx)
                .into_iter()
                .chunk_by(|el| el.0);
            for (element_idx, group) in attributes.into_iter() {
                // the element_idx returned by the attribute reader includes root at 0, but we took root out of the list
                // so we need to subtract 1 from the idx
                let element = elements.get_mut(element_idx - 1).expect("this should never happen");
                element.attributes = group.map(|el| (el.1, el.2)).collect();
            }
        }

        // the remaining attributes will all be root attributes
        let (root_definitions, root_attributes) = {
            let mut root_definitions: Option<Box<[ElementIdx]>> = None;
            let mut remaining_attributes = OrderMap::new();
            for (_, name_idx, attribute) in all_attributes {
                if name_idx == definitions_name_idx
                    && let Attribute::ElementArray(root_defs) = attribute
                {
                    root_definitions = Some(root_defs);
                    continue;
                }

                remaining_attributes.insert(name_idx, attribute);
            }

            let Some(root_definitions) = root_definitions else {
                return Err(Error::MissingRootDefinitions);
            };

            (root_definitions, remaining_attributes)
        };

        Ok((
            Root {
                type_idx: root_type_idx,
                name: root_name,
                signature: root_signature,
                definitions: root_definitions,
                attributes: root_attributes,
            },
            elements,
        ))
    }
}

impl Pcf {
    pub fn get_system_graph(&self) -> Vec<Vec<ElementIdx>> {
        todo!();
        // create an element graph, which may or may not contain multiple disconnected subgraphs.
        // take the first element, find all of its directly related elements, and group that into a single set...
        // take the next element, find all of its directly related elements, and see if any of them are in the latest set...
        // if they are, add them all to the set; otherwise, start a new set with these elements

        // TODO: ask around for an algorithm that forms graphs of related nodes
        //       maybe try creating a map of child -> parent and iterating through every root system's dependencies to build a tree
        let mut graphs: Vec<Vec<ElementIdx>> = Vec::new();
        for (element_idx, _) in self.get_root_particle_systems() {
            let dependencies = self.get_dependencies(element_idx);
            if graphs.is_empty() {
                graphs.push(dependencies.into_iter().collect());
            } else {
                todo!();
            }
            // if current_graph.is_empty() {
            //     current_graph.extend(dependencies);
            // } else if dependencies.iter().any(|idx| current_graph.contains(idx)) {
            //     current_graph.extend(dependencies)
            // } else {
            //     graphs.push(current_graph.into_iter().collect_vec());
            //     current_graph = HashSet::new();
            //     current_graph.extend(dependencies);
            // }
        }

        graphs
    }

    pub fn encoded_group_size_in_slow<'a>(
        &'a self,
        elements: impl IntoIterator<Item = &'a ElementIdx>,
        into: &Pcf,
    ) -> u64 {
        let mut into = into.clone();
        let new = self.new_from_elements(elements);
        into.merge_in(new).unwrap();
        into.encoded_size()
    }

    pub fn encoded_group_size_in<'a>(&'a self, elements: impl IntoIterator<Item = &'a ElementIdx>, into: &Pcf) -> u64 {
        fn visit<'a>(
            from: &'a Pcf,
            into: &Pcf,
            element_idx: ElementIdx,
            missing_strings: &mut HashSet<&'a CString>,
        ) -> usize {
            let element = from.get(element_idx).unwrap();
            let additional_definition_size = if element.type_idx == from.strings.particle_system_definition_type_idx {
                size_of::<ElementIdx>()
            } else {
                0
            };

            let element_type_size = size_of::<u16>();
            let element_signature_size = size_of::<Signature>();
            let element_name_size = element.name.to_bytes_with_nul().len();

            let element_attributes_count_size = size_of::<u32>();
            let element_attributes_name_size = size_of::<NameIndex>() * element.attributes.len();
            let element_attributes_type_size = size_of::<u8>() * element.attributes.len();
            let element_attributes_size: usize = element.attributes.values().map(Pcf::encoded_attribute_size).sum();

            let type_string = from.strings.get_name(element.type_idx).unwrap();
            if !into.strings().base.contains_key(type_string) {
                missing_strings.insert(type_string);
            }

            for (name_idx, _) in &element.attributes {
                let name_string = from.strings.get_name(*name_idx).unwrap();
                if !into.strings().base.contains_key(name_string) {
                    missing_strings.insert(name_string);
                }
            }

            additional_definition_size
                + element_type_size
                + element_signature_size
                + element_name_size
                + element_attributes_count_size
                + element_attributes_name_size
                + element_attributes_type_size
                + element_attributes_size
        }

        let mut missing_strings = HashSet::new();
        let elements_size: usize = elements
            .into_iter()
            .map(|element_idx| visit(self, into, *element_idx, &mut missing_strings))
            .sum();
        let missing_strings_size: usize = missing_strings.iter().map(|s| s.to_bytes_with_nul().len()).sum();

        (elements_size + missing_strings_size) as u64
    }

    /// computes how many additional bytes the [`Pcf`] would encode as if this element were added.
    ///
    /// See [`Pcf::encoded_size`].
    pub fn element_encoded_size_in(&self, idx: ElementIdx, into: &Pcf) -> u64 {
        fn visit<'a>(
            from: &'a Pcf,
            into: &Pcf,
            element: &Element,
            visited: &mut HashSet<ElementIdx>,
            missing_strings: &mut HashSet<&'a CString>,
        ) -> usize {
            let additional_definition_size = if element.type_idx == from.strings.particle_system_definition_type_idx {
                size_of::<ElementIdx>()
            } else {
                0
            };

            let element_type_size = size_of::<u16>();
            let element_signature_size = size_of::<Signature>();
            let element_name_size = element.name.to_bytes_with_nul().len();

            let element_attributes_count_size = size_of::<u32>();
            let element_attributes_name_size = size_of::<NameIndex>() * element.attributes.len();
            let element_attributes_type_size = size_of::<u8>() * element.attributes.len();
            let element_attributes_size: usize = element.attributes.values().map(Pcf::encoded_attribute_size).sum();

            let mut related_elements_size = 0;

            let type_string = from.strings.get_name(element.type_idx).unwrap();
            if !into.strings().base.contains_key(type_string) {
                missing_strings.insert(type_string);
            }

            for (name_idx, attribute) in &element.attributes {
                let name_string = from.strings.get_name(*name_idx).unwrap();
                if !into.strings().base.contains_key(name_string) {
                    missing_strings.insert(name_string);
                }

                related_elements_size += match attribute {
                    Attribute::Element(element_idx) => {
                        if element_idx.is_valid() && visited.insert(*element_idx) {
                            visit(from, into, from.get(*element_idx).unwrap(), visited, missing_strings)
                        } else {
                            0
                        }
                    }
                    Attribute::ElementArray(items) => items
                        .iter()
                        .map(|element_idx| {
                            if element_idx.is_valid() && visited.insert(*element_idx) {
                                visit(from, into, from.get(*element_idx).unwrap(), visited, missing_strings)
                            } else {
                                0
                            }
                        })
                        .sum::<usize>(),
                    _ => 0,
                };
            }

            additional_definition_size
                + element_type_size
                + element_signature_size
                + element_name_size
                + element_attributes_count_size
                + element_attributes_name_size
                + element_attributes_type_size
                + element_attributes_size
                + related_elements_size
        }

        let mut visited = HashSet::from([idx]);
        let mut missing_strings = HashSet::new();
        let size = visit(self, into, self.get(idx).unwrap(), &mut visited, &mut missing_strings) as u64;

        let missing_strings_size: usize = missing_strings.iter().map(|s| s.to_bytes_with_nul().len()).sum();
        size + missing_strings_size as u64
    }

    pub fn recompute_encoded_size(&mut self) {
        self.last_encoded_size = self.encoded_magic_version_size()
            + self.encoded_strings_size()
            + self.encoded_elements_size()
            + self.encoded_element_attributes_size();
    }

    /// computes the size of this PCF if it were encoded
    pub fn encoded_size(&self) -> u64 {
        self.last_encoded_size
    }

    fn encoded_magic_version_size(&self) -> u64 {
        self.version.as_cstr_with_nul_terminator().to_bytes_with_nul().len() as u64
    }

    fn encoded_strings_size(&self) -> u64 {
        let count_size = size_of::<u16>();
        let strings_size: usize = self
            .strings
            .base
            .iter()
            .map(|(string, _)| string.to_bytes_with_nul().len())
            .sum();

        (count_size + strings_size) as u64
    }

    fn encoded_elements_size(&self) -> u64 {
        let element_count_size = size_of::<u32>();
        let root_type_size = size_of::<u16>();
        let root_name_size = self.root.name.to_bytes_with_nul().len();
        let root_signature_size = size_of::<Signature>();

        let elements_type_size = size_of::<u16>() * self.elements.len();
        let elements_signature_size = size_of::<Signature>() * self.elements.len();
        let elements_name_size: usize = self
            .elements
            .iter()
            .map(|element| element.name.to_bytes_with_nul().len())
            .sum();

        (element_count_size
            + root_type_size
            + root_name_size
            + root_signature_size
            + elements_type_size
            + elements_signature_size
            + elements_name_size) as u64
    }

    fn encoded_element_attributes_size(&self) -> u64 {
        let root_attribute_count_size = size_of::<u32>();
        let root_name_size = size_of::<u16>();
        let root_type_size = size_of::<u8>();
        let root_definitions_count_size = size_of::<u32>();
        let root_definitions_size = size_of::<ElementIdx>() * self.root.definitions.len();

        let root_attributes_name_size = size_of::<u16>() * self.root.attributes.len();
        let root_attributes_type_size = size_of::<u8>() * self.root.attributes.len();
        let root_attributes_size: usize = self.root.attributes.values().map(Self::encoded_attribute_size).sum();

        let elements_attribute_count_size = size_of::<u32>() * self.elements.len();
        let elements_sizes = self.elements.iter().fold((0, 0, 0), |a, el| {
            let attributes_name_size = size_of::<u16>() * el.attributes.len();
            let attributes_type_size = size_of::<u8>() * el.attributes.len();
            let attributes_size: usize = el.attributes.values().map(Self::encoded_attribute_size).sum();

            (
                a.0 + attributes_name_size,
                a.1 + attributes_type_size,
                a.2 + attributes_size,
            )
        });

        (root_attribute_count_size
            + root_name_size
            + root_type_size
            + root_definitions_count_size
            + root_definitions_size
            + root_attributes_name_size
            + root_attributes_type_size
            + root_attributes_size
            + elements_attribute_count_size
            + elements_sizes.0
            + elements_sizes.1
            + elements_sizes.2) as u64
    }

    fn encoded_attribute_size(attribute: &Attribute) -> usize {
        match attribute {
            Attribute::Element(value) => size_of_val(value),
            Attribute::Integer(value) => size_of_val(value),
            Attribute::Float(value) => size_of_val(value),
            Attribute::Bool(value) => size_of_val(value),
            Attribute::String(string) => string.as_bytes_with_nul().len(),
            Attribute::Binary(binary) => size_of::<u32>() + binary.len(),
            Attribute::Color(value) => size_of_val(value),
            Attribute::Vector2(value) => size_of_val(value),
            Attribute::Vector3(value) => size_of_val(value),
            Attribute::Vector4(value) => size_of_val(value),
            Attribute::Matrix(value) => size_of_val(value),
            Attribute::ElementArray(array) => size_of::<u32>() + size_of::<ElementIdx>() * array.len(),
            Attribute::IntegerArray(array) => size_of::<u32>() + size_of::<i32>() * array.len(),
            Attribute::FloatArray(array) => size_of::<u32>() + size_of::<Float>() * array.len(),
            Attribute::BoolArray(array) => size_of::<u32>() + size_of::<Bool8>() * array.len(),
            Attribute::StringArray(array) => {
                size_of::<u32>() + array.iter().map(|el| el.as_bytes_with_nul().len()).sum::<usize>()
            }
            Attribute::BinaryArray(array) => {
                size_of::<u32>() + (size_of::<u32>() * array.len()) + array.iter().map(|el| el.len()).sum::<usize>()
            }
            Attribute::ColorArray(array) => size_of::<u32>() + size_of::<Color>() * array.len(),
            Attribute::Vector2Array(array) => size_of::<u32>() + size_of::<Vector2>() * array.len(),
            Attribute::Vector3Array(array) => size_of::<u32>() + size_of::<Vector3>() * array.len(),
            Attribute::Vector4Array(array) => size_of::<u32>() + size_of::<Vector4>() * array.len(),
            Attribute::MatrixArray(array) => size_of::<u32>() + size_of::<Matrix>() * array.len(),
        }
    }
}

// writing functions
impl Pcf {
    pub fn encode(&self, file: &mut impl std::io::Write) -> anyhow::Result<()> {
        self.write_magic_version(file)?;
        self.write_strings(file)?;
        self.write_elements(file)?;
        self.write_element_attributes(file)?;

        Ok(())
    }

    pub(crate) fn write_magic_version(&self, file: &mut impl std::io::Write) -> anyhow::Result<()> {
        let version = self.version.as_cstr_with_nul_terminator().to_bytes_with_nul();
        file.write_all(version)?;

        Ok(())
    }

    pub(crate) fn write_strings(&self, file: &mut impl std::io::Write) -> anyhow::Result<()> {
        file.write_u16::<LittleEndian>(self.strings.len() as u16)?;

        for (string, _) in &self.strings.base {
            file.write_all(string.to_bytes_with_nul())?;
        }

        Ok(())
    }

    pub(crate) fn write_elements(&self, file: &mut impl std::io::Write) -> anyhow::Result<()> {
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

        Ok(())
    }

    pub(crate) fn write_element_attributes(&self, file: &mut impl std::io::Write) -> anyhow::Result<()> {
        AttributeWriter::from(file).write_attributes(
            self.strings.particle_system_definitions_name_idx,
            &self.root,
            &self.elements,
        )?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use bytes::{Buf, BufMut, Bytes, BytesMut};

    use super::*;

    const TEST_PCF: &[u8] = include_bytes!("rankup.pcf");
    const DEFAULT_PCF: &[u8] = include_bytes!("default_values.pcf");

    #[test]
    fn encoded_size_is_correct() {
        let expected_size = TEST_PCF.len();

        let mut reader = Bytes::from(TEST_PCF).reader();
        let pcf = Pcf::decode(&mut reader).expect("decoding failed");
        assert_eq!(expected_size, pcf.encoded_size() as usize);

        let buf = BytesMut::with_capacity(TEST_PCF.len());
        let mut writer = buf.writer();
        pcf.encode(&mut writer).expect("writing failed");

        let inner = writer.into_inner();
        assert_eq!(inner.len(), expected_size);
        assert_eq!(inner.len(), pcf.encoded_size() as usize);
    }

    #[test]
    fn element_encoded_size_in_is_correct() {
        let mut reader = Bytes::from(DEFAULT_PCF).reader();
        let default_pcf = Pcf::decode(&mut reader).expect("decoding failed");

        let mut reader = Bytes::from(TEST_PCF).reader();
        let pcf = Pcf::decode(&mut reader).expect("decoding failed");

        let graph = default_pcf.get_system_graph();
        assert_eq!(1, graph.len());

        let estimated_size = default_pcf.encoded_group_size_in_slow(&graph[0], &pcf);

        let new_pcf = default_pcf.new_from_elements(&graph[0]);
        let pcf = pcf.merge(new_pcf).unwrap();

        assert_eq!(estimated_size, pcf.encoded_size());
    }

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
