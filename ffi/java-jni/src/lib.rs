use jni::errors::ThrowRuntimeExAndDefault;
use jni::objects::{JClass, JString};
use jni::strings::{JNIString, MUTF8Chars};
use jni::sys::{jboolean, jlong};
use jni::{Env, EnvUnowned, jni_str};
use std::ffi::c_char;

#[repr(C)]
struct CRustStr {
    ptr: *const u8,
    len: usize,
}

impl CRustStr {
    fn as_str(&self) -> Option<&str> {
        if self.ptr.is_null() || self.len == 0 {
            return None;
        }
        let slice = unsafe { std::slice::from_raw_parts(self.ptr, self.len) };
        std::str::from_utf8(slice).ok()
    }
}

#[repr(C)]
struct CCow {
    ptr: *const u8,
    len: usize,
    is_owned: u8,
}

/// Raw string handles captured alongside the context so that
/// `cursorContext0`'s `MUTF8Chars` buffers can be released in `freeContext`.
/// Each tuple is `(jstring, ptr, is_copy)` — the is_copy flag is required by
/// `MUTF8Chars::from_raw` in jni 0.22.
#[repr(C)]
struct CWordContext<'local> {
    current: CCow,
    first_before: CRustStr,
    second_before: CRustStr,
    first_after: CRustStr,
    second_after: CRustStr,
    _handles: (
        (JString<'local>, *const c_char, bool),
        (JString<'local>, *const c_char, bool),
    ),
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_no_divvun_fst_Tokenizer_cursorContext0<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    first_half: JString<'local>,
    second_half: JString<'local>,
) -> jlong {
    env.with_env(|env: &mut Env<'local>| -> jni::errors::Result<jlong> {
        if first_half.is_null() || second_half.is_null() {
            env.throw_new(
                jni_str!("java/lang/NullPointerException"),
                jni_str!("Input strings cannot be null"),
            )?;
            return Ok(0);
        }

        let first_java_str = match first_half.mutf8_chars(env) {
            Ok(s) => s,
            Err(e) => {
                env.throw_new(
                    jni_str!("java/lang/RuntimeException"),
                    JNIString::from(format!("Failed to get first_half JavaStr: {}", e)),
                )?;
                return Ok(0);
            }
        };

        let second_java_str = match second_half.mutf8_chars(env) {
            Ok(s) => s,
            Err(e) => {
                env.throw_new(
                    jni_str!("java/lang/RuntimeException"),
                    JNIString::from(format!("Failed to get second_half JavaStr: {}", e)),
                )?;
                return Ok(0);
            }
        };

        let first_len = first_java_str.to_bytes().len();
        let second_len = second_java_str.to_bytes().len();

        let (first_ptr, first_is_copy) = first_java_str.into_raw();
        let (second_ptr, second_is_copy) = second_java_str.into_raw();

        let first_slice = unsafe { std::slice::from_raw_parts(first_ptr as *const u8, first_len) };
        let second_slice =
            unsafe { std::slice::from_raw_parts(second_ptr as *const u8, second_len) };

        let first_str = match std::str::from_utf8(first_slice) {
            Ok(s) => s,
            Err(e) => {
                unsafe {
                    let _ = MUTF8Chars::from_raw(env, &first_half, first_ptr, first_is_copy);
                    let _ = MUTF8Chars::from_raw(env, &second_half, second_ptr, second_is_copy);
                }
                env.throw_new(
                    jni_str!("java/lang/IllegalArgumentException"),
                    JNIString::from(format!("Invalid UTF-8 in first_half: {}", e)),
                )?;
                return Ok(0);
            }
        };

        let second_str = match std::str::from_utf8(second_slice) {
            Ok(s) => s,
            Err(e) => {
                unsafe {
                    let _ = MUTF8Chars::from_raw(env, &first_half, first_ptr, first_is_copy);
                    let _ = MUTF8Chars::from_raw(env, &second_half, second_ptr, second_is_copy);
                }
                env.throw_new(
                    jni_str!("java/lang/IllegalArgumentException"),
                    JNIString::from(format!("Invalid UTF-8 in second_half: {}", e)),
                )?;
                return Ok(0);
            }
        };

        let ctx = divvun_fst::tokenizer::cursor_context(first_str, second_str);

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

        let ccow = match ctx.current.1 {
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
        };

        let c_ctx = CWordContext {
            current: ccow,
            first_before: to_rust_str(ctx.first_before),
            second_before: to_rust_str(ctx.second_before),
            first_after: to_rust_str(ctx.first_after),
            second_after: to_rust_str(ctx.second_after),
            _handles: (
                (first_half, first_ptr, first_is_copy),
                (second_half, second_ptr, second_is_copy),
            ),
        };

        Ok(Box::into_raw(Box::new(c_ctx)) as jlong)
    })
    .resolve::<ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_no_divvun_fst_Tokenizer_freeContext<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
) {
    if handle == 0 {
        return;
    }

    env.with_env(|env: &mut Env<'local>| -> jni::errors::Result<()> {
        unsafe {
            let ctx = *Box::from_raw(handle as *mut CWordContext);

            let (
                (first_jstring, first_ptr, first_is_copy),
                (second_jstring, second_ptr, second_is_copy),
            ) = ctx._handles;

            let _ = MUTF8Chars::from_raw(env, &first_jstring, first_ptr, first_is_copy);
            let _ = MUTF8Chars::from_raw(env, &second_jstring, second_ptr, second_is_copy);

            if ctx.current.is_owned != 0 && !ctx.current.ptr.is_null() {
                let _ =
                    std::mem::ManuallyDrop::into_inner(std::mem::ManuallyDrop::new(Box::from_raw(
                        std::slice::from_raw_parts_mut(ctx.current.ptr as *mut u8, ctx.current.len)
                            as *mut [u8] as *mut str,
                    )));
            }
        }
        Ok(())
    })
    .resolve::<ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_no_divvun_fst_Tokenizer_getCurrentPtr(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
) -> jlong {
    if handle == 0 {
        return 0;
    }
    let ctx = unsafe { &*(handle as *const CWordContext) };
    ctx.current.ptr as jlong
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_no_divvun_fst_Tokenizer_getCurrentLen(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
) -> jlong {
    if handle == 0 {
        return 0;
    }
    let ctx = unsafe { &*(handle as *const CWordContext) };
    ctx.current.len as jlong
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_no_divvun_fst_Tokenizer_getCurrentIsOwned(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
) -> jboolean {
    if handle == 0 {
        return false;
    }
    let ctx = unsafe { &*(handle as *const CWordContext) };
    ctx.current.is_owned != 0
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_no_divvun_fst_Tokenizer_getFirstBeforePtr(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
) -> jlong {
    if handle == 0 {
        return 0;
    }
    let ctx = unsafe { &*(handle as *const CWordContext) };
    ctx.first_before.ptr as jlong
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_no_divvun_fst_Tokenizer_getFirstBeforeLen(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
) -> jlong {
    if handle == 0 {
        return 0;
    }
    let ctx = unsafe { &*(handle as *const CWordContext) };
    ctx.first_before.len as jlong
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_no_divvun_fst_Tokenizer_getSecondBeforePtr(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
) -> jlong {
    if handle == 0 {
        return 0;
    }
    let ctx = unsafe { &*(handle as *const CWordContext) };
    ctx.second_before.ptr as jlong
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_no_divvun_fst_Tokenizer_getSecondBeforeLen(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
) -> jlong {
    if handle == 0 {
        return 0;
    }
    let ctx = unsafe { &*(handle as *const CWordContext) };
    ctx.second_before.len as jlong
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_no_divvun_fst_Tokenizer_getFirstAfterPtr(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
) -> jlong {
    if handle == 0 {
        return 0;
    }
    let ctx = unsafe { &*(handle as *const CWordContext) };
    ctx.first_after.ptr as jlong
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_no_divvun_fst_Tokenizer_getFirstAfterLen(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
) -> jlong {
    if handle == 0 {
        return 0;
    }
    let ctx = unsafe { &*(handle as *const CWordContext) };
    ctx.first_after.len as jlong
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_no_divvun_fst_Tokenizer_getSecondAfterPtr(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
) -> jlong {
    if handle == 0 {
        return 0;
    }
    let ctx = unsafe { &*(handle as *const CWordContext) };
    ctx.second_after.ptr as jlong
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_no_divvun_fst_Tokenizer_getSecondAfterLen(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
) -> jlong {
    if handle == 0 {
        return 0;
    }
    let ctx = unsafe { &*(handle as *const CWordContext) };
    ctx.second_after.len as jlong
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_no_divvun_fst_RustStr_copyBytes<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    ptr: jlong,
    len: jni::sys::jint,
) -> jni::sys::jbyteArray {
    if ptr == 0 || len <= 0 {
        return std::ptr::null_mut();
    }

    env.with_env(
        |env: &mut Env<'local>| -> jni::errors::Result<jni::sys::jbyteArray> {
            let slice = unsafe { std::slice::from_raw_parts(ptr as *const u8, len as usize) };
            Ok(env.byte_array_from_slice(slice)?.into_raw())
        },
    )
    .resolve::<ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_no_divvun_fst_RustStr_hashCode(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    ptr: jlong,
    len: jlong,
) -> jni::sys::jint {
    let rust_str = CRustStr {
        ptr: ptr as *const u8,
        len: len as usize,
    };

    match rust_str.as_str() {
        Some(s) => {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            s.hash(&mut hasher);
            hasher.finish() as jni::sys::jint
        }
        None => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_no_divvun_fst_SuggestionList_getSize(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
) -> jlong {
    if handle == 0 {
        return 0;
    }
    let suggestions =
        unsafe { &*(handle as *const Vec<divvun_fst::speller::suggestion::Suggestion>) };
    suggestions.len() as jlong
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_no_divvun_fst_SuggestionList_getValuePtr(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
    index: jlong,
) -> jlong {
    if handle == 0 {
        return 0;
    }
    let suggestions =
        unsafe { &*(handle as *const Vec<divvun_fst::speller::suggestion::Suggestion>) };
    if index < 0 || index >= suggestions.len() as jlong {
        return 0;
    }
    suggestions[index as usize].value().as_ptr() as jlong
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_no_divvun_fst_SuggestionList_getValueLen(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
    index: jlong,
) -> jlong {
    if handle == 0 {
        return 0;
    }
    let suggestions =
        unsafe { &*(handle as *const Vec<divvun_fst::speller::suggestion::Suggestion>) };
    if index < 0 || index >= suggestions.len() as jlong {
        return 0;
    }
    suggestions[index as usize].value().len() as jlong
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_no_divvun_fst_SuggestionList_getWeight(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
    index: jlong,
) -> jni::sys::jfloat {
    if handle == 0 {
        return 0.0;
    }
    let suggestions =
        unsafe { &*(handle as *const Vec<divvun_fst::speller::suggestion::Suggestion>) };
    if index < 0 || index >= suggestions.len() as jlong {
        return 0.0;
    }
    suggestions[index as usize].weight().0
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_no_divvun_fst_SuggestionList_getCompleted(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
    index: jlong,
) -> jni::sys::jbyte {
    if handle == 0 {
        return 0;
    }
    let suggestions =
        unsafe { &*(handle as *const Vec<divvun_fst::speller::suggestion::Suggestion>) };
    if index < 0 || index >= suggestions.len() as jlong {
        return 0;
    }
    match suggestions[index as usize].completed() {
        None => 0,
        Some(false) => 1,
        Some(true) => 2,
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_no_divvun_fst_SuggestionList_free(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
) {
    if handle == 0 {
        return;
    }
    unsafe {
        let _ = Box::from_raw(handle as *mut Vec<divvun_fst::speller::suggestion::Suggestion>);
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_no_divvun_fst_SpellerArchive_nativeOpen<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    path: JString<'local>,
) -> jlong {
    env.with_env(|env: &mut Env<'local>| -> jni::errors::Result<jlong> {
        if path.is_null() {
            env.throw_new(
                jni_str!("java/lang/NullPointerException"),
                jni_str!("Path cannot be null"),
            )?;
            return Ok(0);
        }

        let path_string = match path.try_to_string(env) {
            Ok(s) => s,
            Err(e) => {
                env.throw_new(
                    jni_str!("java/lang/RuntimeException"),
                    JNIString::from(format!("Failed to get path string: {}", e)),
                )?;
                return Ok(0);
            }
        };

        match divvun_fst::archive::open(std::path::Path::new(&path_string)) {
            Ok(archive) => Ok(Box::into_raw(Box::new(archive)) as jlong),
            Err(e) => {
                env.throw_new(
                    jni_str!("java/io/IOException"),
                    JNIString::from(format!("Failed to open speller archive: {}", e)),
                )?;
                Ok(0)
            }
        }
    })
    .resolve::<ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_no_divvun_fst_SpellerArchive_nativeFree(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
) {
    if handle == 0 {
        return;
    }
    unsafe {
        let _ = Box::from_raw(
            handle as *mut std::sync::Arc<dyn divvun_fst::archive::SpellerArchive + Send + Sync>,
        );
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_no_divvun_fst_SpellerArchive_nativeGetSpeller(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
) -> jlong {
    if handle == 0 {
        return 0;
    }
    let archive = unsafe {
        &*(handle as *const std::sync::Arc<dyn divvun_fst::archive::SpellerArchive + Send + Sync>)
    };
    let speller = archive.speller();
    Box::into_raw(Box::new(speller)) as jlong
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_no_divvun_fst_Speller_free(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
) {
    if handle == 0 {
        return;
    }
    unsafe {
        let _ = Box::from_raw(
            handle as *mut std::sync::Arc<dyn divvun_fst::speller::Speller + Send + Sync>,
        );
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_no_divvun_fst_Speller_isCorrect<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
    word: JString<'local>,
) -> jboolean {
    if handle == 0 || word.is_null() {
        return false;
    }

    env.with_env(|env: &mut Env<'local>| -> jni::errors::Result<jboolean> {
        let word_string = match word.try_to_string(env) {
            Ok(s) => s,
            Err(_) => return Ok(false),
        };

        let speller = unsafe {
            &*(handle as *const std::sync::Arc<dyn divvun_fst::speller::Speller + Send + Sync>)
        };

        Ok(speller.clone().is_correct(&word_string))
    })
    .resolve::<ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_no_divvun_fst_Speller_suggest<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
    word: JString<'local>,
) -> jlong {
    if handle == 0 || word.is_null() {
        return 0;
    }

    env.with_env(|env: &mut Env<'local>| -> jni::errors::Result<jlong> {
        let word_string = match word.try_to_string(env) {
            Ok(s) => s,
            Err(_) => return Ok(0),
        };

        let speller = unsafe {
            &*(handle as *const std::sync::Arc<dyn divvun_fst::speller::Speller + Send + Sync>)
        };

        let suggestions = speller.clone().suggest(&word_string);
        Ok(Box::into_raw(Box::new(suggestions)) as jlong)
    })
    .resolve::<ThrowRuntimeExAndDefault>()
}
