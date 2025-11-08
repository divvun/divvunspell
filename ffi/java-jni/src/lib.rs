use jni::objects::{JClass, JString};
use jni::strings::JavaStr;
use jni::sys::{jboolean, jlong};
use jni::JNIEnv;
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

#[repr(C)]
struct CWordContext<'local> {
    current: CCow,
    first_before: CRustStr,
    second_before: CRustStr,
    first_after: CRustStr,
    second_after: CRustStr,
    _handles: ((JString<'local>, *const c_char), (JString<'local>, *const c_char)),
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_no_divvun_fst_Tokenizer_cursorContext0<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    first_half: JString<'local>,
    second_half: JString<'local>,
) -> jlong {
    if first_half.is_null() || second_half.is_null() {
        let _ = env.throw_new("java/lang/NullPointerException", "Input strings cannot be null");
        return 0;
    }

    let first_java_str = match JavaStr::from_env(&env, &first_half) {
        Ok(s) => s,
        Err(e) => {
            let _ = env.throw_new("java/lang/RuntimeException", format!("Failed to get first_half JavaStr: {}", e));
            return 0;
        }
    };

    let second_java_str = match JavaStr::from_env(&env, &second_half) {
        Ok(s) => s,
        Err(e) => {
            let _ = env.throw_new("java/lang/RuntimeException", format!("Failed to get second_half JavaStr: {}", e));
            return 0;
        }
    };

    let first_len = first_java_str.count_bytes();
    let second_len = second_java_str.count_bytes();

    let first_ptr = first_java_str.into_raw();
    let second_ptr = second_java_str.into_raw();

    let first_slice = unsafe { std::slice::from_raw_parts(first_ptr as *const u8, first_len) };
    let second_slice = unsafe { std::slice::from_raw_parts(second_ptr as *const u8, second_len) };

    let first_str = match std::str::from_utf8(first_slice) {
        Ok(s) => s,
        Err(e) => {
            unsafe {
                let _ = JavaStr::from_raw(&env, &first_half, first_ptr);
                let _ = JavaStr::from_raw(&env, &second_half, second_ptr);
            }
            let _ = env.throw_new("java/lang/IllegalArgumentException", format!("Invalid UTF-8 in first_half: {}", e));
            return 0;
        }
    };

    let second_str = match std::str::from_utf8(second_slice) {
        Ok(s) => s,
        Err(e) => {
            unsafe {
                let _ = JavaStr::from_raw(&env, &first_half, first_ptr);
                let _ = JavaStr::from_raw(&env, &second_half, second_ptr);
            }
            let _ = env.throw_new("java/lang/IllegalArgumentException", format!("Invalid UTF-8 in second_half: {}", e));
            return 0;
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
        _handles: ((first_half, first_ptr), (second_half, second_ptr)),
    };

    Box::into_raw(Box::new(c_ctx)) as jlong
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_no_divvun_fst_Tokenizer_freeContext(
    env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    if handle == 0 {
        return;
    }

    unsafe {
        let ctx = *Box::from_raw(handle as *mut CWordContext);

        let ((first_jstring, first_ptr), (second_jstring, second_ptr)) = ctx._handles;

        let _ = JavaStr::from_raw(&env, &first_jstring, first_ptr);
        let _ = JavaStr::from_raw(&env, &second_jstring, second_ptr);

        if ctx.current.is_owned != 0 && !ctx.current.ptr.is_null() {
            let _ = std::mem::ManuallyDrop::into_inner(std::mem::ManuallyDrop::new(
                Box::from_raw(std::slice::from_raw_parts_mut(
                    ctx.current.ptr as *mut u8,
                    ctx.current.len,
                ) as *mut [u8] as *mut str),
            ));
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_no_divvun_fst_Tokenizer_getCurrentPtr(
    _env: JNIEnv,
    _class: JClass,
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
    _env: JNIEnv,
    _class: JClass,
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
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jboolean {
    if handle == 0 {
        return 0;
    }
    let ctx = unsafe { &*(handle as *const CWordContext) };
    (ctx.current.is_owned != 0) as jboolean
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_no_divvun_fst_Tokenizer_getFirstBeforePtr(
    _env: JNIEnv,
    _class: JClass,
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
    _env: JNIEnv,
    _class: JClass,
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
    _env: JNIEnv,
    _class: JClass,
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
    _env: JNIEnv,
    _class: JClass,
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
    _env: JNIEnv,
    _class: JClass,
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
    _env: JNIEnv,
    _class: JClass,
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
    _env: JNIEnv,
    _class: JClass,
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
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jlong {
    if handle == 0 {
        return 0;
    }
    let ctx = unsafe { &*(handle as *const CWordContext) };
    ctx.second_after.len as jlong
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_no_divvun_fst_RustStr_copyBytes(
    env: JNIEnv,
    _class: JClass,
    ptr: jlong,
    len: jni::sys::jint,
) -> jni::sys::jbyteArray {
    if ptr == 0 || len <= 0 {
        return std::ptr::null_mut();
    }

    let slice = unsafe { std::slice::from_raw_parts(ptr as *const u8, len as usize) };

    match env.byte_array_from_slice(slice) {
        Ok(arr) => arr.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_no_divvun_fst_RustStr_hashCode(
    _env: JNIEnv,
    _class: JClass,
    ptr: jlong,
    len: jlong,
) -> jni::sys::jint {
    let rust_str = CRustStr { ptr: ptr as *const u8, len: len as usize };

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
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jlong {
    if handle == 0 {
        return 0;
    }
    let suggestions = unsafe { &*(handle as *const Vec<divvun_fst::speller::suggestion::Suggestion>) };
    suggestions.len() as jlong
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_no_divvun_fst_SuggestionList_getValuePtr(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
    index: jlong,
) -> jlong {
    if handle == 0 {
        return 0;
    }
    let suggestions = unsafe { &*(handle as *const Vec<divvun_fst::speller::suggestion::Suggestion>) };
    if index < 0 || index >= suggestions.len() as jlong {
        return 0;
    }
    suggestions[index as usize].value().as_ptr() as jlong
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_no_divvun_fst_SuggestionList_getValueLen(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
    index: jlong,
) -> jlong {
    if handle == 0 {
        return 0;
    }
    let suggestions = unsafe { &*(handle as *const Vec<divvun_fst::speller::suggestion::Suggestion>) };
    if index < 0 || index >= suggestions.len() as jlong {
        return 0;
    }
    suggestions[index as usize].value().len() as jlong
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_no_divvun_fst_SuggestionList_getWeight(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
    index: jlong,
) -> jni::sys::jfloat {
    if handle == 0 {
        return 0.0;
    }
    let suggestions = unsafe { &*(handle as *const Vec<divvun_fst::speller::suggestion::Suggestion>) };
    if index < 0 || index >= suggestions.len() as jlong {
        return 0.0;
    }
    suggestions[index as usize].weight().0
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_no_divvun_fst_SuggestionList_getCompleted(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
    index: jlong,
) -> jni::sys::jbyte {
    if handle == 0 {
        return 0;
    }
    let suggestions = unsafe { &*(handle as *const Vec<divvun_fst::speller::suggestion::Suggestion>) };
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
    _env: JNIEnv,
    _class: JClass,
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
pub extern "system" fn Java_no_divvun_fst_SpellerArchive_nativeOpen(
    mut env: JNIEnv,
    _class: JClass,
    path: JString,
) -> jlong {
    if path.is_null() {
        let _ = env.throw_new("java/lang/NullPointerException", "Path cannot be null");
        return 0;
    }

    let path_str = match env.get_string(&path) {
        Ok(s) => s,
        Err(e) => {
            let _ = env.throw_new("java/lang/RuntimeException", format!("Failed to get path string: {}", e));
            return 0;
        }
    };

    let path_string: String = path_str.into();

    match divvun_fst::archive::open(std::path::Path::new(&path_string)) {
        Ok(archive) => Box::into_raw(Box::new(archive)) as jlong,
        Err(e) => {
            let _ = env.throw_new("java/io/IOException", format!("Failed to open speller archive: {}", e));
            0
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_no_divvun_fst_SpellerArchive_nativeFree(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    if handle == 0 {
        return;
    }
    unsafe {
        let _ = Box::from_raw(handle as *mut std::sync::Arc<dyn divvun_fst::archive::SpellerArchive + Send + Sync>);
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_no_divvun_fst_SpellerArchive_nativeGetSpeller(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jlong {
    if handle == 0 {
        return 0;
    }
    let archive = unsafe { &*(handle as *const std::sync::Arc<dyn divvun_fst::archive::SpellerArchive + Send + Sync>) };
    let speller = archive.speller();
    Box::into_raw(Box::new(speller)) as jlong
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_no_divvun_fst_Speller_free(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    if handle == 0 {
        return;
    }
    unsafe {
        let _ = Box::from_raw(handle as *mut std::sync::Arc<dyn divvun_fst::speller::Speller + Send + Sync>);
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_no_divvun_fst_Speller_isCorrect(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    word: JString,
) -> jboolean {
    if handle == 0 || word.is_null() {
        return 0;
    }

    let word_str = match env.get_string(&word) {
        Ok(s) => s,
        Err(_) => return 0,
    };

    let word_string: String = word_str.into();
    let speller = unsafe { &*(handle as *const std::sync::Arc<dyn divvun_fst::speller::Speller + Send + Sync>) };

    speller.clone().is_correct(&word_string) as jboolean
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_no_divvun_fst_Speller_suggest(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    word: JString,
) -> jlong {
    if handle == 0 || word.is_null() {
        return 0;
    }

    let word_str = match env.get_string(&word) {
        Ok(s) => s,
        Err(_) => return 0,
    };

    let word_string: String = word_str.into();
    let speller = unsafe { &*(handle as *const std::sync::Arc<dyn divvun_fst::speller::Speller + Send + Sync>) };

    let suggestions = speller.clone().suggest(&word_string);
    Box::into_raw(Box::new(suggestions)) as jlong
}
