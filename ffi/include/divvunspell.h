#ifdef __cplusplus
extern "C" {
#endif

#include <stdlib.h>
#include <stdio.h>
#include <stdint.h>
#include <string.h>
#include <stdbool.h>

#pragma once

#ifndef __APPLE__
#define _Nonnull
#define _Nullable
#endif

// Rust FFI required types
typedef uint8_t rust_bool_t;
typedef uintptr_t rust_usize_t;

typedef struct rust_slice_s {
    void *_Nullable data;
    uintptr_t len;
} rust_slice_t;

#if _WIN32 
typedef wchar_t rust_path_t;
#else
typedef char rust_path_t;
#endif

// Rust error handling constructs
char*_Nullable divvunspell_err = NULL;

static void divvunspell_err_callback(const char *_Nonnull msg) {
    size_t sz = strlen(msg) + 1;
    divvunspell_err = (char*)calloc(1, sz);
    memcpy(divvunspell_err, msg, sz);
}

static void divvunspell_err_print() {
    if (divvunspell_err != NULL) {
        printf("Err: %s\n", divvunspell_err);
    }
}

static void divvunspell_err_free() {
    if (divvunspell_err != NULL) {
        free(divvunspell_err);
        divvunspell_err = NULL;
    }
}

#define ERR_CALLBACK void (*_Nonnull exception)(const char *_Nonnull)

struct CaseHandlingConfig {
    float start_penalty;
    float end_penalty;
    float mid_penalty;
};

struct SpellerConfig {
    rust_usize_t n_best;
    float max_weight;
    float beam;
    struct CaseHandlingConfig case_handling;
    rust_usize_t node_pool_size;
};

extern const void *_Nullable
divvun_thfst_chunked_box_speller_archive_open(const rust_path_t *_Nonnull path, ERR_CALLBACK);

extern const void *_Nullable
divvun_thfst_chunked_box_speller_archive_speller(const void *_Nonnull handle, ERR_CALLBACK);

extern rust_bool_t
divvun_thfst_chunked_box_speller_is_correct(const void *_Nonnull speller, const char *_Nonnull word, ERR_CALLBACK);

extern const rust_slice_t
divvun_thfst_chunked_box_speller_suggest(const void *_Nonnull speller, const char *_Nonnull word, ERR_CALLBACK);

extern const void *_Nullable
divvun_thfst_chunked_box_speller_suggest_with_config(
    const void *_Nonnull speller,
    const char *_Nonnull word,
    struct SpellerConfig *_Nonnull config,
    ERR_CALLBACK);

extern const void *_Nullable
divvun_thfst_box_speller_archive_open(const rust_path_t *_Nonnull path, ERR_CALLBACK);

extern const void *_Nullable
divvun_thfst_box_speller_archive_speller(const void *_Nonnull handle, ERR_CALLBACK);

extern rust_bool_t
divvun_thfst_box_speller_is_correct(const void *_Nonnull speller, const char *_Nonnull word, ERR_CALLBACK);

extern const rust_slice_t
divvun_thfst_box_speller_suggest(const void *_Nonnull speller, const char *_Nonnull word, ERR_CALLBACK);

extern const void *_Nullable
divvun_thfst_box_speller_suggest_with_config(
    const void *_Nonnull speller,
    const char *_Nonnull word,
    struct SpellerConfig *_Nonnull config,
    ERR_CALLBACK);

extern const void *_Nullable
divvun_hfst_zip_speller_archive_open(const rust_path_t *_Nonnull path, ERR_CALLBACK);

extern const void *_Nullable
divvun_hfst_zip_speller_archive_speller(const void *_Nonnull handle, ERR_CALLBACK);

extern const char *_Nullable
divvun_hfst_zip_speller_archive_locale(const void *_Nonnull handle, ERR_CALLBACK);

extern rust_bool_t
divvun_hfst_zip_speller_is_correct(const void *_Nonnull speller, const char *_Nonnull word, ERR_CALLBACK);

extern const rust_slice_t
divvun_hfst_zip_speller_suggest(const void *_Nonnull speller, const char *_Nonnull word, ERR_CALLBACK);

extern const void *_Nullable
divvun_hfst_zip_speller_suggest_with_config(
    const void *_Nonnull speller,
    const char *_Nonnull word,
    struct SpellerConfig *_Nonnull config,
    ERR_CALLBACK);

extern rust_usize_t
divvun_vec_suggestion_len(const rust_slice_t suggestions, ERR_CALLBACK);

extern const char *_Nullable
divvun_vec_suggestion_get_value(
    const rust_slice_t suggestions,
    rust_usize_t index,
    ERR_CALLBACK);

extern void
divvun_string_free(const char *_Nullable value);


// TODO: this is temporary until a better tokenizer impl is written
extern void *_Nonnull
word_bound_indices(const char *_Nonnull utf8_string);

extern rust_bool_t
word_bound_indices_next(const void *_Nonnull handle, uint64_t *_Nonnull out_index, char *_Nonnull *_Nonnull out_string);

extern void
word_bound_indices_free(void *_Nonnull handle);

#ifdef __cplusplus
}
#endif

