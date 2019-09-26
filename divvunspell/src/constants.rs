pub const INDEX_TABLE_SIZE: usize = 6;
pub const TRANS_TABLE_SIZE: usize = 12;
pub const TARGET_TABLE: u32 = 2_147_483_648;

#[cfg(test)]
mod tests {
    #![allow(non_snake_case)]
    use super::*;

    #[test]
    fn test_INDEX_TABLE_SIZE() {
        use crate::types::*;
        use std::mem;

        let c = mem::size_of::<SymbolNumber>() + mem::size_of::<TransitionTableIndex>();

        assert!(INDEX_TABLE_SIZE == c);
    }

    #[test]
    fn test_TRANS_TABLE_SIZE() {
        use crate::types::*;
        use std::mem;

        let c = 2 * mem::size_of::<SymbolNumber>()
            + mem::size_of::<TransitionTableIndex>()
            + mem::size_of::<Weight>();

        assert!(TRANS_TABLE_SIZE == c);
    }
}
