use libc::{c_char};
use std::ffi::{CString, CStr};

use ffi::support::{CVec};
use ffi::support::IntoCVec;
use speller::Speller;

#[no_mangle]
pub extern fn speller_vec_free(ptr: *mut CVec<*mut c_char>) {
    unsafe {
        let vec = Vec::<*mut c_char>::from_c_vec_raw(ptr);

        for c_str in vec.into_iter() {
            CString::from_raw(c_str);
        }
    }
}

#[no_mangle]
pub extern fn speller_suggest(handle: *mut Speller, raw_word: *mut c_char) -> *const CVec<*const c_char> {
    let c_str = unsafe { CStr::from_ptr(raw_word) };
    let word = c_str.to_str().unwrap();

    let speller = unsafe { &mut *handle };

    let suggestions: Vec<String> = speller.suggest(&word);
    let raw_suggestions: Vec<*mut c_char> = suggestions.into_iter()
            .map(|s| CString::new(s).unwrap().into_raw() as *mut c_char)
            .collect();

    unsafe { raw_suggestions.into_c_vec_raw() as *const _ }
}

/*
#[no_mangle]
pub extern fn speller_new() -> *const Speller {
    let x = Box::new(Speller {});
    Box::into_raw(x)
}
*/

#[no_mangle]
pub extern fn speller_free(ptr: *mut Speller) {
    unsafe { Box::from_raw(ptr) };
}
