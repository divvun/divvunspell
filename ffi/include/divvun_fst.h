#pragma once

#ifdef __cplusplus
extern "C" {
#endif

#pragma once

#ifndef __APPLE__
#define _Nonnull
#define _Nullable
#endif

#include <stdint.h>
#include <stdbool.h>

// Rust FFI required types
typedef uint8_t rust_bool_t;
typedef uintptr_t rust_usize_t;

typedef struct rust_slice_s {
    void *_Nullable data;
    uintptr_t len;
} rust_slice_t;

// CFFI trait objects are fat pointers (data + vtable)
typedef struct cffi_trait_object_s {
    void *_Nullable data;
    void *_Nullable vtable;
} cffi_trait_object;

// Opaque types
typedef cffi_trait_object DFST_SpellerArchive;
typedef cffi_trait_object DFST_Speller;
typedef rust_slice_t DFST_VecSuggestion;
typedef void* DFST_WordIndices;

// CFFI exception callback type
typedef void (*_Nonnull cffi_exception_callback)(const uint8_t*_Nonnull msg, size_t msg_len);

// Archive functions - trait objects passed/returned by value
DFST_SpellerArchive DFST_SpellerArchive_open(
    const rust_slice_t path,
    cffi_exception_callback exception
);
DFST_Speller DFST_SpellerArchive_speller(
    DFST_SpellerArchive archive,
    cffi_exception_callback exception
);
rust_slice_t DFST_SpellerArchive_locale(
    DFST_SpellerArchive archive,
    cffi_exception_callback exception
);

// Speller functions - trait objects passed by value
rust_bool_t DFST_Speller_isCorrect(
    DFST_Speller speller,
    const rust_slice_t word,
    cffi_exception_callback exception
);
DFST_VecSuggestion DFST_Speller_suggest(
    DFST_Speller speller,
    const rust_slice_t word,
    cffi_exception_callback exception
);

// Suggestion vector functions - slices passed by value
rust_usize_t DFST_VecSuggestion_len(
    DFST_VecSuggestion suggestions,
    cffi_exception_callback exception
);
rust_slice_t DFST_VecSuggestion_getValue(
    DFST_VecSuggestion suggestions,
    size_t index,
    cffi_exception_callback exception
);
float DFST_VecSuggestion_getWeight(
    DFST_VecSuggestion suggestions,
    size_t index,
    cffi_exception_callback exception
);
uint8_t DFST_VecSuggestion_getCompleted(
    DFST_VecSuggestion suggestions,
    size_t index,
    cffi_exception_callback exception
);

// Memory management
void DFST_cstr_free(const char *_Nonnull str);
void cffi_string_free(rust_slice_t str);
void cffi_vec_free(rust_slice_t vec);

// Word indices (tokenization) functions
DFST_WordIndices _Nonnull DFST_WordIndices_new(const char*_Nonnull utf8_string);
uint8_t DFST_WordIndices_next(DFST_WordIndices _Nonnull iterator, uint64_t*_Nonnull out_index, char*_Nonnull*_Nonnull out_string);
void DFST_WordIndices_free(DFST_WordIndices _Nonnull iterator);

// Tokenizer cursor context types
typedef struct CCow_s {
    const uint8_t *_Nullable ptr;
    uintptr_t len;
    uint8_t is_owned;
} CCow;

typedef struct CRustStr_s {
    const uint8_t *_Nullable ptr;
    uintptr_t len;
} CRustStr;

typedef struct CWordContext_s {
    CCow current;
    CRustStr first_before;
    CRustStr second_before;
    CRustStr first_after;
    CRustStr second_after;
} CWordContext;

// Tokenizer cursor context functions
CWordContext DFST_Tokenizer_cursorContext(
    const uint8_t *_Nonnull first_half_ptr,
    uintptr_t first_half_len,
    const uint8_t *_Nonnull second_half_ptr,
    uintptr_t second_half_len
);
void DFST_WordContext_freeCurrent(CCow current);

#ifdef __cplusplus
}
#endif
