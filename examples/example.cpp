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

    return 0;
}