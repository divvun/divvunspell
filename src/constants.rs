// Rust doesn't support constant expressions yet so these are guarded by a test
pub const INDEX_TABLE_SIZE: usize = 6;
pub const TRANS_TABLE_SIZE: usize = 12;
pub const TARGET_TABLE: u32 = 2147483648;

#[test]
fn test_INDEX_TABLE_SIZE() {
    use std::mem;
    use crate::types::*;

    let c = mem::size_of::<SymbolNumber>() + mem::size_of::<TransitionTableIndex>();

    assert!(INDEX_TABLE_SIZE == c);
}

#[test]
fn test_TRANS_TABLE_SIZE() {
    use std::mem;
    use crate::types::*;

    let c = 2 * mem::size_of::<SymbolNumber>() + mem::size_of::<TransitionTableIndex>() +
        mem::size_of::<Weight>();

    assert!(TRANS_TABLE_SIZE == c);
}
