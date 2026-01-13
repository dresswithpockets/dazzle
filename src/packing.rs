use std::{collections::HashSet, ffi::CString};

use pcf::{ElementsExt, Pcf, index::ElementIdx};
use thiserror::Error;

use crate::app::App;

pub struct PcfBin {
    pub capacity: u64,
    pub name: String,
    pub pcf: Pcf,
}

pub struct PcfBinMap {
    bins: Vec<PcfBin>,
    system_names: HashSet<CString>,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("The item cannot fit into any bin in the bin map")]
    NoFit,
}

impl PcfBinMap {
    pub fn new(mut bins: Vec<PcfBin>) -> Self {
        bins.sort_by(|a, b| b.pcf.encoded_size().cmp(&a.pcf.encoded_size()));
        Self { bins, system_names: HashSet::new() }
    }

    pub fn iter(&self) -> impl Iterator<Item = &PcfBin> {
        self.bins.iter()
    }

    pub fn has_system_name(&self, name: &CString) -> bool {
        self.system_names.contains(name)
    }

    /// Pack the element in `from` at `element_idx` into a [`Pcf`] in `self`.
    /// 
    /// Uses a best-fit bin-packing algorithm to efficiently pack the element into a [`Pcf`], taking into account the
    /// size that the [`Pcf`] would increase by if the element were to be merged into it.
    /// 
    /// ## Errors
    /// 
    /// If the element can't be fit into any [`Pcf`], then [`Error::NoFit`] is returned.
    pub fn pack_group<'a>(&mut self, from: &'a Pcf, elements: &[ElementIdx]) -> Result<(), Error> {
        let mut packed = false;
        // we assume that the bins are always sorted heaviest to lightest.
        for bin in &mut self.bins {
            let estimated_size = from.encoded_group_size_in_slow(elements, &bin.pcf);
            if estimated_size > bin.capacity {
                continue;
            }

            self.system_names.extend(elements.iter().map(|idx| from.get(*idx).unwrap().name.clone()));

            let new_pcf = from.new_from_elements(elements);
            bin.pcf.merge_in(new_pcf).unwrap();

            assert_eq!(estimated_size, bin.pcf.encoded_size());

            packed = true;
        }

        if packed {
            // make sure the bins are always sorted by encoded size by descending order
            self.bins.sort_by(|a, b| b.pcf.encoded_size().cmp(&a.pcf.encoded_size()));
            Ok(())
        } else {
            Err(Error::NoFit)
        }
    }

    /// Pack the element in `from` at `element_idx` into a [`Pcf`] in `self`.
    /// 
    /// Uses a best-fit bin-packing algorithm to efficiently pack the element into a [`Pcf`], taking into account the
    /// size that the [`Pcf`] would increase by if the element were to be merged into it.
    /// 
    /// ## Errors
    /// 
    /// If the element can't be fit into any [`Pcf`], then [`Error::NoFit`] is returned.
    pub fn pack(&mut self, from: &Pcf, element_idx: ElementIdx) -> Result<(), Error> {
        let mut packed = false;
        // we assume that the bins are always sorted heaviest to lightest.
        for bin in &mut self.bins {
            let estimated_size = bin.pcf.encoded_size() + from.element_encoded_size_in(element_idx, &bin.pcf);
            if estimated_size > bin.capacity {
                continue;
            }

            let element_name = from.get(element_idx).unwrap().name.clone();
            self.system_names.insert(element_name);

            let new_pcf = Pcf::new_from_elements(from, &[element_idx]);
            
            bin.pcf.merge_in(new_pcf).unwrap();

            assert_eq!(estimated_size, bin.pcf.encoded_size());

            packed = true;
        }

        if packed {
            // make sure the bins are always sorted by encoded size by descending order
            self.bins.sort_by(|a, b| b.pcf.encoded_size().cmp(&a.pcf.encoded_size()));
            Ok(())
        } else {
            Err(Error::NoFit)
        }
    }
}
