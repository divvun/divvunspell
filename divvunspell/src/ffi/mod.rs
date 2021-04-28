use libc::c_char;
use std::ffi::{CStr, CString};

use crate::tokenizer::{Tokenize, WordIndices};

pub(crate) mod fbs;

#[no_mangle]
pub extern "C" fn divvun_word_indices<'a>(utf8_string: *const c_char) -> *mut WordIndices<'a> {
    let c_str = unsafe { CStr::from_ptr(utf8_string) };
    let string = c_str.to_str().unwrap();
    let iterator = string.word_indices();
    Box::into_raw(Box::new(iterator)) as *mut _
}

#[no_mangle]
pub extern "C" fn divvun_word_indices_next<'a>(
    iterator: *mut WordIndices<'a>,
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
pub extern "C" fn divvun_word_indices_free<'a>(handle: *mut WordIndices<'a>) {
    unsafe { Box::from_raw(handle) };
}

#[no_mangle]
pub extern "C" fn divvun_cstr_free(handle: *mut c_char) {
    unsafe { CString::from_raw(handle) };
}

use crate::ffi::fbs::IntoFlatbuffer;
use crate::tokenizer::WordContext;
use cffi::{FromForeign, Slice, ToForeign};
use std::convert::Infallible;

pub struct FbsMarshaler;

impl cffi::ReturnType for FbsMarshaler {
    type Foreign = Slice<u8>;
    type ForeignTraitObject = ();

    fn foreign_default() -> Self::Foreign {
        Slice::default()
    }
}

impl<T: IntoFlatbuffer> ToForeign<T, Slice<u8>> for FbsMarshaler {
    type Error = Infallible;

    fn to_foreign(bufferable: T) -> Result<Slice<u8>, Self::Error> {
        let vec = bufferable.into_flatbuffer();
        cffi::VecMarshaler::to_foreign(vec)
    }
}

#[no_mangle]
pub unsafe extern "C" fn divvun_fbs_free(slice: Slice<u8>) {
    cffi::VecMarshaler::from_foreign(slice)
        .map(|_| ())
        .unwrap_or(());
}

#[cfg(feature = "logging")]
#[no_mangle]
pub unsafe extern "C" fn divvun_enable_logging() {
    env_logger::init();
}

#[cffi::marshal(return_marshaler = "FbsMarshaler")]
pub extern "C" fn divvun_cursor_context(
    #[marshal(cffi::StrMarshaler)] first_half: &str,
    #[marshal(cffi::StrMarshaler)] second_half: &str,
) -> WordContext {
    crate::tokenizer::cursor_context(first_half, second_half)
}

#[cfg(all(test, feature = "internal_ffi"))]
mod tests {
    use crate::ffi::fbs::IntoFlatbuffer;

    #[test]
    fn fbs() {
        let word_context = crate::tokenizer::cursor_context("this is some", " text");
        println!("{:?}", &word_context);

        let buf = word_context.into_flatbuffer();
        println!("{:?}", &buf);

        let word_context = crate::ffi::fbs::tokenizer::get_root_as_word_context(&buf);
        println!(
            "{:?} {:?}",
            &word_context.current().index(),
            &word_context.current().value()
        );
    }
}
