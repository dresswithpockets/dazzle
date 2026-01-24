use std::{collections::HashSet, vec};

use thiserror::Error;

#[derive(Debug)]
pub struct PcfBin {
    pub capacity: u64,
    pub name: String,
    pub pcf: pcf::new::Pcf,
}

#[derive(Debug)]
pub struct PcfBinMap {
    bins: Vec<PcfBin>,
    system_names: HashSet<String>,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("The item cannot fit into any bin in the bin map")]
    NoFit,

    #[error(transparent)]
    CantMerge(#[from] pcf::new::MergeError),
}

impl IntoIterator for PcfBinMap {
    type Item = PcfBin;
    type IntoIter = vec::IntoIter<PcfBin>;

    fn into_iter(self) -> Self::IntoIter {
        self.bins.into_iter()
    }
}

impl PcfBinMap {
    pub fn new(mut bins: Vec<PcfBin>) -> Self {
        bins.sort_by(|a, b| b.pcf.encoded_size().cmp(&a.pcf.encoded_size()));
        Self {
            bins,
            system_names: HashSet::new(),
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &PcfBin> {
        self.bins.iter()
    }

    pub fn has_system_name(&self, name: &String) -> bool {
        self.system_names.contains(name)
    }

    /// Pack the new strings and elements in `from` into a [`Pcf`] in `self.`
    ///
    /// Uses a best-fit bin-packing algorithm to efficiently pack the element into a [`Pcf`], taking into account the
    /// size that the [`Pcf`] would increase by if the element were to be merged into it.
    ///
    /// ## Errors
    ///
    /// If the element can't be fit into any [`Pcf`], then [`Error::NoFit`] is returned.
    ///
    /// If there is an error when merging, then [`Error::CantMerge`] is returned.
    pub fn pack_group(&mut self, from: &mut pcf::new::Pcf) -> Result<(), Error> {
        let mut packed = false;
        // we assume that the bins are always sorted heaviest to lightest.
        for bin in &mut self.bins {
            let estimated_size = bin.pcf.compute_merged_size(from);
            if estimated_size as u64 > bin.capacity {
                continue;
            }

            for particle_system in from.particle_systems() {
                // println!("name: {}", &particle_system.name);

                if particle_system.name.starts_with('_') {
                    println!("    skipping adding to exclusivity map");
                    continue;
                }

                assert_ne!(
                    from.symbols().base.contains("DmeElement"),
                    from.symbols().base.contains("DmElement")
                );
                // assert!(self.system_names.insert(particle_system.name.clone()));
            }

            let estimated_symbols_size = bin.pcf.compute_encoded_symbols_size_after_merge(from);
            let estimated_elements_size = bin.pcf.compute_encoded_elements_size_after_merge(from);
            let estimated_root_size = bin.pcf.compute_encoded_root_attributes_size_after_merge(from);
            let estimated_attributes_size = bin.pcf.compute_encoded_attributes_size_after_merge(from);

            bin.pcf.merged_in(from)?;

            assert_eq!(bin.pcf.compute_encoded_symbols_size(), estimated_symbols_size);
            assert_eq!(bin.pcf.compute_encoded_elements_size(), estimated_elements_size);
            assert_eq!(bin.pcf.compute_encoded_root_attributes_size(), estimated_root_size);
            assert_eq!(bin.pcf.compute_encoded_attributes_size(), estimated_attributes_size);
            assert_eq!(bin.pcf.encoded_size(), estimated_size);

            packed = true;
            break;
        }

        if packed {
            // make sure the bins are always sorted by encoded size by descending order
            self.bins
                .sort_by(|a, b| b.pcf.encoded_size().cmp(&a.pcf.encoded_size()));
            Ok(())
        } else {
            Err(Error::NoFit)
        }
    }
}
