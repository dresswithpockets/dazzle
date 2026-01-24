pub mod old;

use pcf::Pcf;
use thiserror::Error;

pub type Bins = Vec<Bin>;

#[derive(Debug)]
pub struct Bin {
    capacity: u64,
    name: String,
    data: Pcf,
}

impl Bin {
    pub fn new(capacity: u64, name: String, data: Pcf) -> Self {
        Self {
            capacity,
            name,
            data,
        }
    }

    pub fn into_inner(self) -> (String, Pcf) {
        (self.name, self.data)
    }
}

pub trait BinPack {
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
    fn pack(&mut self, from: &mut Pcf) -> Result<(), Error>;
}

impl BinPack for [Bin] {
    fn pack(&mut self, from: &mut Pcf) -> Result<(), Error> {
        let mut packed = false;
        // we assume that the bins are always sorted heaviest to lightest.
        for bin in self.iter_mut() {
            let estimated_size = bin.data.compute_merged_size(from);
            if estimated_size as u64 > bin.capacity {
                continue;
            }

            // let estimated_symbols_size = bin.data.compute_encoded_symbols_size_after_merge(from);
            // let estimated_elements_size = bin.data.compute_encoded_elements_size_after_merge(from);
            // let estimated_root_size = bin.data.compute_encoded_root_attributes_size_after_merge(from);
            // let estimated_attributes_size = bin.data.compute_encoded_attributes_size_after_merge(from);

            bin.data.merged_in(from)?;

            
            // assert_eq!(bin.data.compute_encoded_symbols_size(), estimated_symbols_size);
            // assert_eq!(bin.data.compute_encoded_elements_size(), estimated_elements_size);
            // assert_eq!(bin.data.compute_encoded_root_attributes_size(), estimated_root_size);
            // assert_eq!(bin.data.compute_encoded_attributes_size(), estimated_attributes_size);
            assert_eq!(bin.data.encoded_size(), estimated_size);

            packed = true;
            break;
        }

        if packed {
            // make sure the bins are always sorted by encoded size by descending order
            self.sort_by(|a, b| b.data.encoded_size().cmp(&a.data.encoded_size()));
            Ok(())
        } else {
            Err(Error::NoFit)
        }
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("The item cannot fit into any bin in the bin map")]
    NoFit,

    #[error(transparent)]
    CantMerge(#[from] pcf::new::MergeError),
}
