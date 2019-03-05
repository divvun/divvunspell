#include <iostream>

#include "divvunspell.hpp"

// From the project root, build with:
// 
//   $ cargo build --release
//   $ clang++ -std=c++11 -o example -Isupport/ -Ltarget/release -ldivvunspell examples/example.cpp
int main(int argc, char** argv) {
    if (argc < 3) {
        std::cout << "Usage: ./example <path-to-zhfst> <word-to-test>\n";
        return 100;
    }

    auto path = std::string(argv[1]);
    auto word = std::string(argv[2]);

    auto archive = divvunspell::SpellerArchive::create(path);

    std::cout << "Locale: " << archive->locale() << std::endl;
    std::cout << "Is correct? " << archive->isCorrect(word) << std::endl;

    auto suggestions = archive->suggest(word);

    for (auto it = suggestions.begin(); it != suggestions.end(); ++it) {
        std::cout << it->value << "\t" << it->weight << std::endl;
    }

    auto iter = word_bound_indices("this is a test string.");
    uint64_t idx = 0;
    char* string = nullptr;

    while (word_bound_indices_next(iter, &idx, &string)) {
        std::cout << idx << " " << string << std::endl;
    }

    word_bound_indices_free(iter);

    return 0;
}