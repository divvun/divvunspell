#![allow(clippy::not_unsafe_ptr_arg_deref)]

use libc::{c_char, size_t};
use std::ffi::{CStr, CString};
use std::path::Path;
use std::ptr::null;
use std::sync::Arc;

use crate::archive::SpellerArchive;
use crate::speller::suggestion::Suggestion;
use crate::speller::{Speller, SpellerConfig};
// use crate::tokenizer::{Tokenize, Tokenizer, Token};
use crate::transducer::chunk::{ChfstBundle, ChfstTransducer};

pub struct ChfstArchive {
    speller: Arc<Speller<ChfstTransducer>>,
}

impl ChfstArchive {
    pub fn speller(&self) -> Arc<Speller<ChfstTransducer>> {
        self.speller.clone()
    }
}

// SpellerArchive

#[no_mangle]
pub extern "C" fn speller_archive_new(
    raw_path: *mut c_char,
    error: *mut *const c_char,
) -> *const SpellerArchive {
    let c_path = unsafe { CStr::from_ptr(raw_path) };
    let file_path = c_path.to_str().unwrap();

    match SpellerArchive::new(file_path) {
        Ok(v) => {
            let archive = Box::new(v);
            Box::into_raw(archive)
        }
        Err(err) => {
            if error.is_null() {
                return null();
            }

            unsafe {
                *error = CString::new(&*format!("{:?}", err)).unwrap().into_raw();
            }

            null()
        }
    }
}

#[no_mangle]
pub extern "C" fn chfst_new(
    raw_path: *mut c_char,
    error: *mut *const c_char,
) -> *const ChfstArchive {
    let c_path = unsafe { CStr::from_ptr(raw_path) };
    let file_path = c_path.to_str().unwrap();

    match ChfstBundle::from_path(Path::new(file_path)) {
        Ok(v) => Box::into_raw(Box::new(ChfstArchive {
            speller: v.speller(),
        })),
        Err(err) => {
            if error.is_null() {
                return null();
            }

            unsafe {
                *error = CString::new(&*format!("{:?}", err)).unwrap().into_raw();
            }

            null()
        }
    }
}

#[no_mangle]
pub extern "C" fn chfst_meta_get_locale(_handle: *mut Speller<ChfstTransducer>) -> *mut c_char {
    // let ar = unsafe { &*handle };
    // let locale = ar.metadata().info.locale.to_owned();
    // let s = CString::new(&*locale).unwrap();
    // s.into_raw()
    // TODO: wow.

    let s = CString::new("se").unwrap();
    s.into_raw()
}

// #[no_mangle]
// pub extern fn speller_get_error(code: u8) -> *mut c_char {
//     let s = SpellerArchiveError::from(code).to_string();

//     CString::new(s).unwrap().into_raw()
// }

#[no_mangle]
pub extern "C" fn chfst_free(handle: *mut ChfstArchive) {
    unsafe { Box::from_raw(handle) };
}

#[no_mangle]
pub extern "C" fn chfst_suggest(
    handle: *mut ChfstArchive,
    raw_word: *mut c_char,
    n_best: usize,
    max_weight: f32,
    beam: f32,
) -> *const Vec<Suggestion> {
    let c_str = unsafe { CStr::from_ptr(raw_word) };
    let word = c_str.to_str().unwrap();

    let ar = unsafe { &mut *handle };

    let suggestions = ar.speller().suggest_with_config(
        &word,
        &SpellerConfig {
            max_weight: if max_weight > 0.0 {
                Some(max_weight)
            } else {
                None
            },
            n_best: if n_best > 0 { Some(n_best) } else { None },
            beam: if beam > 0.0 { Some(beam) } else { None },
            pool_max: 128,
            pool_start: 128,
            seen_node_sample_rate: 20,
            with_caps: true,
        },
    );

    Box::into_raw(Box::new(suggestions))
}

#[no_mangle]
pub extern "C" fn chfst_is_correct(handle: *mut ChfstArchive, raw_word: *mut c_char) -> u8 {
    let c_str = unsafe { CStr::from_ptr(raw_word) };
    let word = c_str.to_str().unwrap();

    let ar = unsafe { &mut *handle };
    if ar.speller().is_correct(&word) {
        1
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn speller_meta_get_locale(handle: *mut SpellerArchive) -> *mut c_char {
    let ar = unsafe { &*handle };
    let locale = ar.metadata().info.locale.to_owned();
    let s = CString::new(&*locale).unwrap();
    s.into_raw()
}

#[no_mangle]
pub extern "C" fn speller_archive_free(handle: *mut SpellerArchive) {
    unsafe { Box::from_raw(handle) };
}

#[no_mangle]
pub extern "C" fn speller_str_free(s: *mut c_char) {
    unsafe { CString::from_raw(s) };
}

// Speller

#[no_mangle]
pub extern "C" fn speller_suggest(
    handle: *mut SpellerArchive,
    raw_word: *mut c_char,
    n_best: usize,
    max_weight: f32,
    beam: f32,
) -> *const Vec<Suggestion> {
    let c_str = unsafe { CStr::from_ptr(raw_word) };
    let word = c_str.to_str().unwrap();

    let ar = unsafe { &mut *handle };

    let suggestions = ar.speller().suggest_with_config(
        &word,
        &SpellerConfig {
            max_weight: if max_weight > 0.0 {
                Some(max_weight)
            } else {
                None
            },
            n_best: if n_best > 0 { Some(n_best) } else { None },
            beam: if beam > 0.0 { Some(beam) } else { None },
            pool_max: 128,
            pool_start: 128,
            seen_node_sample_rate: 20,
            with_caps: true,
        },
    );

    Box::into_raw(Box::new(suggestions))
}

#[no_mangle]
pub extern "C" fn speller_is_correct(handle: *mut SpellerArchive, raw_word: *mut c_char) -> u8 {
    let c_str = unsafe { CStr::from_ptr(raw_word) };
    let word = c_str.to_str().unwrap();

    let ar = unsafe { &mut *handle };
    if ar.speller().is_correct(&word) {
        1
    } else {
        0
    }
}

// Vec<Suggestion>

#[no_mangle]
pub extern "C" fn suggest_vec_free(handle: *mut Vec<Suggestion>) {
    unsafe {
        Box::from_raw(handle);
    }
}

#[no_mangle]
pub extern "C" fn suggest_vec_len(handle: &mut Vec<Suggestion>) -> size_t {
    handle.len()
}

#[no_mangle]
pub extern "C" fn suggest_vec_get_value(
    handle: &mut Vec<Suggestion>,
    index: size_t,
) -> *mut c_char {
    CString::new(handle[index].value()).unwrap().into_raw()
}

#[no_mangle]
pub extern "C" fn suggest_vec_value_free(handle: *mut c_char) {
    unsafe { CString::from_raw(handle) };
}

#[no_mangle]
pub extern "C" fn suggest_vec_get_weight(handle: &mut Vec<Suggestion>, index: size_t) -> f32 {
    handle[index].weight()
}

// Tokenizer

use crate::tokenizer::Tokenize;
use unic_segment::WordBoundIndices;

pub struct CWordBoundIndices {
    string: String,
    iterator: WordBoundIndices<'static>,
}

#[no_mangle]
pub extern "C" fn word_bound_indices(utf8_string: *const c_char) -> *mut CWordBoundIndices {
    let c_str = unsafe { CStr::from_ptr(utf8_string) };
    let string = c_str.to_str().unwrap().to_string();

    let mut thing = CWordBoundIndices {
        string,
        iterator: unsafe { std::mem::uninitialized() },
    };
    thing.iterator = unsafe { std::mem::transmute(thing.string.word_bound_indices()) };
    Box::into_raw(Box::new(thing))
}

#[no_mangle]
pub extern "C" fn word_bound_indices_next(
    handle: *mut CWordBoundIndices,
    out_index: *mut u64,
    out_string: *mut *mut c_char,
) -> bool {
    let handle = unsafe { &mut *handle };

    match handle.iterator.next() {
        Some((index, word)) => {
            unsafe { *out_index = index as u64 };
            let c_word = CString::new(word).unwrap();
            unsafe { *out_string = c_word.into_raw() };
            true
        }
        None => false,
    }
}

#[no_mangle]
pub extern "C" fn word_bound_indices_free(handle: *mut CWordBoundIndices) {
    unsafe { Box::from_raw(handle) };
}

// #[no_mangle]
// pub extern fn speller_tokenize<'a>(raw_string: *const c_char) -> *mut Tokenizer<'a> {
//     let c_str = unsafe { CStr::from_ptr(raw_string) };

//     let string = match c_str.to_str() {
//         Ok(v) => v,
//         Err(_) => return null_mut()
//     };

//     // Need it to be forgotten
//     ::std::mem::forget(string);

//     let tokenizer = Box::new(string.tokenize());
//     Box::into_raw(tokenizer)
// }

// #[repr(C)]
// #[derive(Debug)]
// pub struct TokenRecord {
//     pub ty: uint8_t,
//     pub start: uint32_t,
//     pub end: uint32_t,
//     pub value: *const c_char
// }

// impl Drop for TokenRecord {
//     fn drop(&mut self) {
//         // Drop the string
//         unsafe { CString::from_raw(self.value as *mut c_char) };
//     }
// }

// #[no_mangle]
// pub extern fn speller_token_next<'a>(handle: *mut Tokenizer<'a>, out: *mut *mut TokenRecord) -> u8 {
//     let tokenizer = unsafe { &mut *handle };

//     if !out.is_null() {
//         // Drop old ref.
//         let ptr = unsafe { *out };
//         if !ptr.is_null() {
//             unsafe { Box::from_raw(ptr); }
//         };
//     }

//     let token = match tokenizer.next() {
//         Some(v) => v,
//         None => {
//             unsafe { *out = null_mut() };
//             return 0;
//         }
//     };

//     let ty: u8 = match token {
//         Token::Word(_, _, _) => 1,
//         Token::Punctuation(_, _, _) => 2,
//         Token::Whitespace(_, _, _) => 3,
//         Token::Other(_, _, _) => 0
//     };

//     let record = TokenRecord {
//         ty,
//         start: token.start() as u32,
//         end: token.end() as u32,
//         value: CString::new(token.value()).unwrap().into_raw()
//     };

//     unsafe { *out = Box::into_raw(Box::new(record)) };
//     1
// }

// #[no_mangle]
// pub extern fn speller_tokenizer_free<'a>(handle: *mut Tokenizer<'a>) {
//     let tokenizer = unsafe { Box::from_raw(handle) };
//     drop(tokenizer.text);
// }
