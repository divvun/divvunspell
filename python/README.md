# divvun-fst

divvunspell for Python. It loads BHFST and ZHFST speller archives and checks
and corrects words with them, and looks words up in optimized-lookup, THFST
and DHFST transducers. It is divvunspell's own Rust library, `divvun-fst`, as
a native extension module, so it gives the results native divvunspell gives:
one wheel per platform for CPython 3.11 and later. It is the Python
counterpart of the npm package
[`@divvun/fst`](https://github.com/divvun/divvunspell/tree/main/js/fst).

```sh
pip install divvun-fst
```

## Spelling

```python
import divvun_fst

speller = divvun_fst.load_speller("se.bhfst")

speller.is_correct("sámi")  # True
speller.suggest("sami")
# [Suggestion(value='sámi', weight=14.58..., completed=None, weight_details=None), ...]
# The weights depend on the speller's build.

speller.locale               # 'se'
speller.metadata.info.title  # [SpellerTitle(lang=None, value='...'), ...]
```

There are two ways to load a speller:

- `divvun_fst.load_speller(path)` opens an archive and maps its members from
  the file, as the divvunspell tools do: a BHFST by the extension `.bhfst`, a
  ZHFST by `.zhfst`.
- `divvun_fst.read_speller(data)` reads the bytes of one: a BHFST of either
  box version, with a THFST or DHFST error model and lexicon, or a ZHFST,
  stored or deflated, told apart by their first bytes. The archive's members
  are read in place from one copy of the bytes.

A speller also has `analyze_input(word)`, the lexicon's analyses of the word
as typed, with its tags, and `analyze_output(word)`, the suggestions with
their tags. Results are `Suggestion`s: `value`, `weight` (lower is better),
`completed`, and `weight_details` when the config sets `verbose`. Each weight
is the 32-bit float divvunspell computed, as the Python float that holds it
exactly.

### Configuration

Every method takes an optional `SpellerConfig`, divvunspell's config with
the same fields and defaults:

```python
from divvun_fst import ReweightingConfig, SpellerConfig

config = SpellerConfig(n_best=3, max_weight=40, reweight=ReweightingConfig(mid_penalty=3))
speller.suggest("sami", config)
```

With no config a speller runs with the config its archive bundles
(`speller.bundled_config`), or else divvunspell's defaults, which
`SpellerConfig()` is. A config given is the whole config: a field it leaves
at its default takes divvunspell's default, as in a config file, not the
bundled value. To change one field of the bundled config, replace it:

```python
import dataclasses

tuned = speller.bundled_config or SpellerConfig()
speller.suggest("sami", dataclasses.replace(tuned, n_best=3))
```

| Field | Default | |
|---|---|---|
| `n_best` | `10` | the most suggestions; `None` for all |
| `max_weight` | `10000.0` | the greatest weight a suggestion may have; `None` for no limit |
| `beam` | `None` | how much heavier than the best the worst suggestion may be |
| `reweight` | `ReweightingConfig()`: start 10, end 10, mid 5, no curve | penalties by where an edit falls; `None` for none |
| `recase` | `True` | keep the input's capitals, and offer the lexicon's |
| `completion_marker` | `None` | the marker for incomplete word parts |
| `astar_lookahead` | `False` | order the search by the A* heuristic |
| `search_dedup` | `True` | walk states rather than paths; leave it on |
| `mutator_subsets` | `True` | determinise the error model as the search goes; leave it on |
| `search_budget` | `None` | how many nodes the search may examine per word |
| `word_split_weight` | `None` | what putting a space back into a word costs |
| `boundary_edit_weight` | `None` | what a lexicon form one separator edit away costs |
| `initial_case_weight` | `None` | what starting a capitalised form from a lower-case letter costs |
| `verbose` | `False` | give each suggestion its `weight_details` |

`SpellerConfig` and `ReweightingConfig` are frozen, keyword-only dataclasses,
checked when they are made: a value of the wrong type raises `TypeError`, NaN
or an infinity `ValueError` (`None` is how a field says there is no limit),
and a number out of range `OverflowError`. A field divvunspell does not have
raises `TypeError`, as for any Python class, so a misspelt setting never
silently does nothing. A config file an archive bundles has the same fields
in kebab-case: `n-best` for `n_best`.

## Lookup

```python
analyser = divvun_fst.load_transducer("analyser-gt-desc.hfstol")
analyser.lookup("gïele")
# [LookupResult(output='gïele+N+Sg+Nom', symbols=['g', 'ï', 'e', 'l', 'e', '+N', '+Sg', '+Nom'], weight=0.0)]

analyser.lookup("gïele", limit=1)  # the cheapest result only
```

`load_transducer(path)` and `read_transducer(data)` take HFST optimized
lookup, a DHFST acceptor or error model, or a box archive holding a THFST
directory or one of those files; `load_transducer` also takes a THFST
directory. A box holding several, such as a BHFST speller, needs the member
named, or raises `ValueError` naming the ones it holds:

```python
lexicon = divvun_fst.load_transducer("se.bhfst", member="acceptor.default.thfst")
```

`lookup` gives every output the transducer has for the word, cheapest first,
then by output. The word is read as `hfst-optimized-lookup` reads it, by
longest match over the input symbols, so a generator's tags such as `+N` are
read whole. `transducer.format` says what was loaded: `optimized-lookup`,
`thfst`, `dhfst-acceptor` or `dhfst-error-model`.

## Tokenizing

divvunspell's tokenizer, with offsets in code points, as Python indexes a
`str`:

```python
text = "Sámi giella, 😀 dál!"
divvun_fst.word_indices(text)        # [(0, 'Sámi'), (5, 'giella'), (15, 'dál')]
divvun_fst.word_bound_indices(text)  # every run between word boundaries, spaces and punctuation too
divvun_fst.cursor_context("Sámi gie", "lla dál")
# WordContext(current=(5, 'giella'), first_before=(0, 'Sámi'), second_before=None,
#             first_after=(12, 'dál'), second_after=None)
```

For each word `text[start:start + len(word)]` is the word. `cursor_context`
gives the word at a cursor between two halves of a text and the two words on
either side, with offsets into the two halves together.

## Threads

Loading, checking, suggesting, analysing and lookup release the GIL while the
library works. Spellers and transducers are immutable, so threads can share
one and run in parallel:

```python
from concurrent.futures import ThreadPoolExecutor

with ThreadPoolExecutor() as pool:
    corrections = list(pool.map(speller.suggest, words))
```

## Errors

A failure the library reports, such as bytes that are no speller archive or
a broken transducer, raises a `divvun_fst.FstError`, whose `code` is the one
`@divvun/fst` gives the same failure:

- `unsupported-format`: input in a format the package does not read. The
  message says what it starts with.
- `invalid-format`: input in a format it reads that is not well formed in it:
  truncated tables, a missing member, broken metadata.
- `internal`: a failure inside the library that no input should cause.
- `failed`: any other failure the library reports; the message says what.

An argument of the wrong type or value raises the `TypeError` or `ValueError`
Python raises for it, a box member that is not there among them, and a file
that cannot be opened raises the `OSError` `open()` would, such as
`FileNotFoundError`.

## Logging

The library's log events go to the standard `logging` module once the call
returns, on loggers under `divvun_fst` named after the Rust module they came
from, such as `divvun_fst.speller.worker` and `divvun_fst.archive`. Python's
own levels apply, with the library's trace level at 5. Events below the level
of the `divvun_fst` logger at the start of a call are not kept, so set that
logger's level to see more:

```python
import logging

logging.getLogger("divvun_fst").setLevel(logging.DEBUG)
```

## How it is checked

The suite loads every fixture of `@divvun/fst`'s parity check twice, by path
and from its bytes, and compares every result with native divvun-fst's,
value for value: speller results from small spellers made from the
repository's test fixtures and, where they are on the machine, the sma, se
and ga spellers, as BHFST of both box versions with THFST and DHFST members
and as ZHFST stored and deflated, under ten configs that between them set
every field; and lookups in THFST, DHFST and optimized-lookup transducers.

## Types

The package ships type stubs and the `py.typed` marker, and `mypy --strict`
checks them.

## Developing

The package is built from `crates/divvun-fst-python` in the
[divvunspell](https://github.com/divvun/divvunspell) repository, with
[maturin](https://www.maturin.rs) and [uv](https://docs.astral.sh/uv/):

```sh
cd python
uv sync --no-install-project
uv run --no-sync maturin develop --uv   # build the extension into .venv
uv run --no-sync pytest                 # the tests
uvx --with 'pytest>=8' --from 'mypy>=1.13' mypy   # the stubs and the tests
uv run --no-sync maturin build --release
```

`uv sync` uninstalls the extension `maturin develop` built, so sync first and
build after.

The tests read `@divvun/fst`'s parity fixtures from the directory
`DIVVUN_FST_FIXTURES` names, as `node js/test/fixtures.mjs <dir>` writes it,
or else run that script with node, which builds the native expectations with
cargo. Real spellers and analysers are found where `js/test/fixtures.mjs`
says, each at a path an environment variable can override; a test that needs
one that is not there is skipped, or fails when `DIVVUN_FST_REQUIRE_ALL=1`.
The package tests check the wheel `DIVVUN_FST_WHEEL` names, and that it is
the package imported, or else build one.

The package's version is the one `crates/divvun-fst-python/Cargo.toml`
carries, apart from the library's. A `python/v<version>` tag naming it
releases the wheels and the sdist to PyPI.

## Licence

MIT or Apache-2.0, at your option. The package carries both licences' text.
