#include <stdlib.h>
#include <stdint.h>
#include <sys/types.h>

#ifndef _Nonnull
#define _Nonnull
#endif

typedef void speller_archive_t;
typedef void speller_t;
typedef void suggest_vec_t;

extern speller_archive_t* _Nonnull
speller_archive_new(const char* _Nonnull path);
extern speller_t* _Nonnull
speller_archive_get_speller(speller_archive_t* _Nonnull handle);
extern void
speller_archive_free(speller_archive_t* _Nonnull handle);
extern suggest_vec_t* _Nonnull
speller_suggest(speller_t* _Nonnull handle, const char* _Nonnull word, size_t n_best, float beam);
extern void
suggest_vec_free(suggest_vec_t* _Nonnull handle);
extern size_t
suggest_vec_len(suggest_vec_t* _Nonnull handle);
extern const char* _Nonnull
suggest_vec_get_value(suggest_vec_t* _Nonnull handle, size_t _Nonnull index);
extern float
suggest_vec_get_weight(suggest_vec_t* _Nonnull handle, size_t _Nonnull index);
extern void
suggest_vec_value_free(const char* _Nonnull value);
