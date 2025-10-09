use std::{
    fmt::Display,
    ops::{Add, Div, Mul, Sub},
};

use serde::{Deserialize, Serialize};

/// Flag diacritic operator for morphological constraints.
///
/// Flag diacritics are used in finite-state morphology to enforce complex
/// constraints during analysis and generation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FlagDiacriticOperator {
    /// Positive set - sets a feature to a value
    PositiveSet,
    /// Negative set - sets a feature to disallowed
    NegativeSet,
    /// Require - requires a feature to have a value
    Require,
    /// Disallow - requires a feature to not have a value
    Disallow,
    /// Clear - clears a feature value
    Clear,
    /// Unification - unifies feature values
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

/// Transducer header property flags.
///
/// These flags describe properties of the finite-state transducer.
#[derive(Debug)]
pub enum HeaderFlag {
    /// Transducer has weighted transitions
    Weighted,
    /// Transducer is deterministic
    Deterministic,
    /// Input side is deterministic
    InputDeterministic,
    /// Transducer is minimized
    Minimized,
    /// Transducer contains cycles
    Cyclic,
    /// Has epsilon-epsilon transitions
    HasEpsilonEpsilonTransitions,
    /// Has input epsilon transitions
    HasInputEpsilonTransitions,
    /// Has input epsilon cycles
    HasInputEpsilonCycles,
    /// Has unweighted input epsilon cycles
    HasUnweightedInputEpsilonCycles,
}

/// A flag diacritic operation in a finite-state transducer.
///
/// Combines an operation, feature, and value to enforce morphological constraints.
#[derive(Debug, Serialize, Deserialize)]
pub struct FlagDiacriticOperation {
    /// The operation to perform
    pub operation: FlagDiacriticOperator,
    /// The feature being operated on
    pub feature: SymbolNumber,
    /// The value for the feature
    pub value: ValueNumber,
}

/// Symbol number in a transducer alphabet.
///
/// Represents an index into the symbol table of a finite-state transducer.
/// Symbol 0 is typically epsilon (empty string).
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
#[serde(transparent)]
pub struct SymbolNumber(pub u16);

impl SymbolNumber {
    pub(crate) const ZERO: Self = SymbolNumber(0);
    pub(crate) const MAX: Self = SymbolNumber(u16::MAX);

    #[inline(always)]
    pub(crate) fn incr(&self) -> Self {
        Self(self.0 + 1)
    }
}

/// Value number for flag diacritics.
///
/// Represents the value assigned to a feature in flag diacritic operations.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
#[serde(transparent)]
pub struct ValueNumber(pub i16);

impl ValueNumber {
    /// Zero value constant
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

/// Index into the input string being processed.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
#[serde(transparent)]
pub struct InputIndex(pub u32);

impl InputIndex {
    #[inline(always)]
    pub(crate) fn incr(&self, val: u32) -> Self {
        Self(self.0 + val)
    }
}

/// Index into a transducer's transition table.
///
/// Identifies a specific state or transition in the finite-state transducer.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
#[serde(transparent)]
pub struct TransitionTableIndex(pub u32);

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
    pub(crate) const ONE: Self = TransitionTableIndex(1);

    #[inline(always)]
    pub(crate) fn incr(&self) -> Self {
        Self(self.0 + 1)
    }
}

/// Weight (cost) of a transducer transition.
///
/// Lower weights represent more preferred paths through the FST.
/// Used for ranking spelling suggestions and morphological analyses.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, PartialOrd)]
#[repr(transparent)]
#[serde(transparent)]
pub struct Weight(pub f32);

impl Weight {
    /// Zero weight (no cost)
    pub const ZERO: Self = Weight(0.0);
    /// Maximum finite weight
    pub const MAX: Self = Weight(f32::MAX);
    /// Infinite weight (blocked path)
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

/// State vector for flag diacritics during FST traversal.
pub type FlagDiacriticState = Vec<ValueNumber>;

/// Map from symbol numbers to their flag diacritic operations.
pub type OperationsMap = hashbrown::HashMap<SymbolNumber, FlagDiacriticOperation>;
