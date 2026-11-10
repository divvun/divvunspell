"""The types of the extension module crates/divvun-fst-python builds, which
the divvun_fst package re-exports. Every public name is declared here, and
nothing uses Any.
"""

from os import PathLike
from typing import ClassVar, Final, Literal, TypeAlias, final

from divvun_fst._config import ReweightingConfig, SpellerConfig

__version__: Final[str]

TransducerFormat: TypeAlias = Literal[
    "optimized-lookup", "thfst", "dhfst-acceptor", "dhfst-error-model"
]

FstErrorCode: TypeAlias = Literal["unsupported-format", "invalid-format", "internal", "failed"]

class FstError(Exception):
    """A failure divvun-fst reports, such as bytes that are no speller
    archive or a broken transducer. `code` says which kind, as @divvun/fst's
    FstError does."""

    code: FstErrorCode
    """unsupported-format: input in a format the package does not read; the
    message says what it starts with. invalid-format: input in a format it
    reads that is not well formed in it, such as truncated tables, a missing
    member or broken metadata. internal: a failure inside the library that no
    input should cause. failed: any other failure the library reports."""

@final
class SpellerTitle:
    """A title in some language; `lang` is None when the archive names none."""

    __hash__: ClassVar[None]  # type: ignore[assignment]
    @property
    def lang(self) -> str | None: ...
    @property
    def value(self) -> str: ...

@final
class SpellerInfo:
    """What an archive says about its speller."""

    __hash__: ClassVar[None]  # type: ignore[assignment]
    @property
    def locale(self) -> str:
        """The language the speller is for, as a BCP 47 tag such as `se`."""
    @property
    def title(self) -> list[SpellerTitle]: ...
    @property
    def description(self) -> str: ...
    @property
    def producer(self) -> str: ...

@final
class AcceptorMetadata:
    """What an archive says about its lexicon."""

    __hash__: ClassVar[None]  # type: ignore[assignment]
    @property
    def type(self) -> str: ...
    @property
    def id(self) -> str: ...
    @property
    def title(self) -> list[SpellerTitle]: ...
    @property
    def description(self) -> str: ...
    @property
    def continuation(self) -> str | None:
        """The marker for incomplete strings, or None."""

@final
class ErrorModelMetadata:
    """What an archive says about its error model."""

    __hash__: ClassVar[None]  # type: ignore[assignment]
    @property
    def id(self) -> str: ...
    @property
    def title(self) -> list[SpellerTitle]: ...
    @property
    def description(self) -> str: ...

@final
class SpellerMetadata:
    """An archive's metadata: index.xml in a ZHFST, meta.json in a BHFST."""

    __hash__: ClassVar[None]  # type: ignore[assignment]
    @property
    def info(self) -> SpellerInfo: ...
    @property
    def acceptor(self) -> AcceptorMetadata: ...
    @property
    def error_model(self) -> ErrorModelMetadata: ...

@final
class WeightDetails:
    """How a suggestion's weight is made up."""

    __hash__: ClassVar[None]  # type: ignore[assignment]
    @property
    def lexicon_weight(self) -> float:
        """What the lexicon charged for the word form."""
    @property
    def mutator_weight(self) -> float:
        """What the error model charged for reaching it."""
    @property
    def reweight_start(self) -> float:
        """The reweighting penalty for edits at the start of the word."""
    @property
    def reweight_mid(self) -> float:
        """The reweighting penalty for edits in the middle, or -1 when the
        word has no middle."""
    @property
    def reweight_end(self) -> float:
        """The reweighting penalty for edits at the end of the word."""

@final
class Suggestion:
    """A suggestion, or an analysis, as divvunspell's Suggestion carries it.
    Each weight is the f32 divvunspell computed, as the float that holds it
    exactly."""

    __hash__: ClassVar[None]  # type: ignore[assignment]
    @property
    def value(self) -> str:
        """The suggested word form; for an analysis, the form with its tags."""
    @property
    def weight(self) -> float:
        """Its total weight: lower is better."""
    @property
    def completed(self) -> bool | None:
        """Whether it is a whole word rather than a part, where the speller
        says."""
    @property
    def weight_details(self) -> WeightDetails | None:
        """How the weight is made up, when the config sets `verbose`."""

@final
class Speller:
    """A speller loaded from a BHFST or ZHFST archive, with the archive's
    metadata and bundled config. It is immutable, so threads may use it at
    the same time, each call without the GIL. Load one with read_speller or
    load_speller.

    Every method takes an optional config. Left out, the speller runs with
    the config the archive bundles, or else divvunspell's built-in defaults.
    Given, it is the whole config: a field it leaves at its default takes the
    built-in value, not the bundled one."""

    @property
    def metadata(self) -> SpellerMetadata | None:
        """The archive's metadata, or None when it carries none, as a BHFST
        without meta.json does."""
    @property
    def locale(self) -> str | None:
        """The language the speller is for, from its metadata, or None when
        it has none."""
    @property
    def bundled_config(self) -> SpellerConfig | None:
        """The config the archive bundles, which a call passed no config runs
        with, or None when it bundles none and such a call runs with the
        built-in defaults, SpellerConfig()."""
    def is_correct(self, word: str, /, config: SpellerConfig | None = None) -> bool:
        """Whether the lexicon accepts `word`, as typed or, with `recase`, in
        the lower-case and other case variants divvunspell tries. A word with
        no letters, the empty string among them, is correct."""
    def suggest(self, word: str, /, config: SpellerConfig | None = None) -> list[Suggestion]:
        """Corrections for `word`, best first, as divvunspell's suggestion
        search finds them."""
    def analyze_input(
        self, word: str, /, config: SpellerConfig | None = None
    ) -> list[Suggestion]:
        """The lexicon's analyses of `word` as typed, with the lexicon's
        tags: no error model, no corrections. A speller whose lexicon is a
        plain word list has no tags to give."""
    def analyze_output(
        self, word: str, /, config: SpellerConfig | None = None
    ) -> list[Suggestion]:
        """The suggestions for `word`, each with the lexicon's tags left in,
        best first."""

@final
class LookupResult:
    """One output a transducer gives a word."""

    __hash__: ClassVar[None]  # type: ignore[assignment]
    @property
    def output(self) -> str:
        """The output: the text of `symbols`, in order."""
    @property
    def symbols(self) -> list[str]:
        """The output symbols, by their text. Epsilons and flag diacritics
        are left out; an identity symbol writes the symbol it read."""
    @property
    def weight(self) -> float:
        """The path's weight, arcs and final weight, as the float that holds
        its f32 exactly."""

@final
class Transducer:
    """A transducer loaded for lookup: an analyser, a generator, or a
    speller's lexicon or error model. It is immutable, so threads may look
    words up in it at the same time, each lookup without the GIL. Load one
    with read_transducer or load_transducer."""

    @property
    def format(self) -> TransducerFormat:
        """The format the transducer was read in."""
    def lookup(self, word: str, /, limit: int | None = None) -> list[LookupResult]:
        """Every output the transducer gives `word`, cheapest first, then by
        output, with no two the same in output and weight: divvun-fst's
        `transducer::lookup`, or the first `limit` of them. The word is read
        as hfst-optimized-lookup reads it, by longest match over the input
        symbols, so a generator's tags such as `+N` are read whole; text no
        symbol matches is read a grapheme cluster at a time as the unknown
        symbol, where the transducer has one, and has no results where it has
        none. Flag diacritics are obeyed."""

@final
class WordContext:
    """The word at a cursor and the words around it, from the text before
    the cursor and the text after it. Each offset is a code point offset into
    `before + after`."""

    __hash__: ClassVar[None]  # type: ignore[assignment]
    @property
    def current(self) -> tuple[int, str]:
        """The word the cursor is in or beside, whole even where the cursor
        splits it, and where it starts. Where the cursor touches no word it
        is the empty string, at the cursor."""
    @property
    def first_before(self) -> tuple[int, str] | None:
        """The word before the current one, if any."""
    @property
    def second_before(self) -> tuple[int, str] | None:
        """The word before that, if any."""
    @property
    def first_after(self) -> tuple[int, str] | None:
        """The word after the current one, if any."""
    @property
    def second_after(self) -> tuple[int, str] | None:
        """The word after that, if any."""

def read_speller(data: bytes | bytearray) -> Speller:
    """Loads the speller in a speller archive's bytes: a BHFST of either box
    version, its error model and lexicon THFST or DHFST, or a ZHFST, its
    members stored or deflated. The format is told by the first bytes, and
    the archive's members are read in place from one copy of them. Bytes that
    are no speller archive raise FstError with code "unsupported-format"; a
    broken archive raises "invalid-format" with the reason."""

def load_speller(path: str | PathLike[str]) -> Speller:
    """Loads the speller in the archive at `path`, mapping its members from
    the file as the divvunspell tools do: a BHFST by the extension `.bhfst`, a
    ZHFST by `.zhfst`. A file that cannot be opened raises what `open()`
    would."""

def read_transducer(data: bytes | bytearray, member: str | None = None) -> Transducer:
    """Loads the transducer in `data`, told by its first bytes: HFST
    optimized lookup, a DHFST acceptor or error model, or a box archive
    holding one, a THFST directory or a single file. A box holding several,
    as a BHFST does, needs `member` to say which: a THFST directory such as
    `acceptor.default.thfst`, `.` for one whose files are at the root, or a
    single file such as `acceptor.default.dhfst`."""

def load_transducer(path: str | PathLike[str], member: str | None = None) -> Transducer:
    """Loads the transducer at `path`, mapping it from the file: a THFST
    directory, or a file read as `read_transducer` reads bytes, a box
    archive's members mapped where they lie. A path that cannot be opened
    raises what `open()` would."""

def word_indices(text: str) -> list[tuple[int, str]]:
    """The words of `text`, each with the code point offset it starts at, so
    that `text[start:start + len(word)]` is the word. A word is a run between
    Unicode word boundaries with a letter or a digit in it; spaces and
    punctuation are left out."""

def word_bound_indices(text: str) -> list[tuple[int, str]]:
    """Every run of `text` between Unicode word boundaries, spaces and
    punctuation included, each with the code point offset it starts at. The
    runs make up the whole text."""

def cursor_context(before: str, after: str) -> WordContext:
    """The word at the cursor between `before` and `after`, and up to two
    words on either side of it."""

def _check_speller_config(config: SpellerConfig) -> None:
    """Checks a SpellerConfig as it is made, raising what a call reading its
    fields would raise."""

def _check_reweighting_config(config: ReweightingConfig) -> None:
    """Checks a ReweightingConfig as it is made."""

def _builtin_speller_config() -> SpellerConfig:
    """divvun-fst's SpellerConfig::default(), as a SpellerConfig, for the
    tests that hold the dataclass defaults against it."""

def _resolve_speller_config(config: SpellerConfig) -> SpellerConfig:
    """`config` read into divvun-fst's config and written back, for the tests
    that show every field reaches divvun-fst as given."""
