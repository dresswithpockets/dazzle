use std::{collections::{BTreeMap, HashMap}, ffi::CString};

use ordermap::OrderMap;
use pcf::{ElementsExt, Pcf, index::ElementIdx};
use typed_path::Utf8PlatformPathBuf;
use vpk::VPK;

#[derive(Debug)]
pub struct App {
    pub addons_dir: Utf8PlatformPathBuf,
    pub extracted_content_dir: Utf8PlatformPathBuf,
    pub backup_dir: Utf8PlatformPathBuf,
    pub working_vpk_dir: Utf8PlatformPathBuf,

    pub vanilla_pcf_paths: Vec<Utf8PlatformPathBuf>,
    pub vanilla_pcf_to_systems: HashMap<String, Vec<CString>>,
    pub vanilla_system_to_pcf: HashMap<CString, String>,

    pub tf_misc_vpk: VPK,
    pub tf_custom_dir: Utf8PlatformPathBuf,
}

impl App {
    pub fn merge_addon_particles(
        &self,
        particle_files: &HashMap<Utf8PlatformPathBuf, pcf::Pcf>,
    ) -> HashMap<String, Vec<Pcf>> {
        let mut processed_target_pcf_paths: HashMap<String, Vec<Pcf>> = HashMap::new();
        for (file_path, pcf) in particle_files {
            // dx80 and dx90 are a special case that we skip over. TODO: i think we generate them later?
            let file_name: &str = file_path.file_name().expect("there should always be a file name");
            if file_name.contains("dx80") || file_name.contains("dx90") {
                continue;
            }

            println!("merging systems from {file_path}");
            // grouping the elements from our addon by the vanilla PCF they're mapped to in particle_system_map.json.
            let mut systems_by_vanilla_pcf_path = HashMap::<&String, OrderMap<&CString, (ElementIdx, &pcf::Element)>>::new();
            println!("  has {} elements", pcf.elements().len());
            for (element_idx, element) in pcf.elements().iter().enumerate() {
                let Some(pcf_path) = self.vanilla_system_to_pcf.get(&element.name) else {
                    continue;
                };

                println!("  discovered element '{}' in map", element.name.display());

                // we're also ridding ourselves of duplicate particle systems here. The first one always takes priority,
                // subsequent particle systems with the same name are skipped entirely.
                systems_by_vanilla_pcf_path
                    .entry(pcf_path)
                    .or_default()
                    .entry(&element.name)
                    .or_insert((element_idx.into(), element));
            }

            for (target_pcf_path, matched_systems) in systems_by_vanilla_pcf_path {
                println!("reindexing discovered elements");
                // matched_elements contains a subset of the original elements in the pcf. As a result, any
                // Element or ElementArray attributes may not point to the correct index - the order is
                // retained but the indices aren't. So, we need to reindex any references to other elements in the set.
                
                let new_elements = Pcf::reindex_elements(pcf, matched_systems.values().map(|el| &el.0));

                // the root element always stores an attribute "particleSystemDefinitions" which stores an ElementArray
                // containing the index of every DmeParticleSystemDefinition-type element. We've changed the indices of
                // our particle system definitions, so we need to update the root element's list with the new indices.
                let particle_system_indices: Vec<ElementIdx> = new_elements
                    .iter()
                    .map_particle_system_indices(&pcf.strings().particle_system_definition_type_idx)
                    .collect();

                // our filtered `new_elements` only contains particle systems, it does not contain a root element
                let root = pcf::Root {
                    type_idx: pcf.root().type_idx,
                    name: pcf.root().name.clone(),
                    signature: pcf.root().signature,
                    definitions: particle_system_indices.into_boxed_slice(),
                    attributes: pcf.root().attributes.clone(), // TODO: do we need to reindex these?
                };

                // this new in-memory PCF has only the elements listed in elements_to_extract, with element references
                // fixed to match any changes in indices.
                let new_pcf = pcf::Pcf::builder()
                    .version(pcf.version())
                    .strings(pcf.strings().clone())
                    .root(root)
                    .elements(new_elements)
                    .build();

                processed_target_pcf_paths
                    .entry(target_pcf_path.clone())
                    .or_default()
                    .push(new_pcf);
            }
        }

        processed_target_pcf_paths
    }

    pub(crate) fn reindex_elements<'a>(
        source_pcf: &'a Pcf,
        systems: impl IntoIterator<Item = &'a ElementIdx>,
    ) -> Vec<pcf::Element> {
        let mut new_elements = Vec::new();
        let mut original_elements: BTreeMap<ElementIdx, &pcf::Element> = BTreeMap::new();
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
                    pcf::Attribute::Element(old_idx) if old_idx.is_valid() => {
                        pcf::Attribute::Element(*old_to_new_idx.get(old_idx).unwrap_or(old_idx))
                    }
                    pcf::Attribute::ElementArray(old_indices) => pcf::Attribute::ElementArray(
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

            new_elements.push(pcf::Element {
                type_idx: element.type_idx,
                name: element.name.clone(),
                signature: element.signature,
                attributes,
            });
        }

        new_elements
    }

    // fn strip_default_values(
    //     pcf: Pcf,
    //     particle_system_defaults: &HashMap<&'static CStr, Attribute>,
    //     operator_defaults: &HashMap<&'static CStr, Attribute>,
    // ) -> anyhow::Result<Pcf> {
    //     let particle_system_defaults: HashMap<NameIndex, &Attribute> = particle_system_defaults
    //         .iter()
    //         .filter_map(|(key, value)| {
    //             pcf.strings().iter()
    //                 .position(|s| s.0.as_bytes().eq_ignore_ascii_case(key.to_bytes()))
    //                 .map(|idx| (idx as NameIndex, value))
    //         })
    //         .collect();

    //     let operator_defaults: HashMap<NameIndex, &Attribute> = operator_defaults
    //         .iter()
    //         .filter_map(|(key, value)| {
    //             pcf.strings().iter()
    //                 .position(|s| s.0.as_bytes().eq_ignore_ascii_case(key.to_bytes()))
    //                 .map(|idx| (idx as NameIndex, value))
    //         })
    //         .collect();

    //     let mut elements = Vec::new();
    //     for element in pcf.elements() {
    //         let attributes = if element.type_idx == pcf.strings().particle_system_definition_type_idx {
    //             element.attributes
    //                 .into_iter()
    //                 .filter(|(name_idx, attribute)| {
    //                     if let Some(default) = particle_system_defaults.get(name_idx) && attribute == *default {
    //                         false
    //                     } else {
    //                         true
    //                     }
    //                 })
    //                 .collect()
    //         } else if element.type_idx == pcf.strings().particle_operator_type_idx {
    //             element.attributes
    //                 .into_iter()
    //                 .filter(|(name_idx, attribute)| {
    //                     if let Some(default) = operator_defaults.get(name_idx) && attribute == *default {
    //                         false
    //                     } else {
    //                         true
    //                     }
    //                 })
    //                 .collect()
    //         } else {
    //             element.attributes
    //         };

    //         elements.push(Element {
    //             attributes,
    //             ..element
    //         });
    //     }

    //     Ok(Pcf {
    //         elements,
    //         ..pcf
    //     })
    // }

    #[cfg(not(feature = "split_item_fx_pcf"))]
    pub fn process_mapped_particles(
        target_pcf: Pcf,
        mut pcf_files: Vec<Pcf>,
    ) -> anyhow::Result<Pcf> {
        // We took care of duplicate elements from our addon when grouping addon elements by vanilla PCF, so we
        // don't do any special handling for duplicate elements here.
        let merged_pcf = pcf_files.pop().expect("there should be at least one pcf in the group");
        let merged_pcf = pcf_files.into_iter().try_fold(merged_pcf, Pcf::merge)?;

        let merged_pcf = merged_pcf
            .merge(target_pcf)
            .expect("failed to merge the vanilla PCF into the modified PCF");

        Ok(merged_pcf)
    }

    #[cfg(feature = "split_item_fx_pcf")]
    fn process_mapped_particles(
        target_pcf_path: &str,
        target_pcf: Pcf,
        mut pcf_files: Vec<Pcf>,
    ) -> anyhow::Result<Vec<(&str, Pcf)>> {
        fn cstr_starts_with(string: &std::ffi::CStr, prefix: &std::ffi::CStr) -> bool {
            string.to_bytes().starts_with(prefix.to_bytes())
        }

        // We took care of duplicate elements from our addon when grouping addon elements by vanilla PCF, so we
        // don't do any special handling for duplicate elements here.
        let merged_pcf = pcf_files.pop().expect("there should be at least one pcf in the group");
        let merged_pcf = pcf_files.into_iter().try_fold(merged_pcf, Pcf::merge)?;

        let merged_pcf = merged_pcf
            .merge(target_pcf)
            .expect("failed to merge the vanilla PCF into the modified PCF");

        // item_fx.pcf is a special case, its elements will get split up into item_fx_unusuals.pcf and into
        // item_fx_gameplay.pcf
        let system_definition_type_idx = merged_pcf
            .index_of_string(c"DmeParticleSystemDefinition")
            .expect("DmeParticleSystemDefinition should always be present");
        let processed_pcfs = if target_pcf_path == "particles/item_fx.pcf" {
            let (unusual_elements, gameplay_elements): (Vec<_>, Vec<_>) = merged_pcf.elements.iter().partition(|el| {
                el.name == c"superare_balloon"
                    || cstr_starts_with(&el.name, c"superrare_")
                    || cstr_starts_with(&el.name, c"unusual_")
            });

            let unusual_elements = Self::reindex_elements(&merged_pcf, unusual_elements);
            let gameplay_elements = Self::reindex_elements(&merged_pcf, gameplay_elements);

            let unusual_system_indices: Vec<_> = unusual_elements
                .iter()
                .map_particle_system_indices(&system_definition_type_idx)
                .collect();
            let gameplay_system_indices: Vec<_> = gameplay_elements
                .iter()
                .map_particle_system_indices(&system_definition_type_idx)
                .collect();

            let unusual_root = pcf::Root {
                type_idx: merged_pcf.root.type_idx,
                name: merged_pcf.root.name.clone(),
                signature: merged_pcf.root.signature,
                definitions: unusual_system_indices.into_boxed_slice(),
            };

            let gameplay_root = pcf::Root {
                type_idx: merged_pcf.root.type_idx,
                name: merged_pcf.root.name.clone(),
                signature: merged_pcf.root.signature,
                definitions: gameplay_system_indices.into_boxed_slice(),
            };

            let unusual_pcf = pcf::Pcf::builder()
                .version(merged_pcf.version)
                .strings(merged_pcf.strings.iter().map(|el| el.0.clone()).collect())
                .root(unusual_root)
                .elements(unusual_elements)
                .build();

            let gameplay_pcf = pcf::Pcf::builder()
                .version(merged_pcf.version)
                .strings(merged_pcf.strings.iter().map(|el| el.0.clone()).collect())
                .root(gameplay_root)
                .elements(gameplay_elements)
                .build();

            vec![
                ("particles/item_fx_unusuals.pcf", unusual_pcf),
                ("particles/item_fx_gameplay.pcf", gameplay_pcf),
            ]
        } else {
            vec![(target_pcf_path, merged_pcf)]
        };

        Ok(processed_pcfs)
    }
}