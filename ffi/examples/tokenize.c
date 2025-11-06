#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>

// Manual FFI functions that work without cffi marshalers
extern void* DFST_WordIndices_new(const char* utf8_string);
extern uint8_t DFST_WordIndices_next(void* iterator, uint64_t* out_index, char** out_string);
extern void DFST_WordIndices_free(void* iterator);
extern void DFST_cstr_free(char* str);

int main(int argc, char *argv[]) {
    const char *text = "This is a test of the word tokenizer.";

    if (argc > 1) {
        text = argv[1];
    }

    printf("Tokenizing: \"%s\"\n\n", text);

    void* iterator = DFST_WordIndices_new(text);
    if (!iterator) {
        fprintf(stderr, "Failed to create word iterator\n");
        return 1;
    }

    printf("Words found:\n");

    uint64_t index;
    char* word;
    int count = 0;

    while (DFST_WordIndices_next(iterator, &index, &word)) {
        count++;
        printf("  %d. [%llu] %s\n", count, index, word);
        DFST_cstr_free(word);
    }

    printf("\nTotal words: %d\n", count);

    DFST_WordIndices_free(iterator);

    return 0;
}
