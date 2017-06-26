// Rust doesn't support constant expressions yet so these are guarded by a test
pub const TRANS_INDEX_SIZE: usize = 6;
pub const TRANS_SIZE: usize = 12;
pub const TARGET_TABLE: u32 = 2147483648;

//#[test]
fn test_trans_index_size() {
    use std::mem;
    use types::*;

    let c = mem::size_of::<SymbolNumber>() + mem::size_of::<TransitionTableIndex>();

    assert!(TRANS_INDEX_SIZE == c);
}

//#[test]
fn test_trans_size() {
    use std::mem;
    use types::*;

    let c = 2 * mem::size_of::<SymbolNumber>() + mem::size_of::<TransitionTableIndex>() + mem::size_of::<Weight>();

    assert!(TRANS_SIZE == c);
}