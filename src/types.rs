use std::{
    fmt::Display,
    ops::{Add, Div, Mul, Sub},
};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FlagDiacriticOperator {
    PositiveSet,
    NegativeSet,
    Require,
    Disallow,
    Clear,
    Unification,
}

impl std::str::FromStr for FlagDiacriticOperator {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "P" => Ok(FlagDiacriticOperator::PositiveSet),
            "N" => Ok(FlagDiacriticOperator::NegativeSet),
            "R" => Ok(FlagDiacriticOperator::Require),
            "D" => Ok(FlagDiacriticOperator::Disallow),
            "C" => Ok(FlagDiacriticOperator::Clear),
            "U" => Ok(FlagDiacriticOperator::Unification),
            _ => Err(()),
        }
    }
}

#[derive(Debug)]
pub enum HeaderFlag {
    Weighted,
    Deterministic,
    InputDeterministic,
    Minimized,
    Cyclic,
    HasEpsilonEpsilonTransitions,
    HasInputEpsilonTransitions,
    HasInputEpsilonCycles,
    HasUnweightedInputEpsilonCycles,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FlagDiacriticOperation {
    pub operation: FlagDiacriticOperator,
    pub feature: SymbolNumber,
    pub value: ValueNumber,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
#[serde(transparent)]
pub struct SymbolNumber(pub(crate) u16);

impl SymbolNumber {
    pub(crate) const ZERO: Self = SymbolNumber(0);
    pub(crate) const MAX: Self = SymbolNumber(u16::MAX);

    #[inline(always)]
    pub(crate) fn incr(&self) -> Self {
        Self(self.0 + 1)
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
#[serde(transparent)]
pub struct ValueNumber(pub(crate) i16);

impl ValueNumber {
    pub const ZERO: Self = ValueNumber(0);

    #[inline(always)]
    pub(crate) fn invert(&self) -> Self {
        ValueNumber(-self.0)
    }

    #[inline(always)]
    pub(crate) fn incr(&self) -> Self {
        ValueNumber(self.0 + 1)
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
#[serde(transparent)]
pub struct InputIndex(pub(crate) u32);

impl InputIndex {
    #[inline(always)]
    pub(crate) fn incr(&self, val: u32) -> Self {
        Self(self.0 + val)
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
#[serde(transparent)]
pub struct TransitionTableIndex(pub(crate) u32);

impl Display for TransitionTableIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Add for TransitionTableIndex {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        TransitionTableIndex(self.0 + rhs.0)
    }
}

impl Sub for TransitionTableIndex {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        TransitionTableIndex(self.0 - rhs.0)
    }
}

impl Mul for TransitionTableIndex {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self::Output {
        TransitionTableIndex(self.0 * rhs.0)
    }
}

impl Div for TransitionTableIndex {
    type Output = Self;

    fn div(self, rhs: Self) -> Self::Output {
        TransitionTableIndex(self.0 / rhs.0)
    }
}

impl TransitionTableIndex {
    pub(crate) const MAX: Self = TransitionTableIndex(u32::MAX);
    pub(crate) const ZERO: Self = TransitionTableIndex(0);
    pub(crate) const ONE: Self = TransitionTableIndex(1);

    #[inline(always)]
    pub(crate) fn incr(&self) -> Self {
        Self(self.0 + 1)
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, PartialOrd)]
#[repr(transparent)]
#[serde(transparent)]
pub struct Weight(pub f32);

impl Weight {
    pub const ZERO: Self = Weight(0.0);
    pub const MAX: Self = Weight(f32::MAX);
    pub const INFINITE: Self = Weight(f32::INFINITY);
}

impl Display for Weight {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Add for Weight {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Weight(self.0 + rhs.0)
    }
}

impl Sub for Weight {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Weight(self.0 - rhs.0)
    }
}

impl Mul for Weight {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self::Output {
        Weight(self.0 * rhs.0)
    }
}

impl Div for Weight {
    type Output = Self;

    fn div(self, rhs: Self) -> Self::Output {
        Weight(self.0 / rhs.0)
    }
}

pub type FlagDiacriticState = Vec<ValueNumber>;
pub type OperationsMap = hashbrown::HashMap<SymbolNumber, FlagDiacriticOperation>;
