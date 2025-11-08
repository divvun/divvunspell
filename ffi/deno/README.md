# divvun-fst Deno Bindings

Deno FFI bindings for the divvun-fst spell checking library.

## Requirements

- Deno 1.40 or later
- The divvun-fst shared library (built from the `ffi` crate)

## Building

First, build the FFI library:

```bash
cd ffi
cargo build  # or cargo build --release
```

## Usage

```typescript
import { SpellerArchive, tokenize } from "./mod.ts";

// Open a speller archive
const archive = new SpellerArchive("path/to/speller.bhfst");

// Get the speller
const speller = archive.speller();

// Check if a word is correct
if (speller.isCorrect("word")) {
  console.log("Correct!");
} else {
  // Get suggestions
  const suggestions = speller.suggest("word");
  for (const suggestion of suggestions) {
    console.log(`  - ${suggestion}`);
  }
}

// Tokenize text
for (const [index, word] of tokenize("This is a test")) {
  console.log(`[${index}] ${word}`);
}
```

## Running the Example

```bash
deno run --allow-ffi --allow-read ffi/deno/example.ts
# or with a custom archive:
deno run --allow-ffi --allow-read ffi/deno/example.ts path/to/speller.bhfst
```

## Permissions

The following Deno permissions are required:

- `--allow-ffi`: To load the native library
- `--allow-read`: To read speller archive files

## API Reference

### `SpellerArchive`

Opens a speller archive file.

```typescript
constructor(path: string)
```

**Methods:**

- `speller(): Speller` - Get the speller instance
- `locale(): string` - Get the locale of the speller

### `Speller`

Spell checking interface.

**Methods:**

- `isCorrect(word: string): boolean` - Check if a word is spelled correctly
- `suggest(word: string): string[]` - Get spelling suggestions for a word

### `tokenize(text: string): Array<[number, string]>`

Tokenize text into words with their byte indices.

Returns an array of `[index, word]` tuples.

### `WordIndices`

Iterator over word boundaries in a string.

```typescript
constructor(text: string)
```

**Usage:**

```typescript
using iterator = new WordIndices("some text");
for (const [index, word] of iterator) {
  console.log(index, word);
}
```

Note: `WordIndices` implements `Symbol.dispose` for automatic cleanup with the
`using` keyword.

## Error Handling

All functions may throw `Error` if an error occurs in the Rust library.

```typescript
try {
  const archive = new SpellerArchive("invalid.bhfst");
} catch (e) {
  console.error(`Error: ${e.message}`);
}
```
