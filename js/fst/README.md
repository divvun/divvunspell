# @divvun/fst

divvunspell for JavaScript. It loads BHFST and ZHFST speller archives and
checks and corrects words with them, and looks words up in optimized-lookup,
THFST and DHFST transducers. It is divvunspell's own Rust library,
`divvun-fst`, built as WebAssembly, so it gives the results native
divvunspell gives. It runs in browsers, Web Workers, Deno, Node 20 and later,
and Bun, as one ES module and one `.wasm` file.

```sh
npm install @divvun/fst
```

## Spelling

```js
import { init, readSpeller } from "@divvun/fst";

await init();

const bytes = new Uint8Array(await (await fetch("/spellers/se.bhfst")).arrayBuffer());
const speller = readSpeller(bytes);

speller.isCorrect("sámi"); // true
speller.suggest("sami");
// [{ value: "sámi", weight: 19.12... }, { value: "Sámi", weight: 48.42... }, ...]
// The weights depend on the speller's build.

speller.locale; // "se"
speller.metadata?.info.title; // [{ lang: null, value: "..." }]

speller.free();
```

`readSpeller` takes the bytes of a BHFST archive, of either box version, with
a THFST or DHFST error model and lexicon, or of a ZHFST archive, stored or
deflated. It tells them apart by their first bytes. The bytes are copied into
the module once and read there in place.

A speller also has `analyzeInput(word)`, the lexicon's analyses of the word as
typed, with its tags, and `analyzeOutput(word)`, the suggestions with their
tags. Results are plain objects: `{ value, weight, completed? }`, with
`weightDetails` when the config sets `verbose`.

### Configuration

Every method takes an optional `SpellerConfig`, divvunspell's config under
camelCase names:

```js
speller.suggest("sami", { nBest: 3, maxWeight: 40, reweight: { midPenalty: 3 } });
```

With no config a speller runs with the config its archive bundles
(`speller.bundledConfig`), or else divvunspell's defaults
(`defaultSpellerConfig`). A config given is the whole config: a field it
leaves out takes divvunspell's default, as in a config file, not the bundled
value. To change one field of the bundled config, spread it:

```js
const tuned = speller.bundledConfig ?? defaultSpellerConfig;
speller.suggest("sami", { ...tuned, nBest: 3 });
```

| Field | Default | |
|---|---|---|
| `nBest` | `10` | the most suggestions; `null` for all |
| `maxWeight` | `10000` | the greatest weight a suggestion may have; `null` for no limit |
| `beam` | `null` | how much heavier than the best the worst suggestion may be |
| `reweight` | `{ startPenalty: 10, endPenalty: 10, midPenalty: 5, curve: null }` | penalties by where an edit falls; `null` for none |
| `recase` | `true` | keep the input's capitals, and offer the lexicon's |
| `completionMarker` | `null` | the marker for incomplete word parts |
| `astarLookahead` | `false` | order the search by the A* heuristic |
| `searchDedup` | `true` | walk states rather than paths; leave it on |
| `mutatorSubsets` | `true` | determinise the error model as the search goes; leave it on |
| `searchBudget` | `null` | how many nodes the search may examine per word |
| `wordSplitWeight` | `null` | what putting a space back into a word costs |
| `boundaryEditWeight` | `null` | what a lexicon form one separator edit away costs |
| `initialCaseWeight` | `null` | what starting a capitalised form from a lower-case letter costs |
| `verbose` | `false` | give each suggestion its `weightDetails` |

A field divvunspell does not have, or a value of the wrong type, throws
`FstError` with code `invalid-argument`, so a misspelt setting never silently
does nothing.

## Lookup

```js
import { init, readTransducer } from "@divvun/fst";

await init();

const response = await fetch("/fst/analyser-gt-desc.hfstol");
const analyser = readTransducer(new Uint8Array(await response.arrayBuffer()));
analyser.lookup("gïele");
// [{ output: "gïele+N+Sg+Nom", symbols: ["g", "ï", "e", "l", "e", "+N", "+Sg", "+Nom"], weight: 0 }]

analyser.lookup("gïele", { limit: 1 }); // the cheapest result only
analyser.free();
```

`readTransducer` takes HFST optimized lookup, a DHFST acceptor or error model,
or a box archive holding a THFST directory or one of those files. A box
holding several, such as a BHFST speller, needs the member named:

```js
const lexicon = readTransducer(bhfstBytes, { member: "acceptor.default.thfst" });
```

`lookup` gives every output the transducer has for the word, cheapest first,
then by output. The word is read as `hfst-optimized-lookup` reads it, by
longest match over the input symbols, so a generator's tags such as `+N` are
read whole. `transducer.format` says what was loaded: `optimized-lookup`,
`thfst`, `dhfst-acceptor` or `dhfst-error-model`.

## Starting the module

`init()` loads the `.wasm` file beside the package. Call it once and wait for
it before anything else; calling it again returns the same promise. It also
takes a URL, a `Response`, the module's bytes or a compiled
`WebAssembly.Module`, for bundlers and for workers that are handed one.
`version()` gives the package's version. The package is versioned on its
own, apart from the divvun-fst library inside it.

## Memory

Spellers and transducers hold WebAssembly memory. Call `free()` when done
with one, or declare it with `using`:

```js
{
  using speller = readSpeller(bytes);
  speller.suggest("sami");
} // freed here
```

An object dropped without `free()` is released after it is garbage collected.
A call on a freed object throws `FstError` with code `freed`.

## Errors

Every error is an `FstError` whose `code` code can test: `not-initialised`,
`init-failed`, `poisoned`, `freed`, `invalid-argument`, `unsupported-format`,
`invalid-format`, `internal` and `failed`. A panic inside the module traps
it; the instance is poisoned from then on and every call throws `poisoned`,
because its memory can no longer be trusted. Load the page or process again
to recover.

## Logging

The library's log events go to the console, warnings and errors only by
default. `setLogger((event) => ...)` takes them instead, each with its
`level`, `target` and `message`, and `setLogLevel("debug")` changes how much
is sent; `setLogLevel("off")` sends nothing.

## Developing

The package is built from `crates/divvun-fst-wasm` in the divvunspell
repository:

```sh
node js/build.mjs           # js/fst/dist, ready for npm publish from js/fst
node js/build.mjs --test    # and the test build, fixtures and native expectations
node js/test/run.mjs --require-all
```

The build needs the `wasm32-unknown-unknown` target, the `wasm-bindgen` CLI
at the version `Cargo.lock` pins, and `npx`; `wasm-opt` is used when it is
installed. The suite runs under Node, Bun, Deno and a headless Chromium, on a
page and in a Web Worker, and compares every result with native
divvun-fst's. It reads real spellers and analysers from the paths
`js/test/fixtures.mjs` lists, each of which an environment variable can
override; one that is missing is skipped, or fails the run under
`--require-all`.

The package's version is the one `crates/divvun-fst-wasm/Cargo.toml` and
`js/fst/package.json` both carry, apart from the library's. A
`wasm/v<version>` tag naming it releases the package to npm.

## Licence

MIT or Apache-2.0, at your option.
