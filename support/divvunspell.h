#include <stdlib.h>
#include <stdbool.h>
#include <stdint.h>
#include <sys/types.h>

#pragma once

#ifdef __cplusplus
extern "C" {
#endif

#ifndef __APPLE__
#define _Nonnull
#define _Nullable
#endif

typedef void speller_t;
typedef void chfst_t;
typedef void suggest_vec_t;

extern speller_t* _Nullable
speller_archive_new(const char* _Nonnull path, char*_Nullable *_Nonnull error);

extern chfst_t* _Nullable
chfst_new(const char* _Nonnull path, char*_Nullable *_Nonnull error);

extern const char* _Nonnull
speller_get_error(uint8_t code);

extern void
speller_archive_free(speller_t* _Nonnull handle);

extern void
chfst_free(chfst_t* _Nonnull handle);

extern const char* _Nonnull
speller_meta_get_locale(speller_t* _Nonnull handle);

extern const char* _Nonnull
chfst_meta_get_locale(speller_t* _Nonnull handle);

extern void
speller_str_free(const char* _Nonnull str);

extern suggest_vec_t* _Nonnull
speller_suggest(speller_t* _Nonnull handle, const char* _Nonnull word, size_t n_best, float max_weight, float beam);

extern suggest_vec_t* _Nonnull
chfst_suggest(chfst_t* _Nonnull handle, const char* _Nonnull word, size_t n_best, float max_weight, float beam);

extern bool
speller_is_correct(speller_t* _Nonnull handle, const char* _Nonnull word);

extern bool
chfst_is_correct(chfst_t* _Nonnull handle, const char* _Nonnull word);

extern void
suggest_vec_free(suggest_vec_t* _Nonnull handle);

extern size_t
suggest_vec_len(suggest_vec_t* _Nonnull handle);

extern const char* _Nonnull
suggest_vec_get_value(suggest_vec_t* _Nonnull handle, size_t index);

extern float
suggest_vec_get_weight(suggest_vec_t* _Nonnull handle, size_t index);

extern void
suggest_vec_value_free(const char* _Nonnull value);

// const uint8_t TOKEN_OTHER = 0;
// const uint8_t TOKEN_WORD = 1;
// const uint8_t TOKEN_PUNCTUATION = 2;
// const uint8_t TOKEN_WHITESPACE = 3;

// typedef struct token_record_s {
//     uint8_t type;
//     uint32_t start;
//     uint32_t end;
//     const char *_Nonnull value;
// } token_record_t;

// typedef void tokenizer_t;

// extern tokenizer_t* _Nonnull
// speller_tokenize(const char* _Nonnull string);

// extern bool
// speller_token_next(tokenizer_t* _Nonnull handle, token_record_t* _Nonnull *_Nonnull record);

// extern void
// speller_tokenizer_free(tokenizer_t* _Nonnull handle);

typedef void word_bound_indices_t;

word_bound_indices_t* _Nonnull
word_bound_indices(const char* _Nonnull utf8_string);

bool
word_bound_indices_next(word_bound_indices_t* _Nonnull handle,
    uint64_t* _Nonnull out_index,
    char*_Nonnull *_Nullable out_string);

void
word_bound_indices_free(word_bound_indices_t* _Nonnull handle);

#ifdef __cplusplus
}
#endif