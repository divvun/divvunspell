use libc::{c_char, size_t, uint8_t, uint32_t};
use std::ffi::{CString, CStr};
use std::ptr::{null, null_mut};

use crate::archive::{SpellerArchive, SpellerArchiveError};
use crate::speller::SpellerConfig;
use crate::speller::suggestion::Suggestion;
use crate::tokenizer::{Tokenize, Tokenizer, Token};

// SpellerArchive

#[no_mangle]
pub extern fn speller_archive_new<'a>(raw_path: *mut c_char, error: *mut u8) -> *const SpellerArchive<'a> {
    let c_path = unsafe { CStr::from_ptr(raw_path) };
    let file_path = c_path.to_str().unwrap();

    match SpellerArchive::new(file_path) {
        Ok(v) => {
            if !error.is_null() {
                unsafe { *error = 0; }
            }
            
            let archive = Box::new(v);
            Box::into_raw(archive)
        },
        Err(err) => {
            if error.is_null() {
                return null();
            }

            let code = match err {
                SpellerArchiveError::Io(_) => 1,
                SpellerArchiveError::UnsupportedCompressed => 2
            };

            unsafe { *error = code; }

            null()
        }
    }
}

#[no_mangle]
pub extern fn speller_get_error(code: u8) -> *mut c_char {
    let s = match code {
        0 => "An IO error occurred. Does the file exist at the specified path?",
        1 => "The provided file is compressed and cannot be memory mapped. Rezip with no compression.",
        _ => {
            let m = format!("Unknown error code {}.", code);
            return CString::new(m).unwrap().into_raw();
        }
    };

    CString::new(s).unwrap().into_raw()
}

#[no_mangle]
pub extern fn speller_meta_get_locale(handle: *mut SpellerArchive) -> *mut c_char {
    let ar = unsafe { &*handle };
    let locale = ar.metadata().info.locale.to_owned();
    let s = CString::new(&*locale).unwrap();
    s.into_raw()
}

#[no_mangle]
pub extern fn speller_archive_free(handle: *mut SpellerArchive) {
    unsafe { Box::from_raw(handle) };
}

#[no_mangle]
pub extern fn speller_str_free(s: *mut c_char) {
    unsafe { CString::from_raw(s) };
}

// Speller

#[no_mangle]
pub extern fn speller_suggest(handle: *mut SpellerArchive, raw_word: *mut c_char, n_best: usize, beam: f32) -> *const Vec<Suggestion> {
    let c_str = unsafe { CStr::from_ptr(raw_word) };
    let word = c_str.to_str().unwrap();

    let ar = unsafe { &mut *handle };

    let suggestions = ar.speller().suggest_with_config(&word, &SpellerConfig {
        max_weight: None,
        n_best: if n_best > 0 { Some(n_best) } else { None },
        beam: if beam > 0.0 { Some(beam) } else { None }
    });

    Box::into_raw(Box::new(suggestions))
}

#[no_mangle]
pub extern fn speller_is_correct(handle: *mut SpellerArchive, raw_word: *mut c_char) -> uint8_t {
    let c_str = unsafe { CStr::from_ptr(raw_word) };
    let word = c_str.to_str().unwrap();

    let ar = unsafe { &mut *handle };
    if ar.speller().is_correct(&word) { 1 } else { 0 }
}

// Vec<Suggestion>

#[no_mangle]
pub extern fn suggest_vec_free(handle: *mut Vec<Suggestion>) {
    unsafe { Box::from_raw(handle); }
}

#[no_mangle]
pub extern fn suggest_vec_len(handle: &mut Vec<Suggestion>) -> size_t {
    handle.len()
}

#[no_mangle]
pub extern fn suggest_vec_get_value(handle: &mut Vec<Suggestion>, index: size_t) -> *mut c_char {
    CString::new(handle[index].value()).unwrap().into_raw()
}

#[no_mangle]
pub extern fn suggest_vec_value_free(handle: *mut c_char) {
    unsafe { CString::from_raw(handle) };
}

#[no_mangle]
pub extern fn suggest_vec_get_weight(handle: &mut Vec<Suggestion>, index: size_t) -> f32 {
    handle[index].weight()
}

// Tokenizer

#[no_mangle]
pub extern fn speller_tokenize<'a>(raw_string: *const c_char) -> *mut Tokenizer<'a> {
    let c_str = unsafe { CStr::from_ptr(raw_string) };

    let string = match c_str.to_str() {
        Ok(v) => v,
        Err(_) => return null_mut()
    };

    // Need it to be forgotten
    ::std::mem::forget(string);

    let tokenizer = Box::new(string.tokenize());
    Box::into_raw(tokenizer)
}

#[repr(C)]
#[derive(Debug)]
pub struct TokenRecord {
    pub ty: uint8_t,
    pub start: uint32_t,
    pub end: uint32_t,
    pub value: *const c_char
}

impl Drop for TokenRecord {
    fn drop(&mut self) {
        // Drop the string
        unsafe { CString::from_raw(self.value as *mut c_char) };
    }
}

#[no_mangle]
pub extern fn speller_token_next<'a>(handle: *mut Tokenizer<'a>, out: *mut *mut TokenRecord) -> u8 {
    let tokenizer = unsafe { &mut *handle };

    if !out.is_null() {
        // Drop old ref.
        let ptr = unsafe { *out };
        if !ptr.is_null() {
            unsafe { Box::from_raw(ptr); }
        };
    }

    let token = match tokenizer.next() {
        Some(v) => v,
        None => {
            unsafe { *out = null_mut() };
            return 0;
        }
    };

    let ty: u8 = match token {
        Token::Word(_, _, _) => 1,
        Token::Punctuation(_, _, _) => 2,
        Token::Whitespace(_, _, _) => 3,
        Token::Other(_, _, _) => 0
    };
 
    let record = TokenRecord {
        ty,
        start: token.start() as u32,
        end: token.end() as u32,
        value: CString::new(token.value()).unwrap().into_raw()
    };

    unsafe { *out = Box::into_raw(Box::new(record)) };
    1
}

#[no_mangle]
pub extern fn speller_tokenizer_free<'a>(handle: *mut Tokenizer<'a>) {
    let tokenizer = unsafe { Box::from_raw(handle) };
    drop(tokenizer.text);
}