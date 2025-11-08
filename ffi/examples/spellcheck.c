#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "../include/divvun_fst.h"

// Error handling
static char* last_error = NULL;

static void error_callback(const uint8_t* msg, size_t msg_len) {
    if (last_error != NULL) {
        free(last_error);
    }
    last_error = (char*)malloc(msg_len + 1);
    memcpy(last_error, msg, msg_len);
    last_error[msg_len] = '\0';
}

static void clear_error() {
    if (last_error != NULL) {
        free(last_error);
        last_error = NULL;
    }
}

static rust_slice_t rust_cstr(const char* cstr) {
    rust_slice_t slice;
    slice.data = (void*)cstr;
    slice.len = strlen(cstr);
    return slice;
}

static bool rust_slice_to_cstr(const rust_slice_t slice, char** out_cstr) {
    *out_cstr = (char*)malloc(slice.len + 1);
    if (*out_cstr == NULL) {
        return false;
    }
    memcpy(*out_cstr, slice.data, slice.len);
    (*out_cstr)[slice.len] = '\0';
    return true;
}

int main(int argc, char *argv[]) {
    const char *archive_path = "../../se.bhfst";

    if (argc > 1) {
        archive_path = argv[1];
    }

    printf("Opening speller archive: %s\n", archive_path);

    DFST_SpellerArchive archive = DFST_SpellerArchive_open(rust_cstr(archive_path), error_callback);
    if (archive.data == NULL) {
        fprintf(stderr, "Failed to open archive: %s\n", last_error ? last_error : "unknown error");
        clear_error();
        return 1;
    }

    printf("Archive opened successfully (data=%p, vtable=%p)\n", archive.data, archive.vtable);

    DFST_Speller speller = DFST_SpellerArchive_speller(archive, error_callback);
    if (speller.data == NULL) {
        fprintf(stderr, "Failed to get speller from archive: %s\n", last_error ? last_error : "unknown error");
        clear_error();
        return 1;
    }

    printf("Speller loaded successfully\n");

    const char *test_words[] = {
        "sámegiella",  // Correct Northern Sami word
        "samegiel",    // Misspelled
        "boahtin",     // Correct
        "boatin",      // Misspelled
        NULL
    };

    for (int i = 0; test_words[i] != NULL; i++) {
        const char *word = test_words[i];
        bool is_correct = DFST_Speller_isCorrect(speller, rust_cstr(word), error_callback);

        printf("\nWord: '%s' - %s\n", word, is_correct ? "CORRECT" : "INCORRECT");

        if (!is_correct) {
            printf("  Getting suggestions...\n");
            DFST_VecSuggestion suggestions = DFST_Speller_suggest(speller, rust_cstr(word), error_callback);

            if (suggestions.data != NULL) {
                rust_usize_t len = DFST_VecSuggestion_len(suggestions, error_callback);
                printf("  Found %zu suggestions:\n", len);

                for (rust_usize_t j = 0; j < len && j < 5; j++) {
                    rust_slice_t suggestion = DFST_VecSuggestion_getValue(suggestions, j, error_callback);
                    char* suggestion_cstr = NULL;
                    if (!rust_slice_to_cstr(suggestion, &suggestion_cstr)) {
                        fprintf(stderr, "    Failed to convert suggestion to C string\n");
                        continue;
                    }
                    printf("    %zu. %s\n", j + 1, suggestion_cstr);
                    DFST_cstr_free(suggestion);
                    free(suggestion_cstr);
                }
            }
        }
    }

    printf("\nCleaning up...\n");
    clear_error();

    return 0;
}
