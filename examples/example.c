#include <stdio.h>
#include "../support/hfstospell.h"

// From the project root, build with:
// 
//   $ cargo build --release
//   $ clang -o example -Ltarget/release -lhfstospell examples/example.c
int main(int argc, char** argv) {
    if (argc < 3) {
        printf("Usage: ./example <path-to-zhfst> <word-to-test>\n");
        return 1;
    }

    speller_archive_t* ar = speller_archive_new(argv[1]);
    speller_t* speller = speller_archive_get_speller(ar);

    suggest_vec_t* suggs = speller_suggest(speller, argv[2], 10, 0);
    size_t len = suggest_vec_len(suggs);
    
    for (size_t i = 0; i < len; ++i) {
        const char* value = suggest_vec_get_value(suggs, i);
        float weight = suggest_vec_get_weight(suggs, i);

        printf("%12.6f %s\n", weight, value);

        suggest_vec_value_free(value);
    }

    suggest_vec_free(suggs);
    speller_archive_free(ar);

    return 0;
}