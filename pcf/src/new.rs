use std::{
    collections::{HashMap, HashSet},
    ffi::{CStr, CString},
    mem,
};

use derive_more::From;
use dmx::{
    ElementIdx, Signature,
    dmx::{Dmx, Element, Version},
};
use itertools::Itertools;
use ordermap::{OrderMap, OrderSet};
use thiserror::Error;

use dmx::attribute::{Bool8, Color, Float, Matrix, Vector2, Vector3, Vector4};

pub type SymbolIdx = u16;
pub type ParticleSystemIdx = usize;

#[derive(Debug, Clone)]
pub struct Pcf {
    pub version: Version,
    pub symbols: Symbols,
    pub root: Root,
}

#[derive(Debug, Clone)]
pub struct Root {
    pub name: String,
    pub signature: Signature,
    pub particle_systems: Box<[ParticleSystem]>,
    pub attributes: OrderMap<SymbolIdx, Attribute>,
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

    #[error("The DMX string list does not contain 'DmElement', so it cant be a valid PCF")]
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
    pub fn merged(self, from: Self) -> Result<Self, MergeError> {
        fn reindex_new_attribute(
            old_to_new_string_idx: &HashMap<u16, u16>,
            name_idx: SymbolIdx,
            attribute: Attribute,
        ) -> (SymbolIdx, Attribute) {
            let name_idx = old_to_new_string_idx
                .get(&name_idx)
                .copied()
                .expect("the attribute's name_idx should always match a value in the Pcf's string list");

            (name_idx, attribute)
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

        let mut root_attributes = self.root.attributes;
        root_attributes.extend(
            from.root
                .attributes
                .into_iter()
                .map(|(name_idx, attribute)| reindex_new_attribute(&old_to_new_string_idx, name_idx, attribute)),
        );

        let mut particle_systems = Vec::from(self.root.particle_systems);
        let system_offset = particle_systems.len();

        for mut new_system in from.root.particle_systems {
            for child in &mut new_system.children {
                child.child += system_offset;
            }

            new_system.attributes = new_system
                .attributes
                .into_iter()
                .map(|(name_idx, attribute)| reindex_new_attribute(&old_to_new_string_idx, name_idx, attribute))
                .collect();

            particle_systems.push(new_system);
        }

        Ok(Self {
            version: self.version,
            symbols,
            root: Root {
                name: self.root.name,
                signature: self.root.signature,
                particle_systems: particle_systems.into_boxed_slice(),
                attributes: root_attributes,
            },
        })
    }

    pub fn compute_encoded_size(&self) -> u64 {
        fn compute_attributes_size(attributes: &OrderMap<SymbolIdx, Attribute>) -> usize {
            (size_of::<SymbolIdx>() * attributes.len())
                + attributes
                    .iter()
                    .map(|(_, attribute)| attribute.get_encoded_size())
                    .sum::<usize>()
        }

        fn compute_operator_size(operator: &Operator) -> usize {
            operator.name.len() + 1 + operator.function_name.len() + 1 + compute_attributes_size(&operator.attributes)
        }

        let version_size: usize = self.version.as_cstr_with_nul_terminator().to_bytes_with_nul().len();

        // there is a nul byte for every string when encoded, so we include that as well by adding .len()
        let symbols_size: usize = size_of::<u16>()
            + self.symbols.base.len()
            + self.symbols.base.iter().map(|string| string.len()).sum::<usize>();

        // accounting for the element counter
        let mut elements_size = size_of::<u32>();

        // and the root element & attribute sizes. signature & type arent included until later
        elements_size += self.root.name.len() + 1 + compute_attributes_size(&self.root.attributes);

        // the particle systems array will itself become an attribute
        elements_size += size_of::<u32>() + (size_of::<ElementIdx>() * self.root.particle_systems.len());

        // the first elements are the root + particle system definitions
        let mut element_count = 1 + self.root.particle_systems.len();

        for system in &self.root.particle_systems {
            elements_size += compute_attributes_size(&system.attributes);

            for child in &system.children {
                elements_size += compute_attributes_size(&child.attributes);
            }

            element_count += system.children.len()
                + system.constraints.len()
                + system.emitters.len()
                + system.forces.len()
                + system.initializers.len()
                + system.operators.len()
                + system.renderers.len();

            elements_size += system.constraints.iter().map(compute_operator_size).sum::<usize>();
            elements_size += system.emitters.iter().map(compute_operator_size).sum::<usize>();
            elements_size += system.forces.iter().map(compute_operator_size).sum::<usize>();
            elements_size += system.initializers.iter().map(compute_operator_size).sum::<usize>();
            elements_size += system.operators.iter().map(compute_operator_size).sum::<usize>();
            elements_size += system.renderers.iter().map(compute_operator_size).sum::<usize>();
        }

        // the type and signature are included with every element, so we can add it all at once
        elements_size += element_count * (size_of::<u16>() + size_of::<Signature>());

        (version_size + symbols_size + elements_size) as u64
    }

    pub fn compute_merged_size(&self, from: &Self) -> u64 {
        self.clone()
            .merged(from.clone())
            .expect("this should never fail")
            .compute_encoded_size()
    }

    /// Consumes the [`Pcf`], splitting it up into multiple [`Pcf`]s. Each [`Pcf`] will only contain
    /// [`ParticleSystem`]s that are connected (such as by a via [`Child`] link).
    ///
    /// Each [`ParticleSystem`] from the original [`Pcf`] will only show up once.
    ///
    /// Unused symbols are also stripped from each [`Pcf`], to ensure it only contains the bare minimum necessary to be
    /// a valid [`Pcf`].
    pub fn into_connected_pcfs(self) -> Vec<Self> {
        let mut groups = Vec::new();
        let mut system_to_group_idx: HashMap<ElementIdx, usize> = HashMap::new();

        for (system_idx, particle_system) in self.root.particle_systems.into_iter().enumerate() {
            let related_groups: Vec<_> = particle_system
                .children
                .iter()
                .filter_map(|child| {
                    system_to_group_idx
                        .get(&child.child)
                        .map(|group_idx| (child.child, *group_idx))
                })
                .collect();

            if related_groups.is_empty() {
                // there are no related groups, so we don't need to merge anything, and should create a new group
                system_to_group_idx.insert(system_idx.into(), groups.len());
                groups.push(vec![(system_idx, particle_system)]);
            } else {
                // we've got some related groups, so we need to merge them all together, and add our particle system to the group
                let (indices_to_remap, group_indices): (Vec<_>, Vec<_>) = related_groups.into_iter().unzip();

                let mut groups_to_merge = Vec::with_capacity(group_indices.len() + 1);
                groups_to_merge.push(vec![(system_idx, particle_system)]);
                for group_idx in group_indices {
                    groups_to_merge.push(groups.remove(group_idx));
                }

                for idx_to_remap in indices_to_remap {
                    system_to_group_idx.insert(idx_to_remap, groups.len());
                }

                groups.push(groups_to_merge.into_iter().concat());
            }
        }

        // at this point each group should only contain related particle systems. Now we need to fix child indices
        for group in &mut groups {
            let (old_to_new_idx, mut particle_systems): (HashMap<_, _>, Vec<_>) = group
                .iter_mut()
                .enumerate()
                .map(|(new_idx, (old_idx, particle_system))| {
                    ((ElementIdx::from(*old_idx), ElementIdx::from(new_idx)), particle_system)
                })
                .unzip();

            for particle_system in &mut particle_systems {
                for child in &mut particle_system.children {
                    child.child = *old_to_new_idx
                        .get(&child.child)
                        .expect("old indices should always be in the old_to_new_idx map")
                }
            }
        }

        groups
            .into_iter()
            .map(|group| {
                let pcf = Pcf {
                    version: self.version,
                    symbols: self.symbols.clone(),
                    root: Root {
                        name: self.root.name.clone(),
                        signature: self.root.signature,
                        particle_systems: group.into_iter().map(|(_, system)| system).collect(),
                        attributes: self.root.attributes.clone(),
                    },
                };

                pcf.strip_unused_symbols()
            })
            .collect()
    }

    /// Consumes the [`Pcf`], returning a new [`Pcf`] with all unused symbols removed. References to symbols are
    /// replaced with the new index for each symbol.
    pub fn strip_unused_symbols(mut self) -> Self {
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
            used_symbols.insert(self.symbols.child);
            used_symbols.insert(self.symbols.particle_child);
            used_symbols.insert(self.symbols.children);
        }

        if has_constraint || has_emitter || has_force || has_initializer || has_operator || has_renderer {
            used_symbols.insert(self.symbols.particle_operator);
            used_symbols.insert(self.symbols.function_name);
        }

        if has_constraint {
            used_symbols.insert(self.symbols.operators);
        }

        if has_emitter {
            used_symbols.insert(self.symbols.emitters);
        }

        if has_emitter {
            used_symbols.insert(self.symbols.forces);
        }

        if has_emitter {
            used_symbols.insert(self.symbols.initializers);
        }

        if has_emitter {
            used_symbols.insert(self.symbols.operators);
        }

        if has_emitter {
            used_symbols.insert(self.symbols.renderers);
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
        self.symbols.particle_child = *old_to_new_idx
            .get(&self.symbols.particle_child)
            .unwrap_or(&SymbolIdx::MAX);
        self.symbols.particle_operator = *old_to_new_idx
            .get(&self.symbols.particle_operator)
            .unwrap_or(&SymbolIdx::MAX);
        self.symbols.function_name = *old_to_new_idx
            .get(&self.symbols.function_name)
            .unwrap_or(&SymbolIdx::MAX);
        self.symbols.children = *old_to_new_idx.get(&self.symbols.children).unwrap_or(&SymbolIdx::MAX);
        self.symbols.constraints = *old_to_new_idx.get(&self.symbols.constraints).unwrap_or(&SymbolIdx::MAX);
        self.symbols.emitters = *old_to_new_idx.get(&self.symbols.emitters).unwrap_or(&SymbolIdx::MAX);
        self.symbols.forces = *old_to_new_idx.get(&self.symbols.forces).unwrap_or(&SymbolIdx::MAX);
        self.symbols.initializers = *old_to_new_idx
            .get(&self.symbols.initializers)
            .unwrap_or(&SymbolIdx::MAX);
        self.symbols.operators = *old_to_new_idx.get(&self.symbols.operators).unwrap_or(&SymbolIdx::MAX);
        self.symbols.renderers = *old_to_new_idx.get(&self.symbols.renderers).unwrap_or(&SymbolIdx::MAX);
        self.symbols.child = *old_to_new_idx.get(&self.symbols.child).unwrap_or(&SymbolIdx::MAX);

        self
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
        let system_indices: HashMap<_, _> = system_indices
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
                    if *name_idx == symbols.children {
                        for child_element_idx in element_indices {
                            let child_element = value
                                .elements
                                .get(usize::from(*child_element_idx))
                                .ok_or(Error::MissingParticleChild(*child_element_idx))?;

                            if child_element.type_idx != symbols.particle_child {
                                return Err(Error::InvalidParticleChild(*child_element_idx));
                            }

                            let child_attribute = child_element
                                .attributes
                                .get(&symbols.child)
                                .ok_or(Error::MissingChild)?;
                            let dmx::attribute::Attribute::Element(child_system_idx) = child_attribute else {
                                return Err(Error::MissingChild);
                            };

                            let mut attributes = OrderMap::new();
                            for (name_idx, attribute) in &child_element.attributes {
                                if *name_idx == symbols.child {
                                    continue;
                                }

                                attributes.insert(*name_idx, attribute.try_into()?);
                            }

                            let name = child_element.name.to_string_lossy().into_owned();
                            let signature = child_element.signature;
                            children.push(Child {
                                name,
                                signature,
                                attributes,
                                child: *system_indices
                                    .get(child_system_idx)
                                    .expect("this relationship should always be valid"),
                            });
                        }
                        continue;
                    }

                    let operators = if *name_idx == symbols.constraints {
                        &mut constraints
                    } else if *name_idx == symbols.emitters {
                        &mut emitters
                    } else if *name_idx == symbols.forces {
                        &mut forces
                    } else if *name_idx == symbols.initializers {
                        &mut initializers
                    } else if *name_idx == symbols.operators {
                        &mut operators
                    } else if *name_idx == symbols.renderers {
                        &mut renderers
                    } else {
                        return Err(Error::UnexpectedElementReference);
                    };

                    for element_idx in element_indices {
                        let element = value
                            .elements
                            .get(usize::from(*element_idx))
                            .ok_or(Error::MissingOperator(*element_idx))?;

                        if element.type_idx != symbols.particle_operator {
                            return Err(Error::InvalidParticleOperator(*element_idx));
                        }

                        operators.push(Operator::try_from(element, &symbols)?);
                    }
                } else {
                    attributes.insert(*name_idx, attribute.try_into()?);
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

            attributes.insert(*name_idx, attribute.try_into()?);
        }

        let root = Root {
            name: root_element.name.to_string_lossy().into_owned(),
            signature: root_element.signature,
            particle_systems: particle_systems.into_boxed_slice(),
            attributes,
        };

        Ok(Self {
            version: value.version,
            symbols,
            root,
        })
    }
}

impl From<Pcf> for Dmx {
    fn from(value: Pcf) -> Self {
        fn push_operators(
            operators: Box<[Operator]>,
            elements: &mut Vec<Element>,
            indices: &mut Vec<ElementIdx>,
            symbols: &Symbols,
        ) {
            for operator in operators {
                indices.push(ElementIdx::from(elements.len()));

                let mut attributes = attribute_map_to_dmx_map(operator.attributes);
                attributes.insert(symbols.function_name, string_to_cstring(operator.function_name).into());

                elements.push(Element {
                    type_idx: symbols.particle_operator,
                    name: string_to_cstring(operator.name),
                    signature: operator.signature,
                    attributes,
                })
            }
        }

        fn push_index_attribute(
            indices: Vec<ElementIdx>,
            attributes: &mut OrderMap<SymbolIdx, dmx::attribute::Attribute>,
            symbols: &Symbols,
        ) {
            if !indices.is_empty() {
                attributes.insert(symbols.children, indices.into_boxed_slice().into());
            }
        }

        let mut root_attributes = attribute_map_to_dmx_map(value.root.attributes);
        let particle_system_definitions: Box<_> =
            (0..value.root.particle_systems.len()).map(ElementIdx::from).collect();

        root_attributes.insert(
            value.symbols.particle_system_definitions,
            particle_system_definitions.into(),
        );

        let root_element = Element {
            type_idx: value.symbols.element,
            name: string_to_cstring(value.root.name),
            signature: value.root.signature,
            attributes: root_attributes,
        };

        let mut elements = vec![root_element];
        for particle_system in &value.root.particle_systems {
            elements.push(Element {
                type_idx: value.symbols.particle_system_definition,
                name: str_to_cstring(&particle_system.name),
                signature: particle_system.signature,
                attributes: OrderMap::new(),
            })
        }

        for (system_idx, particle_system) in value.root.particle_systems.into_iter().enumerate() {
            let mut child_indices = Vec::new();
            let mut constraint_indices = Vec::new();
            let mut emitter_indices = Vec::new();
            let mut force_indices = Vec::new();
            let mut initializer_indices = Vec::new();
            let mut operator_indices = Vec::new();
            let mut renderer_indices = Vec::new();

            for child in particle_system.children {
                child_indices.push(ElementIdx::from(elements.len()));

                let mut attributes = attribute_map_to_dmx_map(child.attributes);

                // child.child indexes into value.root.particle_systems, but we insert particle system defintiions
                // into `elements` before any others, so this index is still correct in our new elements list
                // except we have to offset by 1 to account for the root element we add earlier.
                attributes.insert(value.symbols.child, dmx::attribute::Attribute::Element(child.child + 1));

                elements.push(Element {
                    type_idx: value.symbols.particle_child,
                    name: string_to_cstring(child.name),
                    signature: child.signature,
                    attributes,
                })
            }

            push_operators(
                particle_system.constraints,
                &mut elements,
                &mut constraint_indices,
                &value.symbols,
            );
            push_operators(
                particle_system.emitters,
                &mut elements,
                &mut emitter_indices,
                &value.symbols,
            );
            push_operators(
                particle_system.forces,
                &mut elements,
                &mut force_indices,
                &value.symbols,
            );
            push_operators(
                particle_system.initializers,
                &mut elements,
                &mut initializer_indices,
                &value.symbols,
            );
            push_operators(
                particle_system.operators,
                &mut elements,
                &mut operator_indices,
                &value.symbols,
            );
            push_operators(
                particle_system.renderers,
                &mut elements,
                &mut renderer_indices,
                &value.symbols,
            );

            let mut new_attributes = attribute_map_to_dmx_map(particle_system.attributes);

            push_index_attribute(child_indices, &mut new_attributes, &value.symbols);
            push_index_attribute(constraint_indices, &mut new_attributes, &value.symbols);
            push_index_attribute(emitter_indices, &mut new_attributes, &value.symbols);
            push_index_attribute(force_indices, &mut new_attributes, &value.symbols);
            push_index_attribute(initializer_indices, &mut new_attributes, &value.symbols);
            push_index_attribute(operator_indices, &mut new_attributes, &value.symbols);
            push_index_attribute(renderer_indices, &mut new_attributes, &value.symbols);

            elements[system_idx].attributes = new_attributes;
        }

        Self {
            version: value.version,
            strings: value.symbols.into(),
            elements,
        }
    }
}

fn string_to_cstring(string: String) -> CString {
    let mut vec: Vec<u8> = string.into_bytes();
    vec.push(0);
    CString::from_vec_with_nul(vec).expect("this should never fail")
}

fn str_to_cstring(string: &str) -> CString {
    CString::new(string).expect("this should never fail")
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

#[derive(Debug, Clone)]
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
        let function_name = element
            .attributes
            .get(&symbols.function_name)
            .ok_or(Error::MissingFunctionName)?;

        let dmx::attribute::Attribute::String(function_name) = function_name else {
            return Err(Error::MissingFunctionName);
        };

        let mut attributes: OrderMap<SymbolIdx, Attribute> = OrderMap::new();
        for (name_idx, attribute) in &element.attributes {
            if *name_idx == symbols.function_name {
                continue;
            }

            attributes.insert(*name_idx, attribute.try_into()?);
        }

        Ok(Self {
            name: element.name.to_string_lossy().into_owned(),
            function_name: function_name.to_string_lossy().into_owned(),
            signature: element.signature,
            attributes,
        })
    }
}

#[derive(Debug, From, Clone)]
pub enum Attribute {
    Integer(i32),
    Float(Float),
    Bool(bool),
    String(String),
    Binary(Box<[u8]>),
    Color(Color),
    Vector2(Vector2),
    Vector3(Vector3),
    Vector4(Vector4),
    Matrix(Matrix),
    IntegerArray(Box<[i32]>),
    FloatArray(Box<[Float]>),
    BoolArray(Box<[Bool8]>),
    StringArray(Box<[String]>),
    BinaryArray(Box<[Box<[u8]>]>),
    ColorArray(Box<[Color]>),
    Vector2Array(Box<[Vector2]>),
    Vector3Array(Box<[Vector3]>),
    Vector4Array(Box<[Vector4]>),
    MatrixArray(Box<[Matrix]>),
}

impl Attribute {
    fn get_encoded_size(&self) -> usize {
        match self {
            Attribute::Integer(value) => size_of_val(value),
            Attribute::Float(value) => size_of_val(value),
            Attribute::Bool(value) => size_of_val(value),
            Attribute::String(value) => 1 + value.len(),
            Attribute::Binary(value) => value.len(),
            Attribute::Color(value) => size_of_val(value),
            Attribute::Vector2(value) => size_of_val(value),
            Attribute::Vector3(value) => size_of_val(value),
            Attribute::Vector4(value) => size_of_val(value),
            Attribute::Matrix(value) => size_of_val(value),
            Attribute::IntegerArray(value) => size_of::<u32>() + value.iter().map(size_of_val).sum::<usize>(),
            Attribute::FloatArray(value) => size_of::<u32>() + value.iter().map(size_of_val).sum::<usize>(),
            Attribute::BoolArray(value) => size_of::<u32>() + value.iter().map(size_of_val).sum::<usize>(),
            Attribute::StringArray(value) => {
                size_of::<u32>() + value.len() + value.iter().map(String::len).sum::<usize>()
            }
            Attribute::BinaryArray(value) => size_of::<u32>() + value.iter().map(|value| value.len()).sum::<usize>(),
            Attribute::ColorArray(value) => size_of::<u32>() + value.iter().map(size_of_val).sum::<usize>(),
            Attribute::Vector2Array(value) => size_of::<u32>() + value.iter().map(size_of_val).sum::<usize>(),
            Attribute::Vector3Array(value) => size_of::<u32>() + value.iter().map(size_of_val).sum::<usize>(),
            Attribute::Vector4Array(value) => size_of::<u32>() + value.iter().map(size_of_val).sum::<usize>(),
            Attribute::MatrixArray(value) => size_of::<u32>() + value.iter().map(size_of_val).sum::<usize>(),
        }
    }
}

impl TryFrom<&dmx::attribute::Attribute> for Attribute {
    type Error = Error;

    fn try_from(value: &dmx::attribute::Attribute) -> Result<Self, Self::Error> {
        match value {
            dmx::attribute::Attribute::Element(_) => Err(Error::UnexpectedElementReference),
            dmx::attribute::Attribute::Integer(value) => Ok((*value).into()),
            dmx::attribute::Attribute::Float(value) => Ok((*value).into()),
            dmx::attribute::Attribute::Bool(value) => Ok(bool::from(*value).into()),
            dmx::attribute::Attribute::String(value) => Ok(value.to_string_lossy().into_owned().into()),
            dmx::attribute::Attribute::Binary(value) => Ok(value.clone().into()),
            dmx::attribute::Attribute::Color(value) => Ok((*value).into()),
            dmx::attribute::Attribute::Vector2(value) => Ok((*value).into()),
            dmx::attribute::Attribute::Vector3(value) => Ok((*value).into()),
            dmx::attribute::Attribute::Vector4(value) => Ok((*value).into()),
            dmx::attribute::Attribute::Matrix(value) => Ok((*value).into()),
            dmx::attribute::Attribute::ElementArray(_) => Err(Error::UnexpectedElementReference),
            dmx::attribute::Attribute::IntegerArray(value) => Ok(value.clone().into()),
            dmx::attribute::Attribute::FloatArray(value) => Ok(value.clone().into()),
            dmx::attribute::Attribute::BoolArray(value) => Ok(value.clone().into()),
            dmx::attribute::Attribute::StringArray(value) => Ok(value
                .into_iter()
                .map(|string| string.to_string_lossy().into_owned())
                .collect::<Box<[String]>>()
                .into()),
            dmx::attribute::Attribute::BinaryArray(value) => Ok(value.clone().into()),
            dmx::attribute::Attribute::ColorArray(value) => Ok(value.clone().into()),
            dmx::attribute::Attribute::Vector2Array(value) => Ok(value.clone().into()),
            dmx::attribute::Attribute::Vector3Array(value) => Ok(value.clone().into()),
            dmx::attribute::Attribute::Vector4Array(value) => Ok(value.clone().into()),
            dmx::attribute::Attribute::MatrixArray(value) => Ok(value.clone().into()),
        }
    }
}

impl From<Attribute> for dmx::attribute::Attribute {
    fn from(value: Attribute) -> Self {
        match value {
            Attribute::Integer(value) => value.into(),
            Attribute::Float(value) => value.into(),
            Attribute::Bool(value) => value.into(),
            Attribute::String(value) => string_to_cstring(value).into(),
            Attribute::Binary(value) => value.into(),
            Attribute::Color(value) => value.into(),
            Attribute::Vector2(value) => value.into(),
            Attribute::Vector3(value) => value.into(),
            Attribute::Vector4(value) => value.into(),
            Attribute::Matrix(value) => value.into(),
            Attribute::IntegerArray(value) => value.into(),
            Attribute::FloatArray(value) => value.into(),
            Attribute::BoolArray(value) => value.into(),
            Attribute::StringArray(value) => value
                .into_iter()
                .map(string_to_cstring)
                .collect::<Box<[CString]>>()
                .into(),
            Attribute::BinaryArray(value) => value.into(),
            Attribute::ColorArray(value) => value.into(),
            Attribute::Vector2Array(value) => value.into(),
            Attribute::Vector3Array(value) => value.into(),
            Attribute::Vector4Array(value) => value.into(),
            Attribute::MatrixArray(value) => value.into(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Symbols {
    pub element: SymbolIdx,
    pub particle_system_definitions: SymbolIdx,
    pub particle_system_definition: SymbolIdx,
    pub particle_child: SymbolIdx,
    pub particle_operator: SymbolIdx,
    pub function_name: SymbolIdx,
    pub children: SymbolIdx,
    pub constraints: SymbolIdx,
    pub emitters: SymbolIdx,
    pub forces: SymbolIdx,
    pub initializers: SymbolIdx,
    pub operators: SymbolIdx,
    pub renderers: SymbolIdx,
    pub child: SymbolIdx,
    pub base: OrderSet<String>,
}

impl TryFrom<dmx::Symbols> for Symbols {
    type Error = Error;

    fn try_from(base: dmx::Symbols) -> Result<Self, Self::Error> {
        fn idx_or_max(from: &dmx::Symbols, value: &CStr) -> SymbolIdx {
            from.iter()
                .find_position(|el| *el == value)
                .map(|(idx, _)| idx as SymbolIdx)
                .unwrap_or(SymbolIdx::MAX)
        }

        let element = base
            .iter()
            .find_position(|el| *el == c"DmElement")
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

        let particle_child = idx_or_max(&base, c"DmeParticleChild");
        let particle_operator = idx_or_max(&base, c"DmeParticleOperator");
        let function_name = idx_or_max(&base, c"functionName");
        let children = idx_or_max(&base, c"children");
        let constraints = idx_or_max(&base, c"constraints");
        let emitters = idx_or_max(&base, c"emitters");
        let forces = idx_or_max(&base, c"forces");
        let initializers = idx_or_max(&base, c"initializers");
        let operators = idx_or_max(&base, c"operators");
        let renderers = idx_or_max(&base, c"renderers");
        let child = idx_or_max(&base, c"child");

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
