#include <stdio.h>
#include "../support/hfstospell.h"

// From the project root, build with:
// 
//   $ cargo build --release
//   $ clang -o example -Ltarget/release -lhfstospell examples/example.c
int main(int argc, char** argv) {
    tokenizer_t* iter = speller_tokenize("This is an example string.");

    token_record_t* record = NULL;

    printf("Record.\n");
    while (speller_token_next(iter, &record)) {
        printf("TOKEN %d %d %d %s\n", record->type, record->start, record->end, record->value);
    }

    speller_tokenizer_free(iter);

    if (argc < 3) {
        printf("Usage: ./example <path-to-zhfst> <word-to-test>\n");
        return 100;
    }

    printf("I: Archive loading\n");

    uint8_t error_code = 0;
    speller_t* speller = speller_archive_new(argv[1], &error_code);

    printf("I: error code %d\n", error_code);

    if (error_code != 0) {
        const char* msg = speller_get_error(error_code);
        printf("Error: %s\n", msg);
        // speller_str_free(msg);
        return 1;
    }

    if (speller == NULL) {
        printf("Error: archive is null\n");
        return 1;
    }

    printf("I: Archive loaded, getting locale\n");

    const char* locale = speller_meta_get_locale(speller);
    printf("Locale: %s\n", locale);
    // speller_str_free(locale);

    bool is_correct = speller_is_correct(speller, argv[2]);
    printf("Is correct? %s\n", is_correct ? "Yes" : "No");

    printf("I: Generating suggestions\n");
    suggest_vec_t* suggs = speller_suggest(speller, argv[2], 10, 0);

    printf("I: Getting suggestion length\n");
    size_t len = suggest_vec_len(suggs);
    
    for (size_t i = 0; i < len; ++i) {
        const char* value = suggest_vec_get_value(suggs, i);
        float weight = suggest_vec_get_weight(suggs, i);

        printf("%12.6f %s\n", weight, value);

        suggest_vec_value_free(value);
    }

    suggest_vec_free(suggs);
    speller_archive_free(speller);

    return 0;
}