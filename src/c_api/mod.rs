use libc::{c_char, size_t};
use std::ffi::{CString, CStr};

use archive::SpellerArchive;
use speller::{Speller, SpellerConfig};
use speller::suggestion::Suggestion;

// SpellerArchive

#[no_mangle]
pub extern fn speller_archive_new<'a>(raw_path: *mut c_char) -> *const SpellerArchive<'a> {
    let c_path = unsafe { CStr::from_ptr(raw_path) };
    let file_path = c_path.to_str().unwrap();

    let archive = Box::new(SpellerArchive::new(file_path));
    Box::into_raw(archive)
}

#[no_mangle]
pub extern fn speller_archive_get_speller(handle: *mut SpellerArchive) -> *const Speller {
    let archive = unsafe { &*handle };
    archive.speller()
}

#[no_mangle]
pub extern fn speller_archive_free(handle: *mut SpellerArchive) {
    unsafe { Box::from_raw(handle) };
}

// Speller

#[no_mangle]
pub extern fn speller_suggest(handle: *mut Speller, raw_word: *mut c_char, n_best: usize, beam: f32) -> *const Vec<Suggestion> {
    let c_str = unsafe { CStr::from_ptr(raw_word) };
    let word = c_str.to_str().unwrap();

    let speller = unsafe { &mut *handle };

    let suggestions = speller.suggest_with_config(&word, &SpellerConfig {
        max_weight: None,
        n_best: if n_best > 0 { Some(n_best) } else { None },
        beam: if beam > 0.0 { Some(beam) } else { None }
    });

    Box::into_raw(Box::new(suggestions))
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
