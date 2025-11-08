#![allow(non_snake_case)]

use cffi::{FromForeign, ToForeign};
use libc::c_char;
use std::convert::Infallible;
use std::ffi::{CStr, CString};
use std::sync::Arc;

use divvun_fst::archive::{SpellerArchive, error::SpellerArchiveError};
use divvun_fst::speller::{ReweightingConfig, Speller, SpellerConfig, suggestion::Suggestion};
use divvun_fst::tokenizer::{Tokenize, WordIndices};
use divvun_fst::types::Weight;

#[repr(C)]
pub struct CRustStr {
    pub ptr: *const u8,
    pub len: usize,
}

#[repr(C)]
pub struct CCow {
    pub ptr: *const u8,
    pub len: usize,
    pub is_owned: u8,
}

impl<'a> From<std::borrow::Cow<'a, str>> for CCow {
    fn from(cow: std::borrow::Cow<'a, str>) -> Self {
        match cow {
            std::borrow::Cow::Borrowed(s) => CCow {
                ptr: s.as_ptr(),
                len: s.len(),
                is_owned: 0,
            },
            std::borrow::Cow::Owned(s) => {
                let s = std::mem::ManuallyDrop::new(s.into_boxed_str());
                CCow {
                    ptr: s.as_ptr(),
                    len: s.len(),
                    is_owned: 1,
                }
            }
        }
    }
}

#[repr(C)]
pub struct CWordContext {
    pub current: CCow,
    pub first_before: CRustStr,
    pub second_before: CRustStr,
    pub first_after: CRustStr,
    pub second_after: CRustStr,
}

#[unsafe(no_mangle)]
pub extern "C" fn DFST_WordIndices_new<'a>(utf8_string: *const c_char) -> *mut WordIndices<'a> {
    let c_str = unsafe { CStr::from_ptr(utf8_string) };
    let string = c_str.to_str().unwrap();
    let iterator = string.word_indices();
    Box::into_raw(Box::new(iterator)) as *mut _
}

#[unsafe(no_mangle)]
pub extern "C" fn DFST_WordIndices_next<'a>(
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

#[unsafe(no_mangle)]
pub extern "C" fn DFST_WordIndices_free<'a>(handle: *mut WordIndices<'a>) {
    drop(unsafe { Box::from_raw(handle) });
}

#[unsafe(no_mangle)]
pub extern "C" fn DFST_cstr_free(handle: *mut c_char) {
    drop(unsafe { CString::from_raw(handle) });
}


// Note: This function returns CRustStr pointers that reference the input strings.
// The caller MUST keep first_half and second_half alive for the lifetime of the returned CWordContext.
// WARNING: The current field may be owned (is_owned=1). Call DFST_WordContext_freeCurrent() to free.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn DFST_Tokenizer_cursorContext(
    first_half_ptr: *const u8,
    first_half_len: usize,
    second_half_ptr: *const u8,
    second_half_len: usize,
) -> CWordContext {
    let first_half = unsafe {
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(first_half_ptr, first_half_len))
    };
    let second_half = unsafe {
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(second_half_ptr, second_half_len))
    };

    let ctx = divvun_fst::tokenizer::cursor_context(first_half, second_half);

    fn to_rust_str(opt: Option<(usize, &str)>) -> CRustStr {
        opt.map(|(_, word)| CRustStr {
            ptr: word.as_ptr(),
            len: word.len(),
        })
        .unwrap_or(CRustStr {
            ptr: std::ptr::null(),
            len: 0,
        })
    }

    CWordContext {
        current: CCow::from(ctx.current.1),
        first_before: to_rust_str(ctx.first_before),
        second_before: to_rust_str(ctx.second_before),
        first_after: to_rust_str(ctx.first_after),
        second_after: to_rust_str(ctx.second_after),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn DFST_WordContext_freeCurrent(current: CCow) {
    if current.is_owned != 0 && !current.ptr.is_null() {
        unsafe {
            let _ = std::mem::ManuallyDrop::into_inner(std::mem::ManuallyDrop::new(
                Box::from_raw(std::slice::from_raw_parts_mut(
                    current.ptr as *mut u8,
                    current.len,
                ) as *mut [u8] as *mut str),
            ));
        }
    }
}

pub type SuggestionVecMarshaler = cffi::VecMarshaler<Suggestion>;
pub type SuggestionVecRefMarshaler = cffi::VecRefMarshaler<Suggestion>;

#[derive(Clone, Copy, Default, PartialEq)]
#[repr(C)]
pub struct FfiReweightingConfig {
    start_penalty: f32,
    end_penalty: f32,
    mid_penalty: f32,
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct FfiSpellerConfig {
    pub n_best: usize,
    pub max_weight: Weight,
    pub beam: Weight,
    pub reweight: FfiReweightingConfig,
    pub node_pool_size: usize,
}

pub struct SpellerConfigMarshaler;

impl cffi::InputType for SpellerConfigMarshaler {
    type Foreign = *const std::ffi::c_void;
    type ForeignTraitObject = ();
}

impl cffi::ReturnType for SpellerConfigMarshaler {
    type Foreign = *const std::ffi::c_void;
    type ForeignTraitObject = ();

    fn foreign_default() -> Self::Foreign {
        std::ptr::null()
    }
}

impl ToForeign<SpellerConfig, *const std::ffi::c_void> for SpellerConfigMarshaler {
    type Error = Infallible;

    fn to_foreign(config: SpellerConfig) -> Result<*const std::ffi::c_void, Self::Error> {
        let reweight = config
            .reweight
            .map(|c| FfiReweightingConfig {
                start_penalty: c.start_penalty,
                end_penalty: c.end_penalty,
                mid_penalty: c.mid_penalty,
            })
            .unwrap_or_else(|| FfiReweightingConfig::default());

        let out = FfiSpellerConfig {
            n_best: config.n_best.unwrap_or(0),
            max_weight: config.max_weight.unwrap_or(Weight::ZERO),
            beam: config.beam.unwrap_or(Weight::ZERO),
            reweight,
            node_pool_size: config.node_pool_size,
        };

        Ok(Box::into_raw(Box::new(out)) as *const _)
    }
}

impl FromForeign<*const std::ffi::c_void, SpellerConfig> for SpellerConfigMarshaler {
    type Error = Infallible;

    unsafe fn from_foreign(ptr: *const std::ffi::c_void) -> Result<SpellerConfig, Self::Error> {
        if ptr.is_null() {
            return Ok(SpellerConfig::default());
        }

        let config: &FfiSpellerConfig = unsafe { &*ptr.cast() };

        let reweight = if config.reweight == FfiReweightingConfig::default() {
            None
        } else {
            let c = config.reweight;
            Some(ReweightingConfig {
                start_penalty: c.start_penalty,
                end_penalty: c.end_penalty,
                mid_penalty: c.mid_penalty,
            })
        };

        let out = SpellerConfig {
            n_best: if config.n_best > 0 {
                Some(config.n_best)
            } else {
                None
            },
            max_weight: if config.max_weight > Weight::ZERO {
                Some(config.max_weight)
            } else {
                None
            },
            beam: if config.beam > Weight::ZERO {
                Some(config.beam)
            } else {
                None
            },
            reweight,
            node_pool_size: config.node_pool_size,
            recase: true,
            completion_marker: None,
        };

        Ok(out)
    }
}

#[cffi::marshal]
pub extern "C" fn DFST_Speller_isCorrect(
    #[marshal(cffi::ArcRefMarshaler::<dyn Speller + Sync + Send>)] speller: Arc<
        dyn Speller + Sync + Send,
    >,
    #[marshal(cffi::StrMarshaler)] word: &str,
) -> bool {
    speller.is_correct(word)
}

#[cffi::marshal(return_marshaler = "SuggestionVecMarshaler")]
pub extern "C" fn DFST_Speller_suggest(
    #[marshal(cffi::ArcRefMarshaler::<dyn Speller + Sync + Send>)] speller: Arc<
        dyn Speller + Sync + Send,
    >,
    #[marshal(cffi::StrMarshaler)] word: &str,
) -> Vec<Suggestion> {
    speller.suggest(word)
}

#[cffi::marshal(return_marshaler = "SuggestionVecMarshaler")]
pub extern "C" fn DFST_Speller_suggestWithConfig(
    #[marshal(cffi::ArcRefMarshaler::<dyn Speller + Sync + Send>)] speller: Arc<
        dyn Speller + Sync + Send,
    >,
    #[marshal(cffi::StrMarshaler)] word: &str,
    #[marshal(SpellerConfigMarshaler)] config: SpellerConfig,
) -> Vec<Suggestion> {
    speller.suggest_with_config(word, &config)
}

#[cffi::marshal]
pub extern "C" fn DFST_VecSuggestion_len(
    #[marshal(SuggestionVecRefMarshaler)] suggestions: &[Suggestion],
) -> usize {
    suggestions.len()
}

#[cffi::marshal(return_marshaler = "cffi::StringMarshaler")]
pub extern "C" fn DFST_VecSuggestion_getValue(
    #[marshal(SuggestionVecRefMarshaler)] suggestions: &[Suggestion],
    index: usize,
) -> String {
    suggestions[index].value().to_string()
}

#[cffi::marshal(return_marshaler = cffi::ArcMarshaler::<dyn SpellerArchive + Send + Sync>)]
pub extern "C" fn DFST_SpellerArchive_open(
    #[marshal(cffi::PathBufMarshaler)] path: std::path::PathBuf,
) -> Result<Arc<dyn SpellerArchive + Send + Sync>, Box<dyn std::error::Error>> {
    divvun_fst::archive::open(&path).map_err(|e| Box::new(e) as _)
}

#[cffi::marshal(return_marshaler = "cffi::ArcMarshaler::<dyn Speller + Send + Sync>")]
pub extern "C" fn DFST_SpellerArchive_speller(
    #[marshal(cffi::ArcRefMarshaler::<dyn SpellerArchive + Send + Sync>)] handle: Arc<
        dyn SpellerArchive + Send + Sync,
    >,
) -> Arc<dyn Speller + Send + Sync> {
    handle.speller()
}

#[cffi::marshal(return_marshaler = "cffi::StringMarshaler")]
pub extern "C" fn DFST_SpellerArchive_locale(
    #[marshal(cffi::ArcRefMarshaler::<dyn SpellerArchive + Send + Sync>)] handle: Arc<
        dyn SpellerArchive + Send + Sync,
    >,
) -> Result<String, Box<dyn std::error::Error>> {
    match handle.metadata() {
        Some(v) => Ok(v.info().locale().to_string()),
        None => Err(Box::new(SpellerArchiveError::NoMetadata) as _),
    }
}
