use std::ops::{Add, AddAssign};

use derive_more::{Display, From, Into};

#[derive(From, Debug, Clone, Copy, Into, Hash, PartialEq, Eq, PartialOrd, Ord, Display)]
pub struct ElementIdx(u32);

impl ElementIdx {
    pub const INVALID: ElementIdx = ElementIdx(u32::MAX);

    pub fn is_valid(&self) -> bool {
        self.0 != u32::MAX
    }

    pub fn inner(&self) -> u32 {
        self.0
    }
}

impl From<usize> for ElementIdx {
    fn from(value: usize) -> Self {
        ElementIdx(value as u32)
    }
}

impl From<ElementIdx> for usize {
    fn from(value: ElementIdx) -> Self {
        value.0 as usize
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
