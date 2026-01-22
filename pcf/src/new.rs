use std::{
    collections::{HashMap, HashSet},
    ffi::{CStr, CString},
    mem,
};

use dmx::{
    ElementIdx, Signature,
    dmx::{Dmx, Element, Version},
};
use itertools::Itertools;
use ordermap::{OrderMap, OrderSet};
use petgraph::{algo::tarjan_scc, prelude::UnGraphMap};
use thiserror::Error;

use crate::{
    attribute::Attribute,
    strings::{str_to_cstring, string_to_cstring},
};

pub type SymbolIdx = u16;
pub type ParticleSystemIdx = usize;

#[derive(Debug, Clone, Default)]
pub struct Pcf {
    version: Version,
    symbols: Symbols,
    root: Root,
    encoded_size: usize,
}

#[derive(Debug, Clone, Default)]
pub struct Root {
    name: String,
    signature: Signature,
    particle_systems: Box<[ParticleSystem]>,
    attributes: OrderMap<SymbolIdx, Attribute>,
}

impl Root {
    pub fn into_parts(self) -> (String, Signature, Box<[ParticleSystem]>, OrderMap<SymbolIdx, Attribute>) {
        (self.name, self.signature, self.particle_systems, self.attributes)
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn signature(&self) -> Signature {
        self.signature
    }

    pub fn particle_systems(&self) -> &[ParticleSystem] {
        &self.particle_systems
    }

    pub fn attributes(&self) -> &OrderMap<SymbolIdx, Attribute> {
        &self.attributes
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("The DMX contains no elements, so it cant be a valid PCF")]
    NoElements,

    #[error("The root element is missing a `partileSystemDefintions` array, so a valid PCF cannot be parsed")]
    MissingRootDefintions,

    #[error("The particle system definitions array contains a reference to an element that doesn't exist")]
    MissingParticleSystem(ElementIdx),

    #[error(
        "The particle system definitions array contains a reference to an element that is not a valid particle system"
    )]
    InvalidParticleSystem(ElementIdx),

    #[error("A particle system references a child element that does not exist")]
    MissingParticleChild(ElementIdx),

    #[error("A particle system references a child element that is not a valid DmeParticleChild")]
    InvalidParticleChild(ElementIdx),

    #[error("The child element is missing a valid child attribute")]
    MissingChild,

    #[error("The operator element is missing a valid function name attribute")]
    MissingFunctionName,

    #[error("The element contains an unexpected Element or ElementArray attribute")]
    UnexpectedElementReference,

    #[error("A particle system contains a reference to an operator that is not a valid DmeParticleOperator")]
    InvalidParticleOperator(ElementIdx),

    #[error("A particle system contains a reference to an operator that doesn't exist")]
    MissingOperator(ElementIdx),

    #[error("The DMX string list does not contain 'DmElement' or 'DmeElement', so it cant be a valid PCF")]
    MissingDatamodelElementString,

    #[error("The DMX string list does not contain 'particleSystemDefinitions', so it cant be a valid PCF")]
    MissingRootDefinitionString,

    #[error("The DMX string list does not contain 'DmeParticleSystemDefinition', so it cant be a valid PCF")]
    MissingSystemDefinitionString,
}

#[derive(Debug, Error)]
pub enum MergeError {
    #[error("can't merge DMX with version {0} into DMX with version {1}")]
    VersionMismatch(Version, Version),
}

impl Pcf {
    pub fn version(&self) -> Version {
        self.version
    }

    pub fn symbols(&self) -> &Symbols {
        &self.symbols
    }

    pub fn root(&self) -> &Root {
        &self.root
    }

    pub fn merged_in(&mut self, from: &mut Self) -> Result<(), MergeError> {
        *self = mem::take(self).merged(mem::take(from))?;
        Ok(())
    }

    pub fn merged(self, from: Self) -> Result<Self, MergeError> {
        fn reindex_new_attributes(
            old_to_new_string_idx: &HashMap<u16, u16>,
            attributes: OrderMap<SymbolIdx, Attribute>,
        ) -> impl Iterator<Item = (u16, Attribute)> {
            attributes.into_iter().map(|(name_idx, attribute)| {
                let name_idx = old_to_new_string_idx
                    .get(&name_idx)
                    .copied()
                    .expect("the attribute's name_idx should always match a value in the Pcf's string list");

                (name_idx, attribute)
            })
        }

        if self.version != from.version {
            return Err(MergeError::VersionMismatch(from.version, self.version));
        }

        let mut symbols = self.symbols;

        // The PCF format is based on DMX, so there are no guarantees that the strings list will be identical between
        // two PCF files. Its possible to have new strings, or strings that have changed position. So, we create a
        // map here to convert from incoming string index to merged string index.
        //
        // We also add any new strings from `other` into `self.strings` here.
        let mut old_to_new_string_idx = HashMap::new();
        for (from_idx, string) in from.symbols.base.into_iter().enumerate() {
            let (mapped_idx, _) = symbols.base.insert_full(string);
            old_to_new_string_idx.insert(from_idx as SymbolIdx, mapped_idx as SymbolIdx);
        }

        fn find_idx(from: &Symbols, value: &str) -> Option<SymbolIdx> {
            from.base
                .iter()
                .find_position(|el| *el == value)
                .map(|(idx, _)| idx as SymbolIdx)
        }

        symbols.particle_child = find_idx(&symbols, "DmeParticleChild");
        symbols.particle_operator = find_idx(&symbols, "DmeParticleOperator");
        symbols.function_name = find_idx(&symbols, "functionName");
        symbols.children = find_idx(&symbols, "children");
        symbols.constraints = find_idx(&symbols, "constraints");
        symbols.emitters = find_idx(&symbols, "emitters");
        symbols.forces = find_idx(&symbols, "forces");
        symbols.initializers = find_idx(&symbols, "initializers");
        symbols.operators = find_idx(&symbols, "operators");
        symbols.renderers = find_idx(&symbols, "renderers");
        symbols.child = find_idx(&symbols, "child");

        let mut root_attributes = self.root.attributes;
        for (name_idx, attribute) in from.root.attributes {
            let name_idx = old_to_new_string_idx
                .get(&name_idx)
                .copied()
                .expect("the attribute's name_idx should always match a value in the Pcf's string list");

            if root_attributes.contains_key(&name_idx) {
                continue;
            }

            root_attributes.insert(name_idx, attribute);
        }
        // root_attributes.extend(
        //     reindex_new_attributes(&old_to_new_string_idx, from.root.attributes),
        // );

        let mut particle_systems = Vec::from(self.root.particle_systems);
        let system_offset = particle_systems.len();

        for mut new_system in from.root.particle_systems {
            for child in &mut new_system.children {
                child.child += system_offset;
                child.attributes =
                    reindex_new_attributes(&old_to_new_string_idx, mem::take(&mut child.attributes)).collect();
            }

            for operator in &mut new_system.constraints {
                operator.attributes =
                    reindex_new_attributes(&old_to_new_string_idx, mem::take(&mut operator.attributes)).collect();
            }

            for operator in &mut new_system.emitters {
                operator.attributes =
                    reindex_new_attributes(&old_to_new_string_idx, mem::take(&mut operator.attributes)).collect();
            }

            for operator in &mut new_system.forces {
                operator.attributes =
                    reindex_new_attributes(&old_to_new_string_idx, mem::take(&mut operator.attributes)).collect();
            }

            for operator in &mut new_system.initializers {
                operator.attributes =
                    reindex_new_attributes(&old_to_new_string_idx, mem::take(&mut operator.attributes)).collect();
            }

            for operator in &mut new_system.operators {
                operator.attributes =
                    reindex_new_attributes(&old_to_new_string_idx, mem::take(&mut operator.attributes)).collect();
            }

            for operator in &mut new_system.renderers {
                operator.attributes =
                    reindex_new_attributes(&old_to_new_string_idx, mem::take(&mut operator.attributes)).collect();
            }

            new_system.attributes = reindex_new_attributes(&old_to_new_string_idx, new_system.attributes).collect();

            particle_systems.push(new_system);
        }

        let mut pcf = Self {
            version: self.version,
            symbols,
            root: Root {
                name: self.root.name,
                signature: self.root.signature,
                particle_systems: particle_systems.into_boxed_slice(),
                attributes: root_attributes,
            },
            encoded_size: 0,
        };

        pcf.encoded_size = pcf.compute_encoded_size();
        Ok(pcf)
    }

    fn compute_encoded_size(&self) -> usize {
        self.compute_encoded_version_size()
            + self.compute_encoded_symbols_size()
            + self.compute_encoded_elements_size()
            + self.compute_encoded_root_attributes_size()
            + self.compute_encoded_attributes_size()
    }

    pub fn compute_encoded_version_size(&self) -> usize {
        self.version.as_cstr_with_nul_terminator().to_bytes_with_nul().len()
    }

    pub fn compute_encoded_symbols_size(&self) -> usize {
        // symbols counter + strings with nul bytes
        size_of::<u16>() + self.symbols.base.len() + self.symbols.base.iter().map(|string| string.len()).sum::<usize>()
    }

    pub fn compute_encoded_elements_size(&self) -> usize {
        let mut elements_size = 0;

        // 32-bit element counter
        elements_size += size_of::<u32>();

        // the root is itself an element. In the first elements section we just have type idx, name, signature
        elements_size += size_of::<u16>() + self.root.name.len() + 1 + size_of::<Signature>();

        // do the same for each element across all of our particle systems
        for system in &self.root.particle_systems {
            elements_size += size_of::<u16>() + system.name.len() + 1 + size_of::<Signature>();
            for child in &system.children {
                elements_size += size_of::<u16>() + child.name.len() + 1 + size_of::<Signature>();
            }
            for operator in &system.constraints {
                elements_size += size_of::<u16>() + operator.name.len() + 1 + size_of::<Signature>();
            }
            for operator in &system.emitters {
                elements_size += size_of::<u16>() + operator.name.len() + 1 + size_of::<Signature>();
            }
            for operator in &system.forces {
                elements_size += size_of::<u16>() + operator.name.len() + 1 + size_of::<Signature>();
            }
            for operator in &system.initializers {
                elements_size += size_of::<u16>() + operator.name.len() + 1 + size_of::<Signature>();
            }
            for operator in &system.operators {
                elements_size += size_of::<u16>() + operator.name.len() + 1 + size_of::<Signature>();
            }
            for operator in &system.renderers {
                elements_size += size_of::<u16>() + operator.name.len() + 1 + size_of::<Signature>();
            }
        }

        elements_size
    }

    pub fn compute_encoded_root_attributes_size(&self) -> usize {
        let mut attributes_size = 0;

        // the root elements 32-bit attribute counter
        attributes_size += size_of::<u32>();

        // the root element's particle system definitions will become an attribute
        attributes_size += size_of::<SymbolIdx>()
            + size_of::<u8>()
            + size_of::<u32>()
            + (self.root.particle_systems.len() * size_of::<ElementIdx>());

        for (_, attribute) in &self.root.attributes {
            // the 16-bit name index
            attributes_size += size_of::<SymbolIdx>();
            // the 8-bit type index
            attributes_size += size_of::<u8>();
            // and the actual encoded size of each attribute
            attributes_size += attribute.get_encoded_size();
        }

        attributes_size
    }

    pub fn compute_encoded_attributes_size(&self) -> usize {
        let mut attributes_size = 0;

        // do the same for each element across all of our particle systems
        for system in &self.root.particle_systems {
            attributes_size += size_of::<u32>();
            for (_, attribute) in &system.attributes {
                attributes_size += size_of::<SymbolIdx>();
                attributes_size += size_of::<u8>();
                attributes_size += attribute.get_encoded_size();
            }

            if !system.children.is_empty() {
                attributes_size += size_of::<SymbolIdx>() + size_of::<u8>() + size_of::<u32>();
                attributes_size += system.children.len() * size_of::<ElementIdx>();
            }

            if !system.constraints.is_empty() {
                attributes_size += size_of::<SymbolIdx>() + size_of::<u8>() + size_of::<u32>();
                attributes_size += system.constraints.len() * size_of::<ElementIdx>();
            }

            if !system.emitters.is_empty() {
                attributes_size += size_of::<SymbolIdx>() + size_of::<u8>() + size_of::<u32>();
                attributes_size += system.emitters.len() * size_of::<ElementIdx>();
            }

            if !system.forces.is_empty() {
                attributes_size += size_of::<SymbolIdx>() + size_of::<u8>() + size_of::<u32>();
                attributes_size += system.forces.len() * size_of::<ElementIdx>();
            }

            if !system.initializers.is_empty() {
                attributes_size += size_of::<SymbolIdx>() + size_of::<u8>() + size_of::<u32>();
                attributes_size += system.initializers.len() * size_of::<ElementIdx>();
            }

            if !system.operators.is_empty() {
                attributes_size += size_of::<SymbolIdx>() + size_of::<u8>() + size_of::<u32>();
                attributes_size += system.operators.len() * size_of::<ElementIdx>();
            }

            if !system.renderers.is_empty() {
                attributes_size += size_of::<SymbolIdx>() + size_of::<u8>() + size_of::<u32>();
                attributes_size += system.renderers.len() * size_of::<ElementIdx>();
            }

            for child in &system.children {
                attributes_size += size_of::<u32>();
                // child.child will also become an attribute
                attributes_size += size_of::<SymbolIdx>() + size_of::<u8>() + size_of::<u32>();
                for (_, attribute) in &child.attributes {
                    attributes_size += size_of::<SymbolIdx>();
                    attributes_size += size_of::<u8>();
                    attributes_size += attribute.get_encoded_size();
                }
            }
            for operator in &system.constraints {
                attributes_size += size_of::<u32>();
                // function name will also become an attribute
                attributes_size += size_of::<SymbolIdx>() + size_of::<u8>() + 1 + operator.function_name.len();
                for (_, attribute) in &operator.attributes {
                    attributes_size += size_of::<SymbolIdx>();
                    attributes_size += size_of::<u8>();
                    attributes_size += attribute.get_encoded_size();
                }
            }
            for operator in &system.emitters {
                attributes_size += size_of::<u32>();
                attributes_size += size_of::<SymbolIdx>() + size_of::<u8>() + 1 + operator.function_name.len();
                for (_, attribute) in &operator.attributes {
                    attributes_size += size_of::<SymbolIdx>();
                    attributes_size += size_of::<u8>();
                    attributes_size += attribute.get_encoded_size();
                }
            }
            for operator in &system.forces {
                attributes_size += size_of::<u32>();
                attributes_size += size_of::<SymbolIdx>() + size_of::<u8>() + 1 + operator.function_name.len();
                for (_, attribute) in &operator.attributes {
                    attributes_size += size_of::<SymbolIdx>();
                    attributes_size += size_of::<u8>();
                    attributes_size += attribute.get_encoded_size();
                }
            }
            for operator in &system.initializers {
                attributes_size += size_of::<u32>();
                attributes_size += size_of::<SymbolIdx>() + size_of::<u8>() + 1 + operator.function_name.len();
                for (_, attribute) in &operator.attributes {
                    attributes_size += size_of::<SymbolIdx>();
                    attributes_size += size_of::<u8>();
                    attributes_size += attribute.get_encoded_size();
                }
            }
            for operator in &system.operators {
                attributes_size += size_of::<u32>();
                attributes_size += size_of::<SymbolIdx>() + size_of::<u8>() + 1 + operator.function_name.len();
                for (_, attribute) in &operator.attributes {
                    attributes_size += size_of::<SymbolIdx>();
                    attributes_size += size_of::<u8>();
                    attributes_size += attribute.get_encoded_size();
                }
            }
            for operator in &system.renderers {
                attributes_size += size_of::<u32>();
                attributes_size += size_of::<SymbolIdx>() + size_of::<u8>() + 1 + operator.function_name.len();
                for (_, attribute) in &operator.attributes {
                    attributes_size += size_of::<SymbolIdx>();
                    attributes_size += size_of::<u8>();
                    attributes_size += attribute.get_encoded_size();
                }
            }
        }

        attributes_size
    }

    pub fn compute_merged_size(&self, from: &Self) -> usize {
        self.compute_encoded_version_size()
            + self.compute_encoded_symbols_size_after_merge(from)
            + self.compute_encoded_elements_size_after_merge(from)
            + self.compute_encoded_root_attributes_size_after_merge(from)
            + self.compute_encoded_attributes_size_after_merge(from)
    }

    pub fn compute_encoded_symbols_size_after_merge(&self, from: &Self) -> usize {
        // size of all new symbols + a nul byte for each symbol, because these are encoded as c-strings
        let new_symbols_size = from
            .symbols
            .base
            .iter()
            .filter(|symbol| !self.symbols.base.contains(*symbol))
            .map(|symbol| symbol.len() + 1)
            .sum::<usize>();

        self.compute_encoded_symbols_size() + new_symbols_size
    }

    pub fn compute_encoded_elements_size_after_merge(&self, from: &Self) -> usize {
        let mut elements_size = 0;

        // N.B. we dont add element counter or root element size because they are already accounted for later, when we
        // add compute_encoded_size() to our result

        // do the same for each element across all of our particle systems
        for system in &from.root.particle_systems {
            elements_size += size_of::<u16>() + system.name.len() + 1 + size_of::<Signature>();
            for child in &system.children {
                elements_size += size_of::<u16>() + child.name.len() + 1 + size_of::<Signature>();
            }
            for operator in &system.constraints {
                elements_size += size_of::<u16>() + operator.name.len() + 1 + size_of::<Signature>();
            }
            for operator in &system.emitters {
                elements_size += size_of::<u16>() + operator.name.len() + 1 + size_of::<Signature>();
            }
            for operator in &system.forces {
                elements_size += size_of::<u16>() + operator.name.len() + 1 + size_of::<Signature>();
            }
            for operator in &system.initializers {
                elements_size += size_of::<u16>() + operator.name.len() + 1 + size_of::<Signature>();
            }
            for operator in &system.operators {
                elements_size += size_of::<u16>() + operator.name.len() + 1 + size_of::<Signature>();
            }
            for operator in &system.renderers {
                elements_size += size_of::<u16>() + operator.name.len() + 1 + size_of::<Signature>();
            }
        }

        self.compute_encoded_elements_size() + elements_size
    }

    pub fn compute_encoded_root_attributes_size_after_merge(&self, from: &Self) -> usize {
        let mut from_to_self_idx = HashMap::new();
        for (from_idx, name) in from.symbols.base.iter().enumerate() {
            if let Some(self_idx) = self.symbols.base.get_index_of(name) {
                from_to_self_idx.insert(from_idx as SymbolIdx, self_idx as SymbolIdx);
            }
        }

        let mut attributes_size = 0;

        // the root element's particle system definitions will become an attribute
        attributes_size += from.root.particle_systems.len() * size_of::<ElementIdx>();

        for (name_idx, attribute) in &from.root.attributes {
            // only include attributes from `from` that don't already exist in `self`
            if let Some(name_idx) = from_to_self_idx.get(name_idx)
                && self.root.attributes.contains_key(name_idx)
            {
                continue;
            }

            // the 16-bit name index
            attributes_size += size_of::<SymbolIdx>();
            // the 8-bit type index
            attributes_size += size_of::<u8>();
            // and the actual encoded size of each attribute
            attributes_size += attribute.get_encoded_size();
        }

        self.compute_encoded_root_attributes_size() + attributes_size
    }

    pub fn compute_encoded_attributes_size_after_merge(&self, from: &Self) -> usize {
        let mut attributes_size = 0;

        // do the same for each element across all of our particle systems
        for system in &from.root.particle_systems {
            attributes_size += size_of::<u32>();
            for (_, attribute) in &system.attributes {
                attributes_size += size_of::<SymbolIdx>();
                attributes_size += size_of::<u8>();
                attributes_size += attribute.get_encoded_size();
            }

            if !system.children.is_empty() {
                attributes_size += size_of::<SymbolIdx>() + size_of::<u8>() + size_of::<u32>();
                attributes_size += system.children.len() * size_of::<ElementIdx>();
            }

            if !system.constraints.is_empty() {
                attributes_size += size_of::<SymbolIdx>() + size_of::<u8>() + size_of::<u32>();
                attributes_size += system.constraints.len() * size_of::<ElementIdx>();
            }

            if !system.emitters.is_empty() {
                attributes_size += size_of::<SymbolIdx>() + size_of::<u8>() + size_of::<u32>();
                attributes_size += system.emitters.len() * size_of::<ElementIdx>();
            }

            if !system.forces.is_empty() {
                attributes_size += size_of::<SymbolIdx>() + size_of::<u8>() + size_of::<u32>();
                attributes_size += system.forces.len() * size_of::<ElementIdx>();
            }

            if !system.initializers.is_empty() {
                attributes_size += size_of::<SymbolIdx>() + size_of::<u8>() + size_of::<u32>();
                attributes_size += system.initializers.len() * size_of::<ElementIdx>();
            }

            if !system.operators.is_empty() {
                attributes_size += size_of::<SymbolIdx>() + size_of::<u8>() + size_of::<u32>();
                attributes_size += system.operators.len() * size_of::<ElementIdx>();
            }

            if !system.renderers.is_empty() {
                attributes_size += size_of::<SymbolIdx>() + size_of::<u8>() + size_of::<u32>();
                attributes_size += system.renderers.len() * size_of::<ElementIdx>();
            }

            for child in &system.children {
                attributes_size += size_of::<u32>();
                // child.child will also become an attribute
                attributes_size += size_of::<SymbolIdx>() + size_of::<u8>() + size_of::<u32>();
                for (_, attribute) in &child.attributes {
                    attributes_size += size_of::<SymbolIdx>();
                    attributes_size += size_of::<u8>();
                    attributes_size += attribute.get_encoded_size();
                }
            }
            for operator in &system.constraints {
                attributes_size += size_of::<u32>();
                // function name will also become an attribute
                attributes_size += size_of::<SymbolIdx>() + size_of::<u8>() + 1 + operator.function_name.len();
                for (_, attribute) in &operator.attributes {
                    attributes_size += size_of::<SymbolIdx>();
                    attributes_size += size_of::<u8>();
                    attributes_size += attribute.get_encoded_size();
                }
            }
            for operator in &system.emitters {
                attributes_size += size_of::<u32>();
                attributes_size += size_of::<SymbolIdx>() + size_of::<u8>() + 1 + operator.function_name.len();
                for (_, attribute) in &operator.attributes {
                    attributes_size += size_of::<SymbolIdx>();
                    attributes_size += size_of::<u8>();
                    attributes_size += attribute.get_encoded_size();
                }
            }
            for operator in &system.forces {
                attributes_size += size_of::<u32>();
                attributes_size += size_of::<SymbolIdx>() + size_of::<u8>() + 1 + operator.function_name.len();
                for (_, attribute) in &operator.attributes {
                    attributes_size += size_of::<SymbolIdx>();
                    attributes_size += size_of::<u8>();
                    attributes_size += attribute.get_encoded_size();
                }
            }
            for operator in &system.initializers {
                attributes_size += size_of::<u32>();
                attributes_size += size_of::<SymbolIdx>() + size_of::<u8>() + 1 + operator.function_name.len();
                for (_, attribute) in &operator.attributes {
                    attributes_size += size_of::<SymbolIdx>();
                    attributes_size += size_of::<u8>();
                    attributes_size += attribute.get_encoded_size();
                }
            }
            for operator in &system.operators {
                attributes_size += size_of::<u32>();
                attributes_size += size_of::<SymbolIdx>() + size_of::<u8>() + 1 + operator.function_name.len();
                for (_, attribute) in &operator.attributes {
                    attributes_size += size_of::<SymbolIdx>();
                    attributes_size += size_of::<u8>();
                    attributes_size += attribute.get_encoded_size();
                }
            }
            for operator in &system.renderers {
                attributes_size += size_of::<u32>();
                attributes_size += size_of::<SymbolIdx>() + size_of::<u8>() + 1 + operator.function_name.len();
                for (_, attribute) in &operator.attributes {
                    attributes_size += size_of::<SymbolIdx>();
                    attributes_size += size_of::<u8>();
                    attributes_size += attribute.get_encoded_size();
                }
            }
        }

        self.compute_encoded_attributes_size() + attributes_size
    }

    /// Consumes the [`Pcf`], splitting it up into multiple [`Pcf`]s. Each [`Pcf`] will only contain
    /// [`ParticleSystem`]s that are connected (such as by a via [`Child`] link).
    ///
    /// Each [`ParticleSystem`] from the original [`Pcf`] will only show up once.
    ///
    /// Unused symbols are also stripped from each [`Pcf`], to ensure it only contains the bare minimum necessary to be
    /// a valid [`Pcf`].
    pub fn into_connected(mut self) -> Vec<Self> {
        // fn bfs(graph: &HashMap<ElementIdx, Vec<ElementIdx>>) -> Vec<Vec<ElementIdx>> {
        //     let mut visited = OrderSet::new();
        //     let mut components = Vec::new();

        //     for start in graph.keys() {
        //         if !visited.insert(*start) {
        //             continue;
        //         }

        //         let mut component = Vec::new();
        //         let mut queue = VecDeque::from([*start]);

        //         while let Some(value) = queue.pop_front() {
        //             component.push(value);
        //             for child in graph.get(&value).unwrap() {
        //                 if visited.insert(*child) {
        //                     queue.push_back(*child);
        //                 }
        //             }
        //         }

        //         components.push(component);
        //     }

        //     components
        // }

        let mut graph: UnGraphMap<ElementIdx, ()> = UnGraphMap::new();
        for (system_idx, _) in self.root.particle_systems.iter().enumerate() {
            graph.add_node(system_idx.into());
        }

        for (system_idx, particle_system) in self.root.particle_systems.iter().enumerate() {
            for child in particle_system.children.iter() {
                graph.add_edge(system_idx.into(), child.child, ());
            }
        }

        let mut components = tarjan_scc(&graph);

        // tarjan_scc results in reversed element groups, so we unreverse it here
        components.reverse();
        let mut groups = Vec::new();
        for mut component in components {
            // tarjan_scc results in reversed element indices in each group, so we unreverse it here
            component.reverse();

            let old_to_new_idx: HashMap<_, _> = component
                .iter()
                .enumerate()
                .map(|(new_idx, old_idx)| (*old_idx, ElementIdx::from(new_idx)))
                .collect();

            let mut group = Vec::new();
            for system_idx in component {
                let mut system = mem::take(&mut self.root.particle_systems[usize::from(system_idx)]);
                for child in &mut system.children {
                    child.child = old_to_new_idx[&child.child];
                }

                group.push(system);
            }

            groups.push(group);
        }

        groups
            .into_iter()
            .map(|group| {
                Self {
                    version: self.version,
                    symbols: self.symbols.clone(),
                    root: Root {
                        name: self.root.name.clone(),
                        signature: self.root.signature,
                        particle_systems: group.into_iter().collect(),
                        attributes: self.root.attributes.clone(),
                    },
                    encoded_size: 0,
                }
                .unused_symbols_stripped()
            })
            .collect()
    }

    /// Consumes the [`Pcf`], returning a new [`Pcf`] with all unused symbols removed. References to symbols are
    /// replaced with the new index for each symbol.
    pub fn unused_symbols_stripped(mut self) -> Self {
        // these symbols are always required
        let mut used_symbols = HashSet::from([
            self.symbols.element,
            self.symbols.particle_system_definitions,
            self.symbols.particle_system_definition,
        ]);

        for (name_idx, _) in &self.root.attributes {
            used_symbols.insert(*name_idx);
        }

        let mut has_child = false;
        let mut has_constraint = false;
        let mut has_emitter = false;
        let mut has_force = false;
        let mut has_initializer = false;
        let mut has_operator = false;
        let mut has_renderer = false;

        for system in &self.root.particle_systems {
            for (name_idx, _) in &system.attributes {
                used_symbols.insert(*name_idx);
            }

            if !system.children.is_empty() {
                has_child = true;
                for child in &system.children {
                    for (name_idx, _) in &child.attributes {
                        used_symbols.insert(*name_idx);
                    }
                }
            }

            if !system.constraints.is_empty() {
                has_constraint = true;
                for operator in &system.constraints {
                    for (name_idx, _) in &operator.attributes {
                        used_symbols.insert(*name_idx);
                    }
                }
            }

            if !system.emitters.is_empty() {
                has_emitter = true;
                for operator in &system.emitters {
                    for (name_idx, _) in &operator.attributes {
                        used_symbols.insert(*name_idx);
                    }
                }
            }

            if !system.forces.is_empty() {
                has_force = true;
                for operator in &system.forces {
                    for (name_idx, _) in &operator.attributes {
                        used_symbols.insert(*name_idx);
                    }
                }
            }

            if !system.initializers.is_empty() {
                has_initializer = true;
                for operator in &system.initializers {
                    for (name_idx, _) in &operator.attributes {
                        used_symbols.insert(*name_idx);
                    }
                }
            }

            if !system.operators.is_empty() {
                has_operator = true;
                for operator in &system.operators {
                    for (name_idx, _) in &operator.attributes {
                        used_symbols.insert(*name_idx);
                    }
                }
            }

            if !system.renderers.is_empty() {
                has_renderer = true;
                for operator in &system.renderers {
                    for (name_idx, _) in &operator.attributes {
                        used_symbols.insert(*name_idx);
                    }
                }
            }
        }

        if has_child {
            used_symbols.insert(
                self.symbols
                    .child
                    .expect("the child symbol index is unassigned despite the Pcf having a child"),
            );
            used_symbols.insert(
                self.symbols
                    .particle_child
                    .expect("the particle child symbol index is unassigned despite the Pcf having a child"),
            );
            used_symbols.insert(
                self.symbols
                    .children
                    .expect("the children symbol index is unassigned despite the Pcf having a child"),
            );
        }

        if has_constraint || has_emitter || has_force || has_initializer || has_operator || has_renderer {
            used_symbols.insert(
                self.symbols
                    .particle_operator
                    .expect("the particle operator symbol index is unassigned despite the Pcf having an operator"),
            );
            used_symbols.insert(
                self.symbols
                    .function_name
                    .expect("the function name symbol index is unassigned despite the Pcf having an operator"),
            );
        }

        if has_constraint {
            used_symbols.insert(
                self.symbols
                    .constraints
                    .expect("the constraints symbol index is unassigned despite the Pcf having a constraints"),
            );
        }

        if has_emitter {
            used_symbols.insert(
                self.symbols
                    .emitters
                    .expect("the emitters symbol index is unassigned despite the Pcf having a emitters"),
            );
        }

        if has_emitter {
            used_symbols.insert(
                self.symbols
                    .forces
                    .expect("the forces symbol index is unassigned despite the Pcf having a forces"),
            );
        }

        if has_emitter {
            used_symbols.insert(
                self.symbols
                    .initializers
                    .expect("the initializers symbol index is unassigned despite the Pcf having a initializers"),
            );
        }

        if has_emitter {
            used_symbols.insert(
                self.symbols
                    .operators
                    .expect("the operators symbol index is unassigned despite the Pcf having a operators"),
            );
        }

        if has_emitter {
            used_symbols.insert(
                self.symbols
                    .renderers
                    .expect("the renderers symbol index is unassigned despite the Pcf having a renderers"),
            );
        }

        let old_symbols = mem::replace(&mut self.symbols.base, OrderSet::new());

        let mut old_to_new_idx: HashMap<SymbolIdx, SymbolIdx> = HashMap::new();
        let mut running_offset = 0;
        for (idx, symbol) in old_symbols.into_iter().enumerate() {
            let idx = idx as SymbolIdx;
            if !used_symbols.contains(&idx) {
                running_offset += 1;
                continue;
            }

            old_to_new_idx.insert(idx, idx - running_offset);
            self.symbols.base.insert(symbol);
        }

        fn remap_attributes(
            old_to_new_idx: &HashMap<u16, u16>,
            attributes: OrderMap<SymbolIdx, Attribute>,
        ) -> OrderMap<SymbolIdx, Attribute> {
            attributes
                .into_iter()
                .map(|(name_idx, attribute)| {
                    let new_name_idx = *old_to_new_idx
                        .get(&name_idx)
                        .expect("old name indices should always be present in the map");

                    (new_name_idx, attribute)
                })
                .collect()
        }

        fn remap_operators(old_to_new_idx: &HashMap<u16, u16>, operators: &mut Box<[Operator]>) {
            for operator in operators {
                let attributes = mem::take(&mut operator.attributes);
                operator.attributes = remap_attributes(old_to_new_idx, attributes);
            }
        }

        self.root.attributes = remap_attributes(&old_to_new_idx, self.root.attributes);
        self.root.particle_systems = self
            .root
            .particle_systems
            .into_iter()
            .map(|mut particle_system| {
                particle_system.attributes = remap_attributes(&old_to_new_idx, particle_system.attributes);

                particle_system.children = particle_system
                    .children
                    .into_iter()
                    .map(|mut child| {
                        child.attributes = remap_attributes(&old_to_new_idx, child.attributes);
                        child
                    })
                    .collect();

                remap_operators(&old_to_new_idx, &mut particle_system.constraints);
                remap_operators(&old_to_new_idx, &mut particle_system.emitters);
                remap_operators(&old_to_new_idx, &mut particle_system.forces);
                remap_operators(&old_to_new_idx, &mut particle_system.initializers);
                remap_operators(&old_to_new_idx, &mut particle_system.renderers);
                remap_operators(&old_to_new_idx, &mut particle_system.operators);

                particle_system
            })
            .collect();

        self.symbols.element = *old_to_new_idx
            .get(&self.symbols.element)
            .expect("this should always be present in the map");
        self.symbols.particle_system_definitions = *old_to_new_idx
            .get(&self.symbols.particle_system_definitions)
            .expect("this should always be present in the map");
        self.symbols.particle_system_definition = *old_to_new_idx
            .get(&self.symbols.particle_system_definition)
            .expect("this should always be present in the map");
        self.symbols.particle_child = self
            .symbols
            .particle_child
            .and_then(|idx| old_to_new_idx.get(&idx).cloned());
        self.symbols.particle_operator = self
            .symbols
            .particle_operator
            .and_then(|idx| old_to_new_idx.get(&idx).cloned());
        self.symbols.function_name = self
            .symbols
            .function_name
            .and_then(|idx| old_to_new_idx.get(&idx).cloned());
        self.symbols.children = self.symbols.children.and_then(|idx| old_to_new_idx.get(&idx).cloned());
        self.symbols.constraints = self
            .symbols
            .constraints
            .and_then(|idx| old_to_new_idx.get(&idx).cloned());
        self.symbols.emitters = self.symbols.emitters.and_then(|idx| old_to_new_idx.get(&idx).cloned());
        self.symbols.forces = self.symbols.forces.and_then(|idx| old_to_new_idx.get(&idx).cloned());
        self.symbols.initializers = self
            .symbols
            .initializers
            .and_then(|idx| old_to_new_idx.get(&idx).cloned());
        self.symbols.operators = self.symbols.operators.and_then(|idx| old_to_new_idx.get(&idx).cloned());
        self.symbols.renderers = self.symbols.renderers.and_then(|idx| old_to_new_idx.get(&idx).cloned());
        self.symbols.child = self.symbols.child.and_then(|idx| old_to_new_idx.get(&idx).cloned());

        self.encoded_size = self.compute_encoded_size();
        self
    }

    pub fn defaults_stripped_nth(
        mut self,
        to: usize,
        particle_defaults: &HashMap<&str, Attribute>,
        operator_defaults: &HashMap<&str, Attribute>,
    ) -> Self {
        fn remove_operator_defaults(op: &mut Operator, defaults: &HashMap<SymbolIdx, &Attribute>) {
            op.attributes = mem::take(&mut op.attributes)
                .into_iter()
                .filter(|(name_idx, attribute)| {
                    if let Some(default) = defaults.get(name_idx)
                        && attribute == *default
                    {
                        false
                    } else {
                        true
                    }
                })
                .collect();
        }

        let particle_defaults: HashMap<_, _> = particle_defaults
            .iter()
            .filter_map(|(name, value)| {
                self.symbols
                    .base
                    .iter()
                    .position(|s| s == name)
                    .map(|idx| (idx as SymbolIdx, value))
            })
            .collect();

        let operator_defaults: HashMap<_, _> = operator_defaults
            .iter()
            .filter_map(|(attribute_name, attribute)| {
                let name_idx = self.symbols.base.get_index_of(*attribute_name)? as SymbolIdx;
                Some((name_idx, attribute))
            })
            .collect();

        for (idx, system) in self.root.particle_systems.iter_mut().enumerate() {
            if idx >= to {
                break;
            }

            system.attributes = mem::take(&mut system.attributes)
                .into_iter()
                .filter(|(name_idx, attribute)| {
                    if let Some(default) = particle_defaults.get(name_idx)
                        && attribute == *default
                    {
                        false
                    } else {
                        true
                    }
                })
                .collect();

            system
                .constraints
                .iter_mut()
                .for_each(|op| remove_operator_defaults(op, &operator_defaults));
            system
                .emitters
                .iter_mut()
                .for_each(|op| remove_operator_defaults(op, &operator_defaults));
            system
                .forces
                .iter_mut()
                .for_each(|op| remove_operator_defaults(op, &operator_defaults));
            system
                .initializers
                .iter_mut()
                .for_each(|op| remove_operator_defaults(op, &operator_defaults));
            system
                .operators
                .iter_mut()
                .for_each(|op| remove_operator_defaults(op, &operator_defaults));
            system
                .renderers
                .iter_mut()
                .for_each(|op| remove_operator_defaults(op, &operator_defaults));
        }

        self
    }

    pub fn defaults_stripped(
        mut self,
        particle_defaults: &HashMap<&str, Attribute>,
        operator_defaults: &HashMap<String, HashMap<String, Attribute>>,
    ) -> Self {
        fn remove_operator_defaults(op: &mut Operator, defaults: &HashMap<&String, HashMap<SymbolIdx, &Attribute>>) {
            if let Some(defaults) = defaults.get(&op.function_name) {
                op.attributes = mem::take(&mut op.attributes)
                    .into_iter()
                    .filter(|(name_idx, attribute)| {
                        if let Some(default) = defaults.get(name_idx)
                            && attribute == *default
                        {
                            false
                        } else {
                            true
                        }
                    })
                    .collect();
            }
        }

        let particle_defaults: HashMap<_, _> = particle_defaults
            .iter()
            .filter_map(|(name, value)| {
                self.symbols
                    .base
                    .iter()
                    .position(|s| s == name)
                    .map(|idx| (idx as SymbolIdx, value))
            })
            .collect();

        let operator_defaults: HashMap<_, _> = operator_defaults
            .iter()
            .map(|(function_name, defaults)| {
                let map: HashMap<_, _> = defaults
                    .iter()
                    .filter_map(|(attribute_name, attribute)| {
                        let name_idx = self.symbols.base.get_index_of(attribute_name)? as SymbolIdx;
                        Some((name_idx, attribute))
                    })
                    .collect();

                (function_name, map)
            })
            .collect();

        for system in &mut self.root.particle_systems {
            system.attributes = mem::take(&mut system.attributes)
                .into_iter()
                .filter(|(name_idx, attribute)| {
                    if let Some(default) = particle_defaults.get(name_idx)
                        && attribute == *default
                    {
                        false
                    } else {
                        true
                    }
                })
                .collect();

            system
                .constraints
                .iter_mut()
                .for_each(|op| remove_operator_defaults(op, &operator_defaults));
            system
                .emitters
                .iter_mut()
                .for_each(|op| remove_operator_defaults(op, &operator_defaults));
            system
                .forces
                .iter_mut()
                .for_each(|op| remove_operator_defaults(op, &operator_defaults));
            system
                .initializers
                .iter_mut()
                .for_each(|op| remove_operator_defaults(op, &operator_defaults));
            system
                .operators
                .iter_mut()
                .for_each(|op| remove_operator_defaults(op, &operator_defaults));
            system
                .renderers
                .iter_mut()
                .for_each(|op| remove_operator_defaults(op, &operator_defaults));
        }

        self
    }

    pub fn encoded_size(&self) -> usize {
        self.encoded_size
    }

    pub fn into_parts(self) -> (Version, Symbols, Root) {
        (self.version, self.symbols, self.root)
    }

    pub fn new_empty_from(other: &Self) -> Self {
        let mut pcf = Self {
            version: other.version,
            symbols: Symbols::default(),
            root: Root {
                name: other.root.name.clone(),
                signature: other.root.signature,
                particle_systems: Vec::new().into_boxed_slice(),
                attributes: OrderMap::new(),
            },
            encoded_size: 0,
        };

        pcf.encoded_size = pcf.compute_encoded_size();
        pcf
    }

    pub fn particle_systems(&self) -> &[ParticleSystem] {
        &self.root.particle_systems
    }
}

impl TryFrom<Dmx> for Pcf {
    type Error = Error;

    fn try_from(value: Dmx) -> Result<Self, Self::Error> {
        let symbols: Symbols = value.strings.try_into()?;

        let root_element = value.elements.first().ok_or(Error::NoElements)?;
        let Some(dmx::attribute::Attribute::ElementArray(system_indices)) =
            root_element.attributes.get(&symbols.particle_system_definitions)
        else {
            return Err(Error::MissingRootDefintions);
        };

        // `particle_systems` will contain each particle system in the same order defined in `system_indices`; if we
        // didn't map old indices to new indices, we'd have to do a second pass over each particle system after each
        // index is known in order to map the old child element indices. This lets us avoid the second pass entirely.
        let system_indices: OrderMap<_, _> = system_indices
            .iter()
            .enumerate()
            .map(|(new_idx, old_idx)| (*old_idx, ElementIdx::from(new_idx)))
            .collect();

        // the elements list is an association list for a Directed Acyclic Graph.
        // there is always at least a root element, usually named "untitled", which always has an attribute named
        // "particleSystemDefinitions" - an array containing indices into the elements list. Each of these indices
        // will always be a DmeParticleSystemDefinition.
        //
        // Each DmeParticleSystemDefinition element can contains a handful of attributes, one of these attributes
        // - "children" - is also an element array containing indices into the elements list. These indices will point
        // to DmeParticleChild elements. DmeParticleChild always have a "child" attribute whose value is a single element
        // index; the referenced child element will always be another DmeParticleSystemDefinition.
        //
        // Some other DmeParticleSystemDefinition attributes can also contain element references; but, they will always
        // be references to DmeParticleOperator elements. DmeParticleOperator are always leaf nodes in the DAG.

        let mut particle_systems: Vec<ParticleSystem> = Vec::new();

        for system_idx in system_indices.keys() {
            let element = value
                .elements
                .get(usize::from(*system_idx))
                .ok_or(Error::MissingParticleSystem(*system_idx))?;

            if element.type_idx != symbols.particle_system_definition {
                return Err(Error::InvalidParticleSystem(*system_idx));
            }

            let name = element.name.to_string_lossy().into_owned();
            let signature = element.signature;

            let mut children: Vec<Child> = Vec::new();
            let mut constraints: Vec<Operator> = Vec::new();
            let mut emitters: Vec<Operator> = Vec::new();
            let mut forces: Vec<Operator> = Vec::new();
            let mut initializers: Vec<Operator> = Vec::new();
            let mut operators: Vec<Operator> = Vec::new();
            let mut renderers: Vec<Operator> = Vec::new();
            let mut attributes = OrderMap::new();

            for (name_idx, attribute) in &element.attributes {
                if let dmx::attribute::Attribute::ElementArray(element_indices) = attribute {
                    if symbols.children.is_some_and(|idx| *name_idx == idx) {
                        for child_element_idx in element_indices {
                            let child_element = value
                                .elements
                                .get(usize::from(*child_element_idx))
                                .ok_or(Error::MissingParticleChild(*child_element_idx))?;

                            if symbols.particle_child.is_none_or(|idx| child_element.type_idx != idx) {
                                return Err(Error::InvalidParticleChild(*child_element_idx));
                            }

                            let child_attribute = symbols
                                .child
                                .and_then(|idx| child_element.attributes.get(&idx))
                                .ok_or(Error::MissingChild)?;
                            let dmx::attribute::Attribute::Element(child_system_idx) = child_attribute else {
                                return Err(Error::MissingChild);
                            };

                            if !child_system_idx.is_valid() {
                                continue;
                            }

                            let mut attributes = OrderMap::new();
                            for (name_idx, attribute) in &child_element.attributes {
                                if symbols.child.is_some_and(|idx| *name_idx == idx) {
                                    continue;
                                }

                                attributes.insert(*name_idx, attribute.clone().try_into()?);
                            }

                            let name = child_element.name.to_string_lossy().into_owned();
                            let signature = child_element.signature;
                            let child = *system_indices
                                .get(child_system_idx)
                                .expect("this relationship should always be valid");
                            children.push(Child {
                                name,
                                signature,
                                attributes,
                                child,
                            });
                        }
                        continue;
                    }

                    let dme_operators = if symbols.constraints.is_some_and(|idx| *name_idx == idx) {
                        &mut constraints
                    } else if symbols.emitters.is_some_and(|idx| *name_idx == idx) {
                        &mut emitters
                    } else if symbols.forces.is_some_and(|idx| *name_idx == idx) {
                        &mut forces
                    } else if symbols.initializers.is_some_and(|idx| *name_idx == idx) {
                        &mut initializers
                    } else if symbols.operators.is_some_and(|idx| *name_idx == idx) {
                        &mut operators
                    } else if symbols.renderers.is_some_and(|idx| *name_idx == idx) {
                        &mut renderers
                    } else {
                        return Err(Error::UnexpectedElementReference);
                    };

                    for element_idx in element_indices {
                        let element = value
                            .elements
                            .get(usize::from(*element_idx))
                            .ok_or(Error::MissingOperator(*element_idx))?;

                        if symbols.particle_operator.is_none_or(|idx| element.type_idx != idx) {
                            return Err(Error::InvalidParticleOperator(*element_idx));
                        }

                        dme_operators.push(Operator::try_from(element, &symbols)?);
                    }
                } else {
                    attributes.insert(*name_idx, attribute.clone().try_into()?);
                }
            }

            particle_systems.push(ParticleSystem {
                name,
                signature,
                children: children.into_boxed_slice(),
                constraints: constraints.into_boxed_slice(),
                emitters: emitters.into_boxed_slice(),
                forces: forces.into_boxed_slice(),
                initializers: initializers.into_boxed_slice(),
                operators: operators.into_boxed_slice(),
                renderers: renderers.into_boxed_slice(),
                attributes,
            });
        }

        let mut attributes = OrderMap::new();
        for (name_idx, attribute) in &root_element.attributes {
            if *name_idx == symbols.particle_system_definitions {
                continue;
            }

            attributes.insert(*name_idx, attribute.clone().try_into()?);
        }

        let root = Root {
            name: root_element.name.to_string_lossy().into_owned(),
            signature: root_element.signature,
            particle_systems: particle_systems.into_boxed_slice(),
            attributes,
        };

        let mut pcf = Self {
            version: value.version,
            symbols,
            root,
            encoded_size: 0,
        };

        pcf.encoded_size = pcf.compute_encoded_size();
        Ok(pcf)
    }
}

impl From<Pcf> for Dmx {
    fn from(pcf: Pcf) -> Self {
        fn push_operators(
            operators: Box<[Operator]>,
            elements: &mut Vec<Element>,
            indices: &mut Vec<ElementIdx>,
            symbols: &Symbols,
        ) {
            if !operators.is_empty() {
                let function_name_idx = symbols
                    .function_name
                    .expect("function name symbol idx not set despite having operators in dmx");
                let particle_operator_idx = symbols
                    .particle_operator
                    .expect("particle operator symbol idx not set despite having operators in dmx");
                for operator in operators {
                    let mut attributes = attribute_map_to_dmx_map(operator.attributes);
                    attributes.insert(function_name_idx, string_to_cstring(operator.function_name).into());

                    indices.push(ElementIdx::from(elements.len()));
                    elements.push(Element {
                        type_idx: particle_operator_idx,
                        name: string_to_cstring(operator.name),
                        signature: operator.signature,
                        attributes,
                    })
                }
            }
        }

        fn push_index_attribute(
            indices: Vec<ElementIdx>,
            key: Option<SymbolIdx>,
            attributes: &mut OrderMap<SymbolIdx, dmx::attribute::Attribute>,
        ) {
            if !indices.is_empty() {
                attributes.insert(
                    key.expect("key should be set if the indices vec is not empty"),
                    indices.into_boxed_slice().into(),
                );
            }
        }

        let mut root_attributes = attribute_map_to_dmx_map(pcf.root.attributes);
        let particle_system_definitions: Box<_> = (1..=pcf.root.particle_systems.len()).map(ElementIdx::from).collect();

        root_attributes.insert(
            pcf.symbols.particle_system_definitions,
            particle_system_definitions.into(),
        );

        let root_element = Element {
            type_idx: pcf.symbols.element,
            name: string_to_cstring(pcf.root.name),
            signature: pcf.root.signature,
            attributes: root_attributes,
        };

        let mut elements = vec![root_element];
        for particle_system in &pcf.root.particle_systems {
            elements.push(Element {
                type_idx: pcf.symbols.particle_system_definition,
                name: str_to_cstring(&particle_system.name),
                signature: particle_system.signature,
                attributes: OrderMap::new(),
            })
        }

        for (system_idx, particle_system) in pcf.root.particle_systems.into_iter().enumerate() {
            let mut child_indices = Vec::new();
            let mut constraint_indices = Vec::new();
            let mut emitter_indices = Vec::new();
            let mut force_indices = Vec::new();
            let mut initializer_indices = Vec::new();
            let mut operator_indices = Vec::new();
            let mut renderer_indices = Vec::new();

            if !particle_system.children.is_empty() {
                let child_idx = pcf
                    .symbols
                    .child
                    .expect("particle child symbol idx not set despite having children in dmx");
                let particle_child_idx = pcf
                    .symbols
                    .particle_child
                    .expect("particle child symbol idx not set despite having children in dmx");
                for child in particle_system.children {
                    child_indices.push(ElementIdx::from(elements.len()));

                    let mut attributes = attribute_map_to_dmx_map(child.attributes);

                    // child.child indexes into value.root.particle_systems, but we insert particle system defintiions
                    // into `elements` before any others, so this index is still correct in our new elements list
                    // except we have to offset by 1 to account for the root element we add earlier.
                    attributes.insert(child_idx, dmx::attribute::Attribute::Element(child.child + 1));

                    elements.push(Element {
                        type_idx: particle_child_idx,
                        name: string_to_cstring(child.name),
                        signature: child.signature,
                        attributes,
                    })
                }
            }

            push_operators(
                particle_system.constraints,
                &mut elements,
                &mut constraint_indices,
                &pcf.symbols,
            );
            push_operators(
                particle_system.emitters,
                &mut elements,
                &mut emitter_indices,
                &pcf.symbols,
            );
            push_operators(particle_system.forces, &mut elements, &mut force_indices, &pcf.symbols);
            push_operators(
                particle_system.initializers,
                &mut elements,
                &mut initializer_indices,
                &pcf.symbols,
            );
            push_operators(
                particle_system.operators,
                &mut elements,
                &mut operator_indices,
                &pcf.symbols,
            );
            push_operators(
                particle_system.renderers,
                &mut elements,
                &mut renderer_indices,
                &pcf.symbols,
            );

            let mut new_attributes = attribute_map_to_dmx_map(particle_system.attributes);

            push_index_attribute(child_indices, pcf.symbols.children, &mut new_attributes);
            push_index_attribute(constraint_indices, pcf.symbols.constraints, &mut new_attributes);
            push_index_attribute(emitter_indices, pcf.symbols.emitters, &mut new_attributes);
            push_index_attribute(force_indices, pcf.symbols.forces, &mut new_attributes);
            push_index_attribute(initializer_indices, pcf.symbols.initializers, &mut new_attributes);
            push_index_attribute(operator_indices, pcf.symbols.operators, &mut new_attributes);
            push_index_attribute(renderer_indices, pcf.symbols.renderers, &mut new_attributes);

            elements[system_idx + 1].attributes = new_attributes;
        }

        Self {
            version: pcf.version,
            strings: pcf.symbols.into(),
            elements,
        }
    }
}

fn attribute_map_to_dmx_map(map: OrderMap<SymbolIdx, Attribute>) -> OrderMap<SymbolIdx, dmx::attribute::Attribute> {
    map.into_iter()
        .map(|(name_idx, attribute)| (name_idx, dmx::attribute::Attribute::from(attribute)))
        .collect()
}

#[derive(Debug, Clone)]
pub struct Child {
    pub name: String,
    pub signature: Signature,
    pub child: ElementIdx,
    pub attributes: OrderMap<SymbolIdx, Attribute>,
}

#[derive(Debug, Clone, Default)]
pub struct ParticleSystem {
    pub name: String,
    pub signature: Signature,
    pub children: Box<[Child]>,
    pub constraints: Box<[Operator]>,
    pub emitters: Box<[Operator]>,
    pub forces: Box<[Operator]>,
    pub initializers: Box<[Operator]>,
    pub operators: Box<[Operator]>,
    pub renderers: Box<[Operator]>,
    pub attributes: OrderMap<SymbolIdx, Attribute>,
}

#[derive(Debug, Clone)]
pub struct Operator {
    pub name: String,
    pub function_name: String,
    pub signature: Signature,
    pub attributes: OrderMap<SymbolIdx, Attribute>,
}

impl Operator {
    fn try_from(element: &Element, symbols: &Symbols) -> Result<Self, Error> {
        let function_name = symbols
            .function_name
            .and_then(|idx| element.attributes.get(&idx))
            .ok_or(Error::MissingFunctionName)?;

        let dmx::attribute::Attribute::String(function_name) = function_name else {
            return Err(Error::MissingFunctionName);
        };

        let mut attributes: OrderMap<SymbolIdx, Attribute> = OrderMap::new();
        for (name_idx, attribute) in &element.attributes {
            if symbols.function_name.is_some_and(|idx| *name_idx == idx) {
                continue;
            }

            attributes.insert(*name_idx, attribute.clone().try_into()?);
        }

        Ok(Self {
            name: element.name.to_string_lossy().into_owned(),
            function_name: function_name.to_string_lossy().into_owned(),
            signature: element.signature,
            attributes,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Symbols {
    pub element: SymbolIdx,
    pub particle_system_definitions: SymbolIdx,
    pub particle_system_definition: SymbolIdx,
    pub particle_child: Option<SymbolIdx>,
    pub particle_operator: Option<SymbolIdx>,
    pub function_name: Option<SymbolIdx>,
    pub children: Option<SymbolIdx>,
    pub constraints: Option<SymbolIdx>,
    pub emitters: Option<SymbolIdx>,
    pub forces: Option<SymbolIdx>,
    pub initializers: Option<SymbolIdx>,
    pub operators: Option<SymbolIdx>,
    pub renderers: Option<SymbolIdx>,
    pub child: Option<SymbolIdx>,
    pub base: OrderSet<String>,
}

impl Symbols {
    pub fn new_with_all_special() -> Self {
        Self {
            element: 0,
            particle_system_definitions: 1,
            particle_system_definition: 2,
            particle_child: Some(3),
            particle_operator: Some(4),
            function_name: Some(5),
            children: Some(6),
            constraints: Some(7),
            emitters: Some(8),
            forces: Some(9),
            initializers: Some(10),
            operators: Some(11),
            renderers: Some(12),
            child: Some(13),
            base: OrderSet::from([
                "DmElement".to_string(),
                "particleSystemDefinitions".to_string(),
                "DmeParticleSystemDefinition".to_string(),
                "DmeParticleChild".to_string(),
                "DmeParticleOperator".to_string(),
                "functionName".to_string(),
                "children".to_string(),
                "constraints".to_string(),
                "emitters".to_string(),
                "forces".to_string(),
                "initializers".to_string(),
                "operators".to_string(),
                "renderers".to_string(),
                "child".to_string(),
            ]),
        }
    }
}

impl Default for Symbols {
    fn default() -> Self {
        Self {
            element: 0,
            particle_system_definitions: 1,
            particle_system_definition: 2,
            particle_child: None,
            particle_operator: None,
            function_name: None,
            children: None,
            constraints: None,
            emitters: None,
            forces: None,
            initializers: None,
            operators: None,
            renderers: None,
            child: None,
            base: OrderSet::from([
                "DmElement".to_string(),
                "particleSystemDefinitions".to_string(),
                "DmeParticleSystemDefinition".to_string(),
            ]),
        }
    }
}

impl TryFrom<dmx::Symbols> for Symbols {
    type Error = Error;

    fn try_from(base: dmx::Symbols) -> Result<Self, Self::Error> {
        fn find_idx(from: &dmx::Symbols, value: &CStr) -> Option<SymbolIdx> {
            from.iter()
                .find_position(|el| *el == value)
                .map(|(idx, _)| idx as SymbolIdx)
        }

        let element = base
            .iter()
            .find_position(|el| *el == c"DmElement" || *el == c"DmeElement")
            .ok_or(Error::MissingDatamodelElementString)?
            .0 as SymbolIdx;

        let particle_system_definitions = base
            .iter()
            .find_position(|el| *el == c"particleSystemDefinitions")
            .ok_or(Error::MissingRootDefinitionString)?
            .0 as SymbolIdx;

        let particle_system_definition = base
            .iter()
            .find_position(|el| *el == c"DmeParticleSystemDefinition")
            .ok_or(Error::MissingSystemDefinitionString)?
            .0 as SymbolIdx;

        let particle_child = find_idx(&base, c"DmeParticleChild");
        let particle_operator = find_idx(&base, c"DmeParticleOperator");
        let function_name = find_idx(&base, c"functionName");
        let children = find_idx(&base, c"children");
        let constraints = find_idx(&base, c"constraints");
        let emitters = find_idx(&base, c"emitters");
        let forces = find_idx(&base, c"forces");
        let initializers = find_idx(&base, c"initializers");
        let operators = find_idx(&base, c"operators");
        let renderers = find_idx(&base, c"renderers");
        let child = find_idx(&base, c"child");

        let base = base
            .into_iter()
            .map(|string| string.to_string_lossy().into_owned())
            .collect();

        Ok(Self {
            element,
            particle_system_definitions,
            particle_system_definition,
            particle_child,
            particle_operator,
            function_name,
            children,
            constraints,
            emitters,
            forces,
            initializers,
            operators,
            renderers,
            child,
            base,
        })
    }
}

impl From<Symbols> for dmx::Symbols {
    fn from(value: Symbols) -> Self {
        value
            .base
            .into_iter()
            .map(|string| {
                let mut vec: Vec<u8> = string.into_bytes();
                vec.push(0);
                CString::from_vec_with_nul(vec).expect("this should never fail")
            })
            .collect()
    }
}

#[cfg(test)]
mod symbol_stripping_tests {
    use dmx::dmx::Version;
    use ordermap::OrderMap;

    use crate::{
        ParticleSystem, Pcf, Root,
        new::{Child, Operator, Symbols},
    };

    #[test]
    fn never_strips_required_symbols() {
        let pcf = Pcf {
            version: Version::Binary2Pcf1,
            symbols: Symbols::default(),
            root: Root::default(),
            encoded_size: 0,
        }
        .unused_symbols_stripped();

        assert_eq!(
            Symbols::default().base,
            pcf.symbols.base,
            "the required symbols (left) were stripped in the resulting pcf (right)"
        );
    }

    #[test]
    fn strips_all_unused_symbols() {
        let mut symbols = Symbols::new_with_all_special();
        symbols.base.insert("test1".to_string());
        symbols.base.insert("test2".to_string());
        symbols.base.insert("test3".to_string());

        let pcf = Pcf {
            version: Version::Binary2Pcf1,
            symbols,
            root: Root::default(),
            encoded_size: 0,
        }
        .unused_symbols_stripped();

        assert!(
            !pcf.symbols.base.contains("test1"),
            "the unused symbols were not stripped from the pcf"
        );
        assert!(
            !pcf.symbols.base.contains("test2"),
            "the unused symbols were not stripped from the pcf"
        );
        assert!(
            !pcf.symbols.base.contains("test3"),
            "the unused symbols were not stripped from the pcf"
        );
        assert_eq!(Symbols::default(), pcf.symbols);
    }

    #[test]
    fn doesnt_strip_used_symbols() {
        fn empty_operator(name: &str, function_name: &str) -> Operator {
            Operator {
                name: name.to_string(),
                function_name: function_name.to_string(),
                signature: [2; 16],
                attributes: OrderMap::new(),
            }
        }

        let pcf = Pcf {
            version: Version::Binary2Pcf1,
            symbols: Symbols::new_with_all_special(),
            root: Root {
                name: "untitled".to_string(),
                signature: [0; 16],
                particle_systems: Box::from([ParticleSystem {
                    name: "parent".to_string(),
                    signature: [1; 16],
                    children: Vec::new().into_boxed_slice(),
                    constraints: Box::from([empty_operator("constraint", "constrain function")]) as Box<[Operator]>,
                    emitters: Box::from([empty_operator("emitter", "emit function")]) as Box<[Operator]>,
                    forces: Box::from([empty_operator("force", "force function")]) as Box<[Operator]>,
                    initializers: Box::from([empty_operator("initializer", "initialize function")]) as Box<[Operator]>,
                    operators: Box::from([empty_operator("operator", "operate function")]) as Box<[Operator]>,
                    renderers: Box::from([empty_operator("renderer", "render function")]) as Box<[Operator]>,
                    attributes: OrderMap::new(),
                }]) as Box<[ParticleSystem]>,
                attributes: OrderMap::new(),
            },
            encoded_size: 0,
        }
        .unused_symbols_stripped();

        assert!(!pcf.symbols.base.contains("DmeParticleChild"));
        assert!(!pcf.symbols.base.contains("child"));
        assert!(!pcf.symbols.base.contains("children"));
    }
}

#[cfg(test)]
mod graph_tests {
    use bytes::Buf;
    use dmx::{Dmx, dmx::Version};
    use ordermap::OrderMap;

    use crate::{
        ParticleSystem, Pcf, Root,
        new::{Child, Symbols},
    };

    #[test]
    fn creates_single_connected_component() {
        let pcf = Pcf {
            version: Version::Binary2Pcf1,
            symbols: Symbols::new_with_all_special(),
            root: Root {
                name: "untitled".to_string(),
                signature: [0; 16],
                particle_systems: Box::from([
                    ParticleSystem {
                        name: "parent".to_string(),
                        signature: [0; 16],
                        children: Box::from([Child {
                            name: "parent_child".to_string(),
                            signature: [0; 16],
                            child: 1usize.into(),
                            attributes: OrderMap::new(),
                        }]) as Box<[Child]>,
                        ..ParticleSystem::default()
                    },
                    ParticleSystem {
                        name: "daughter".to_string(),
                        signature: [0; 16],
                        ..ParticleSystem::default()
                    },
                    ParticleSystem {
                        name: "step parent".to_string(),
                        signature: [0; 16],
                        children: Box::from([
                            Child {
                                name: "parent_child".to_string(),
                                signature: [0; 16],
                                child: 1usize.into(),
                                attributes: OrderMap::new(),
                            },
                            Child {
                                name: "parent_child".to_string(),
                                signature: [0; 16],
                                child: 3usize.into(),
                                attributes: OrderMap::new(),
                            },
                        ]) as Box<[Child]>,
                        ..ParticleSystem::default()
                    },
                    ParticleSystem {
                        name: "step daughter".to_string(),
                        signature: [0; 16],
                        ..ParticleSystem::default()
                    },
                ]) as Box<[ParticleSystem]>,
                attributes: OrderMap::new(),
            },
            encoded_size: 0,
        };

        let graph = pcf.into_connected();
        assert_eq!(1, graph.len());
    }

    #[test]
    fn creates_two_connected_components() {
        let pcf = Pcf {
            version: Version::Binary2Pcf1,
            symbols: Symbols::new_with_all_special(),
            root: Root {
                name: "untitled".to_string(),
                signature: [0; 16],
                particle_systems: Box::from([
                    ParticleSystem {
                        name: "parent".to_string(),
                        signature: [0; 16],
                        children: Box::from([Child {
                            name: "child daughter".to_string(),
                            signature: [0; 16],
                            child: 1usize.into(),
                            attributes: OrderMap::new(),
                        }]) as Box<[Child]>,
                        ..ParticleSystem::default()
                    },
                    ParticleSystem {
                        name: "daughter".to_string(),
                        signature: [0; 16],
                        ..ParticleSystem::default()
                    },
                    ParticleSystem {
                        name: "parent's sister".to_string(),
                        signature: [0; 16],
                        children: Box::from([Child {
                            name: "child cousin".to_string(),
                            signature: [0; 16],
                            child: 3usize.into(),
                            attributes: OrderMap::new(),
                        }]) as Box<[Child]>,
                        ..ParticleSystem::default()
                    },
                    ParticleSystem {
                        name: "daughter's cousin".to_string(),
                        signature: [0; 16],
                        ..ParticleSystem::default()
                    },
                ]) as Box<[ParticleSystem]>,
                attributes: OrderMap::new(),
            },
            encoded_size: 0,
        };

        let mut graph = pcf.into_connected();
        assert_eq!(2, graph.len());

        let parent = graph.pop().unwrap();
        let step_parent = graph.pop().unwrap();

        assert_eq!("untitled", parent.root.name);
        assert_eq!(2, parent.root.particle_systems.len());
        assert_eq!("parent", &parent.root.particle_systems[0].name);
        assert_eq!("daughter", &parent.root.particle_systems[1].name);

        assert_eq!("untitled", step_parent.root.name);
        assert_eq!(2, step_parent.root.particle_systems.len());
        assert_eq!("parent's sister", &step_parent.root.particle_systems[0].name);
        assert_eq!("daughter's cousin", &step_parent.root.particle_systems[1].name);
    }

    const TEST_PCF_DATA: &[u8] = include_bytes!("medicgun_beam.pcf");

    #[test]
    fn connected_components_can_be_reparsed_as_valid_pcf() {
        let mut reader = TEST_PCF_DATA.reader();
        let dmx = dmx::decode(&mut reader).unwrap();
        let pcf: Pcf = dmx.try_into().unwrap();
        for (idx, pcf) in pcf.into_connected().into_iter().enumerate() {
            let dmx: Dmx = pcf.into();
            let _: Pcf = dmx.try_into().unwrap();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{HashSet, VecDeque},
        fs::OpenOptions,
        io::BufWriter,
    };

    use bytes::{Buf, BufMut, BytesMut};
    use dmx::{Dmx, ElementIdx, SymbolIdx, dmx::Element};
    use ordermap::{OrderMap, OrderSet};

    use crate::new::Pcf;

    struct Node {
        children: Vec<char>,
    }

    const TEST_PCF_DATA: &[u8] = include_bytes!("medicgun_beam.pcf");

    #[test]
    fn converts_children() {
        let dmx = Dmx {
            version: dmx::dmx::Version::Binary2Pcf1,
            strings: OrderSet::from([
                c"DmElement".to_owned(),
                c"particleSystemDefinitions".to_owned(),
                c"DmeParticleSystemDefinition".to_owned(),
                c"DmeParticleChild".to_owned(),
                c"children".to_owned(),
                c"child".to_owned(),
                c"root_attribute_1".to_owned(),
                c"system1_attribute_1".to_owned(),
                c"system2_attribute_1".to_owned(),
                c"child_attribute_1".to_owned(),
            ]),
            elements: vec![
                Element {
                    type_idx: 0,
                    name: c"untitled".to_owned(),
                    signature: [0; 16],
                    attributes: OrderMap::from([
                        (6, c"root attribute value".to_owned().into()),
                        (1, [ElementIdx::from(1usize), ElementIdx::from(2usize)].into()),
                    ]),
                },
                Element {
                    type_idx: 2,
                    name: c"system1".to_owned(),
                    signature: [1; 16],
                    attributes: OrderMap::from([
                        (7, c"system1 attribute value".to_owned().into()),
                        (4, [ElementIdx::from(3usize)].into()),
                    ]),
                },
                Element {
                    type_idx: 2,
                    name: c"system2".to_owned(),
                    signature: [2; 16],
                    attributes: OrderMap::from([(8, c"system2 attribute value".to_owned().into())]),
                },
                Element {
                    type_idx: 3,
                    name: c"child1".to_owned(),
                    signature: [3; 16],
                    attributes: OrderMap::from([
                        (9, c"child attribute value".to_owned().into()),
                        (5 as SymbolIdx, ElementIdx::from(2usize).into()),
                    ]),
                },
            ],
        };

        let expected_dmx = dmx.clone();
        let pcf = Pcf::try_from(dmx).unwrap();
        let computed_size = pcf.compute_encoded_size();
        let new_dmx = Dmx::from(pcf);

        let mut writer = BytesMut::new().writer();
        expected_dmx.encode(&mut writer).unwrap();
        let expected_size = writer.get_ref().len();

        let mut writer = BytesMut::new().writer();
        new_dmx.encode(&mut writer).unwrap();
        let new_size = writer.get_ref().len();

        assert_eq!(expected_size, new_size);
        assert_eq!(expected_dmx, new_dmx);
        assert_eq!(
            expected_size, computed_size,
            "compute_encoded_size is not computing the correct size"
        );
    }

    #[test]
    fn converts_operator() {
        let dmx = Dmx {
            version: dmx::dmx::Version::Binary2Pcf1,
            strings: OrderSet::from([
                c"DmElement".to_owned(),
                c"particleSystemDefinitions".to_owned(),
                c"DmeParticleSystemDefinition".to_owned(),
                c"DmeParticleOperator".to_owned(),
                c"operators".to_owned(),
                c"functionName".to_owned(),
                c"root_attribute_1".to_owned(),
                c"system1_attribute_1".to_owned(),
                c"system2_attribute_1".to_owned(),
                c"operator_attribute_1".to_owned(),
            ]),
            elements: vec![
                Element {
                    type_idx: 0,
                    name: c"untitled".to_owned(),
                    signature: [0; 16],
                    attributes: OrderMap::from([
                        (6, c"root attribute value".to_owned().into()),
                        (1, [ElementIdx::from(1usize), ElementIdx::from(2usize)].into()),
                    ]),
                },
                Element {
                    type_idx: 2,
                    name: c"system1".to_owned(),
                    signature: [1; 16],
                    attributes: OrderMap::from([
                        (7, c"system1 attribute value".to_owned().into()),
                        (4, [ElementIdx::from(3usize)].into()),
                    ]),
                },
                Element {
                    type_idx: 2,
                    name: c"system2".to_owned(),
                    signature: [2; 16],
                    attributes: OrderMap::from([(8, c"system2 attribute value".to_owned().into())]),
                },
                Element {
                    type_idx: 3,
                    name: c"operator1".to_owned(),
                    signature: [3; 16],
                    attributes: OrderMap::from([(5, c"test function name".to_owned().into())]),
                },
            ],
        };

        let expected_dmx = dmx.clone();
        let pcf = Pcf::try_from(dmx).unwrap();
        let computed_size = pcf.compute_encoded_size();
        let new_dmx = Dmx::from(pcf);

        let mut writer = BytesMut::new().writer();
        expected_dmx.encode(&mut writer).unwrap();
        let expected_size = writer.get_ref().len();

        let mut writer = BytesMut::new().writer();
        new_dmx.encode(&mut writer).unwrap();
        let new_size = writer.get_ref().len();

        assert_eq!(expected_size, new_size);
        assert_eq!(expected_dmx, new_dmx);
        assert_eq!(
            expected_size, computed_size,
            "compute_encoded_size is not computing the correct size"
        );
    }

    #[test]
    fn computes_correct_size_of_encoded_pcf() {
        let mut reader = TEST_PCF_DATA.reader();
        let dmx = dmx::decode(&mut reader).unwrap();
        let pcf: Pcf = dmx.try_into().unwrap();

        let computed_size = pcf.compute_encoded_size();
        let dmx: Dmx = pcf.into();

        let buf = BytesMut::with_capacity(TEST_PCF_DATA.len());
        let mut writer = buf.writer();
        dmx.encode(&mut writer).expect("writing failed");

        // all of the same data should be present - just reordered - so the sizes should be identical
        let bytes = writer.get_mut();
        assert_eq!(bytes.len(), computed_size as usize);
    }

    #[test]
    fn dmx_to_pcf_to_dmx_has_same_attribute_data() {
        let mut reader = TEST_PCF_DATA.reader();
        let original_dmx = dmx::decode(&mut reader).unwrap();

        let original_strings = original_dmx.strings.clone();
        let mut original_elements = original_dmx.elements.clone();
        original_elements.sort_unstable_by(|a, b| a.name.cmp(&b.name).then(a.signature.cmp(&b.signature)));
        for element in &mut original_elements {
            element
                .attributes
                .sort_unstable_by_key(|a, _| original_strings.get_index(*a as usize).unwrap());
        }

        let pcf: Pcf = original_dmx.try_into().unwrap();
        let new_dmx: Dmx = pcf.into();

        let mut new_elements = new_dmx.elements.clone();
        new_elements.sort_unstable_by(|a, b| a.name.cmp(&b.name).then(a.signature.cmp(&b.signature)));
        for element in &mut new_elements {
            element
                .attributes
                .sort_unstable_by_key(|a, _| original_strings.get_index(*a as usize).unwrap());
        }

        for (idx, original_element) in original_elements.iter_mut().enumerate() {
            let new_element = &new_elements[idx];
            assert_eq!(
                original_element.name,
                new_element.name,
                "new is missing {}",
                original_element.name.display()
            );

            for (name_idx, attribute) in &original_element.attributes {
                if attribute.is_empty_element_array() {
                    continue;
                }

                let name = original_strings.get_index(*name_idx as usize).unwrap();
                let matching_new_name_idx = new_dmx.strings.get_index_of(name).unwrap() as SymbolIdx;
                let new_value = new_element.attributes.get(&matching_new_name_idx).unwrap();
                match attribute {
                    dmx::attribute::Attribute::Element(_) => (),
                    dmx::attribute::Attribute::ElementArray(_) => (),
                    _ => assert_eq!(
                        attribute,
                        new_value,
                        "new {}.{} (#{:x?}) mismatched",
                        original_element.name.display(),
                        name.display(),
                        original_element.signature
                    ),
                }
            }
        }
    }

    #[test]
    fn pcf_converts_to_valid_dmx() {
        let mut reader = TEST_PCF_DATA.reader();
        let original_dmx = dmx::decode(&mut reader).unwrap();

        let attribute_count = original_dmx
            .elements
            .iter()
            .map(|el| el.attributes.len())
            .sum::<usize>();
        println!("attribute count: {attribute_count}");

        let pcf: Pcf = original_dmx.try_into().unwrap();
        println!("{}", pcf.compute_encoded_size());
        let new_dmx: Dmx = pcf.into();

        let attribute_count = new_dmx.elements.iter().map(|el| el.attributes.len()).sum::<usize>();
        println!("attribute count: {attribute_count}");

        let file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open("pcf_converts_to_valid_dmx.pcf")
            .unwrap();
        let mut writer = BufWriter::new(file);
        new_dmx.encode(&mut writer).expect("writing failed");

        let buf = BytesMut::with_capacity(TEST_PCF_DATA.len());
        let mut writer = buf.writer();
        new_dmx.encode(&mut writer).expect("writing failed");

        // the same output should be decodable as a pcf once more
        let mut reader = writer.into_inner().reader();
        let _: Pcf = dmx::decode(&mut reader).unwrap().try_into().unwrap();
    }

    #[test]
    fn compute_merged_size_equal_to_compute_encoded_size_after_merge() {
        let mut reader = TEST_PCF_DATA.reader();
        let dmx = dmx::decode(&mut reader).unwrap();
        let into = Pcf::try_from(dmx).unwrap();
        let from = into.clone();

        let expected_size = into.compute_merged_size(&from);

        let pcf = into.merged(from).unwrap();
        let new_size = pcf.compute_encoded_size();

        assert_eq!(expected_size, new_size);
    }

    #[test]
    fn merging_creates_pcf_that_can_be_converted_into_a_valid_dmx_and_back() {
        let mut reader = TEST_PCF_DATA.reader();
        let dmx = dmx::decode(&mut reader).unwrap();
        let into = Pcf::try_from(dmx).unwrap();
        let from = into.clone();

        let pcf = into.merged(from).unwrap();
        let new_dmx: Dmx = pcf.clone().into();
        let new_pcf: Pcf = new_dmx.try_into().unwrap();
    }

    #[test]
    #[ignore]
    fn test_dfs() {
        fn dfs_wcc(association_list: &OrderMap<char, Node>) -> Vec<OrderSet<char>> {
            fn dfs(
                list: &OrderMap<char, Node>,
                component: &mut OrderSet<char>,
                value: &Node,
                visited: &mut HashSet<char>,
            ) {
                for child in &value.children {
                    if visited.insert(*child) {
                        component.insert(*child);
                        dfs(list, component, list.get(child).unwrap(), visited);
                    }
                }
            }

            let mut visited = HashSet::new();
            let mut components = Vec::new();
            for (key, node) in association_list {
                if visited.insert(*key) {
                    let mut component = OrderSet::from([*key]);
                    dfs(association_list, &mut component, node, &mut visited);
                    components.push(component);
                }
            }

            components
        }

        fn bfs_wcc(graph: &OrderMap<char, Node>) -> Vec<OrderSet<char>> {
            let mut visited = OrderSet::new();
            let mut components = Vec::new();

            for start in graph.keys() {
                if !visited.insert(*start) {
                    continue;
                }

                let mut component = OrderSet::new();
                let mut queue = VecDeque::from([*start]);

                while let Some(value) = queue.pop_front() {
                    component.insert(value);
                    for child in &graph.get(&value).unwrap().children {
                        if visited.insert(*child) {
                            queue.push_back(*child);
                        }
                    }
                }

                components.push(component);
            }

            components
        }

        fn connected_components(graph: &OrderMap<char, Node>) -> Vec<Vec<char>> {
            fn find_connected(
                key: char,
                graph: &OrderMap<char, Node>,
                visited: &mut HashSet<char>,
                component: &mut Vec<char>,
            ) {
                visited.insert(key);
                component.push(key);

                for v in &graph.get(&key).unwrap().children {
                    if visited.contains(v) {
                        continue;
                    }

                    find_connected(*v, graph, visited, component);
                }
            }

            let mut components = Vec::new();
            let mut visited = HashSet::new();

            for key in graph.keys() {
                if visited.contains(key) {
                    continue;
                }

                let mut component = Vec::new();
                find_connected(*key, graph, &mut visited, &mut component);
                components.push(component);
            }

            components
        }

        let nodes = OrderMap::from([
            (
                '0',
                Node {
                    children: vec!['1', '2'],
                },
            ),
            (
                '1',
                Node {
                    children: vec!['0', '3'],
                },
            ),
            (
                '2',
                Node {
                    children: vec!['0', '3'],
                },
            ),
            (
                '3',
                Node {
                    children: vec!['1', '2'],
                },
            ),
            ('4', Node { children: vec!['5'] }),
            ('5', Node { children: vec!['4'] }),
        ]);

        /*
        a -> b
             ^
             |
        d <- c

        e -> f

        */

        let graphs = dfs_wcc(&nodes);
        assert_eq!(2, graphs.len());

        let graphs = bfs_wcc(&nodes);
        assert_eq!(2, graphs.len());

        let graphs = connected_components(&nodes);
        assert_eq!(2, graphs.len());
    }
}
