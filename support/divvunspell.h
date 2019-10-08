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

#if _WIN32 
typedef wchar_t rust_path_t;
#else
typedef char rust_path_t;
#endif

// Rust error handling constructs
char* divvunspell_err = NULL;

static void divvunspell_err_callback(const char* msg) {
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

#define ERR_CALLBACK void (*exception)(const char*)
#define RET_CALLBACK(V) void (*ret)(rust_usize_t, V)


struct SpellerConfig {

};

extern const void *_Nullable
divvun_thfst_chunked_box_speller_archive_open(const rust_path_t *_Nonnull path, ERR_CALLBACK);

extern const void *_Nullable
divvun_thfst_chunked_box_speller_archive_speller(const void *_Nonnull handle, ERR_CALLBACK);

extern rust_bool_t
divvun_thfst_chunked_box_speller_is_correct(const void *_Nonnull speller, const char *_Nonnull word, ERR_CALLBACK);

extern const void *_Nullable
divvun_thfst_chunked_box_speller_suggest(const void *_Nonnull speller, const char *_Nonnull word, ERR_CALLBACK);

extern const void *_Nullable
divvun_thfst_chunked_box_speller_suggest_with_config(
    const void *_Nonnull speller,
    const char *_Nonnull word,
    struct SpellerConfig *_Nonnull config,
    ERR_CALLBACK);

extern rust_usize_t
divvun_vec_suggestion_len(const void *_Nonnull suggestions, ERR_CALLBACK);

void divvun_vec_suggestion_get_value(
    const void *_Nonnull suggestions,
    rust_usize_t index,
    ERR_CALLBACK,
    RET_CALLBACK(const char*));

#ifdef __cplusplus
}
#endif