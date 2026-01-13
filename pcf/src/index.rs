use std::{ops::{Add, AddAssign}, u32};

use derive_more::{Display, Into};
use thiserror::Error;

#[derive(Debug, Clone, Copy, Into, Hash, PartialEq, Eq, PartialOrd, Ord, Display)]
// An index to an element in the [`Pcf`], starts at 1 and does not include the Root element.
pub struct ElementIdx(u32);

impl ElementIdx {
    pub const INVALID: ElementIdx = ElementIdx(u32::MAX);
    
    pub(crate) fn from_unchecked(value: u32) -> ElementIdx {
        ElementIdx(value)
    }

    pub fn is_valid(&self) -> bool {
        self.0 != u32::MAX
    }

    pub fn inner(&self) -> u32 {
        self.0
    }
}

#[derive(Debug, Error)]
pub enum ElementIdxError {
    #[error("the element index cannot be 0, as the root element is never referenced")]
    Zero
}

impl TryFrom<u32> for ElementIdx {
    type Error = ElementIdxError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        if value == 0 {
            Err(ElementIdxError::Zero)
        } else {
            Ok(ElementIdx(value))
        }
    }
}

impl TryFrom<&u32> for ElementIdx {
    type Error = ElementIdxError;

    fn try_from(value: &u32) -> Result<Self, Self::Error> {
        if *value == 0 {
            Err(ElementIdxError::Zero)
        } else {
            Ok(ElementIdx(*value))
        }
    }
}

impl From<usize> for ElementIdx {
    fn from(value: usize) -> Self {
        ElementIdx(value as u32 + 1)
    }
}

impl From<ElementIdx> for usize {
    fn from(value: ElementIdx) -> Self {
        value.0 as usize - 1
    }
}

impl Add<usize> for ElementIdx {
    type Output = Self;

    fn add(self, rhs: usize) -> Self::Output {
        ElementIdx(self.0 + rhs as u32)
    }
}

impl AddAssign<usize> for ElementIdx {
    fn add_assign(&mut self, rhs: usize) {
        self.0 += rhs as u32
    }
}