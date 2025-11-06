#pragma once
#ifndef _DIVVUNSPELL_H
#define _DIVVUNSPELL_H

#include <memory>
#include <vector>
#include <string>
#include <exception>

#include "divvunspell.h"

namespace divvunspell {

struct Suggestion {
    std::string value;
    float weight;

    Suggestion(std::string value, float weight) : value(value), weight(weight) {}
};

class SpellerError : public std::exception {
    std::string message;

public:
    SpellerError(std::string message) : message(message.c_str()) {
    }

    const char* what() const throw () {
        return message.c_str();
    }
};

struct SpellerConfig {
    std::size_t nBest;
    float maxWeight;
    float beam;

    SpellerConfig(std::size_t nBest, float maxWeight, float beam) : nBest(nBest), maxWeight(maxWeight), beam(beam) {}
    SpellerConfig() : nBest(5), maxWeight(20000.0), beam(0.0) {} 
};

class SpellerArchive {
private:
    speller_t* handle;

    SpellerArchive(std::string path);
public:
    ~SpellerArchive();
    static std::shared_ptr<SpellerArchive> create(std::string path);
    std::string locale();
    bool isCorrect(std::string word);
    std::vector<Suggestion> suggest(std::string word);
    std::vector<Suggestion> suggest(std::string word, SpellerConfig config);
};

std::shared_ptr<SpellerArchive> SpellerArchive::create(std::string path) {
    return std::shared_ptr<SpellerArchive>(new SpellerArchive(path));
}

std::string SpellerArchive::locale() {
    auto c_locale = speller_meta_get_locale(handle);
    std::string locale = std::string(c_locale);
    speller_str_free(c_locale);
    return locale;
}

bool SpellerArchive::isCorrect(std::string word) {
    return speller_is_correct(handle, word.c_str());
}

std::vector<Suggestion> SpellerArchive::suggest(std::string word, SpellerConfig config) {
    auto vec_handle = speller_suggest(handle, word.c_str(), config.nBest, config.maxWeight, config.beam);
    auto len = suggest_vec_len(vec_handle);

    std::vector<Suggestion> out_vector;
    
    for (auto i = 0; i < len; ++i) {
        auto c_value = suggest_vec_get_value(vec_handle, i);
        auto weight = suggest_vec_get_weight(vec_handle, i);
        std::string value(c_value);
        suggest_vec_value_free(c_value);
        out_vector.push_back(Suggestion(value, weight));
    }

    suggest_vec_free(vec_handle);

    return out_vector;
}

std::vector<Suggestion> SpellerArchive::suggest(std::string word) {
    return suggest(word, SpellerConfig());
}

SpellerArchive::SpellerArchive(std::string path) {
    char* error = nullptr;
    speller_t* handle = speller_archive_new(path.c_str(), &error);

    if (handle == NULL) {
        auto msg = std::string(error);
        speller_str_free(error);
        throw new SpellerError(msg);
    }

    this->handle = handle;
}

SpellerArchive::~SpellerArchive() {
    speller_archive_free(handle);
}

}
#endif