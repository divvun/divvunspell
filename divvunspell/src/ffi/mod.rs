use libc::{c_char, size_t};
use std::ffi::{CStr, CString};
use std::path::Path;
use std::ptr::null;
use std::sync::Arc;

use crate::tokenizer::word::WordBoundIndices;
use crate::tokenizer::Tokenize;

pub(crate) mod fbs;

#[no_mangle]
pub extern "C" fn divvun_word_bound_indices(
    utf8_string: *const c_char,
) -> *mut WordBoundIndices<'static> {
    let c_str = unsafe { CStr::from_ptr(utf8_string) };
    let string = c_str.to_str().unwrap();
    let iterator = string.word_bound_indices();
    Box::into_raw(Box::new(iterator)) as *mut _
}

#[no_mangle]
pub extern "C" fn divvun_word_bound_indices_next(
    iterator: *mut WordBoundIndices<'static>,
    out_index: *mut u64,
    out_string: *mut *const c_char,
) -> u8 {
    let iterator = unsafe { &mut *iterator };

    match iterator.next() {
        Some((index, word)) => {
            let c_word = CString::new(word).unwrap();
            unsafe {
                *out_index = index as u64;
                *out_string = c_word.into_raw();
            }
            1
        }
        None => 0,
    }
}

#[no_mangle]
pub extern "C" fn divvun_word_bound_indices_free(handle: *mut WordBoundIndices) {
    unsafe { Box::from_raw(handle) };
}

use std::convert::Infallible;
use cursed::{ToForeign, FromForeign, Slice};
use crate::ffi::fbs::IntoFlatbuffer;
use crate::tokenizer::{cursor_context, WordContext};

pub struct FbsMarshaler;

impl cursed::ReturnType for FbsMarshaler {
    type Foreign = Slice<u8>;

    fn foreign_default() -> Self::Foreign {
        Slice::default()
    }
}

impl<T: IntoFlatbuffer> ToForeign<T, Slice<u8>> for FbsMarshaler {
    type Error = Infallible;

    fn to_foreign(bufferable: T) -> Result<Slice<u8>, Self::Error> {
        let vec = bufferable.into_flatbuffer();
        cursed::VecMarshaler::to_foreign(vec)
    }
}

#[cthulhu::invoke(return_marshaler = "FbsMarshaler")]
pub extern "C" fn divvun_cursor_context(
    #[marshal(cursed::StrMarshaler)] first_half: &str,
    #[marshal(cursed::StrMarshaler)] second_half: &str
) -> WordContext {
    crate::tokenizer::cursor_context(first_half, second_half)
}