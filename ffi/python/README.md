# divvun-fst Python Bindings

Python ctypes bindings for the divvun-fst spell checking library.

## Requirements

- Python 3.7 or later
- The divvun-fst shared library (built from the `ffi` crate)

## Building

First, build the FFI library:

```bash
cd ffi
cargo build  # or cargo build --release
```

## Usage

```python
from divvun_fst import SpellerArchive, tokenize

# Open a speller archive
archive = SpellerArchive("path/to/speller.bhfst")

# Get the speller
speller = archive.speller()

# Check if a word is correct
if speller.is_correct("word"):
    print("Correct!")
else:
    # Get suggestions
    suggestions = speller.suggest("word")
    for suggestion in suggestions:
        print(f"  - {suggestion}")

# Tokenize text
for index, word in tokenize("This is a test"):
    print(f"[{index}] {word}")
```

## Running the Example

```bash
python ffi/python/example.py
# or with a custom archive:
python ffi/python/example.py path/to/speller.bhfst
```

## API Reference

### `SpellerArchive(path: str)`

Opens a speller archive file.

- **Methods:**
  - `speller() -> Speller`: Get the speller instance
  - `locale() -> str`: Get the locale of the speller

### `Speller`

Spell checking interface.

- **Methods:**
  - `is_correct(word: str) -> bool`: Check if a word is spelled correctly
  - `suggest(word: str) -> List[str]`: Get spelling suggestions for a word

### `tokenize(text: str) -> List[Tuple[int, str]]`

Tokenize text into words with their byte indices.

Returns a list of `(index, word)` tuples.

### `WordIndices(text: str)`

Iterator over word boundaries in a string.

- **Usage:**
  ```python
  for index, word in WordIndices("some text"):
      print(index, word)
  ```

## Error Handling

All functions may raise `RuntimeError` if an error occurs in the Rust library.

```python
try:
    archive = SpellerArchive("invalid.bhfst")
except RuntimeError as e:
    print(f"Error: {e}")
```
