#!/usr/bin/env python3
"""Example usage of divvun-fst Python bindings."""

import sys
from pathlib import Path
from divvun_fst import SpellerArchive, tokenize


def main():
    archive_path = Path(__file__).parent.parent.parent / "se.bhfst"

    if len(sys.argv) > 1:
        archive_path = Path(sys.argv[1])

    if not archive_path.exists():
        print(f"Error: Archive not found at {archive_path}", file=sys.stderr)
        print("Usage: python example.py [path/to/archive.bhfst]", file=sys.stderr)
        sys.exit(1)

    print(f"Opening speller archive: {archive_path}")
    archive = SpellerArchive(str(archive_path))

    try:
        locale = archive.locale()
        print(f"Archive locale: {locale}")
    except RuntimeError as e:
        print(f"Could not get locale: {e}")

    speller = archive.speller()
    print("Speller loaded successfully\n")

    test_words = [
        "sámegiella",  # Correct Northern Sami word
        "samegiel",    # Misspelled
        "boahtin",     # Correct
        "boatin",      # Misspelled
    ]

    for word in test_words:
        is_correct = speller.is_correct(word)
        status = "CORRECT" if is_correct else "INCORRECT"
        print(f"Word: '{word}' - {status}")

        if not is_correct:
            suggestions = speller.suggest(word)
            print(f"  Found {len(suggestions)} suggestions:")
            for i, suggestion in enumerate(suggestions[:5], 1):
                print(f"    {i}. {suggestion}")
        print()

    print("\nTokenization example:")
    text = "This is a test of the word tokenizer."
    print(f"Text: \"{text}\"")
    print("Words found:")
    for i, (index, word) in enumerate(tokenize(text), 1):
        print(f"  {i}. [{index}] {word}")


if __name__ == "__main__":
    main()
