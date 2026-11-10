"""A speller's config, as two immutable values."""

from __future__ import annotations

from dataclasses import dataclass, fields

from divvun_fst._divvun_fst import _check_reweighting_config, _check_speller_config


@dataclass(frozen=True, kw_only=True, slots=True, repr=False)
class ReweightingConfig:
    """Extra penalties for an edit by where in the word it falls,
    divvunspell's ReweightingConfig.

    ``curve`` decides how sharply the start and end penalties fall away into
    the word. With a number ``k`` the three penalties are anchors on the curve
    ``mid + (start - mid) * (1 - r)**k + (end - mid) * r**k``, ``r`` being the
    edit's position relative to the word's length; about 5 fits measured
    misspellings. None, the default, keeps three flat zones.
    """

    start_penalty: float = 10.0
    """What an edit at the first character costs."""
    end_penalty: float = 10.0
    """What an edit at the last character costs."""
    mid_penalty: float = 5.0
    """What an edit between them costs."""
    curve: float | None = None

    def __post_init__(self) -> None:
        _check_reweighting_config(self)

    def __repr__(self) -> str:
        return _repr(self)


@dataclass(frozen=True, kw_only=True, slots=True, repr=False)
class SpellerConfig:
    """divvunspell's SpellerConfig, field for field: what tunes the
    suggestion search. Each field's default is divvunspell's built-in one, so
    ``SpellerConfig()`` is the built-in config and a field left out takes the
    built-in value, as in a config file an archive bundles, where the fields
    are written in kebab-case (``n-best`` for ``n_best``).

    The fields are checked when the config is made: a value of the wrong type
    raises TypeError, NaN or an infinity ValueError, and a number out of
    range OverflowError. ``dataclasses.replace`` makes a variant, of the
    built-in config or of a speller's ``bundled_config``.
    """

    n_best: int | None = 10
    """The most suggestions to return; None returns every one found."""
    max_weight: float | None = 10000.0
    """The greatest weight a suggestion may have; None for no limit."""
    beam: float | None = None
    """How much heavier than the best suggestion the worst may be; None for
    no limit."""
    reweight: ReweightingConfig | None = ReweightingConfig()
    """Penalties for edits by where they fall; None adds none."""
    recase: bool = True
    """Whether to keep the input's capitals on lower-case lexicon forms, and
    to offer the lexicon's own capitals for a lower-case misspelling. It
    changes suggestions only: a lower-cased proper noun is still misspelt."""
    completion_marker: str | None = None
    """The marker for suggesting an incomplete part of a word."""
    astar_lookahead: bool = False
    """Whether the search orders itself by the A* heuristic, which needs a
    precomputation per transducer. The heuristic is admissible, so this
    changes how long the search runs, not what it finds."""
    search_dedup: bool = True
    """Whether the search walks states rather than paths. It changes what
    the search costs, not what it finds; leave it on."""
    mutator_subsets: bool = True
    """Whether the search determinises the error model as it goes. It
    changes what the search costs, not what it finds; leave it on."""
    search_budget: int | None = None
    """How many nodes the search may examine for a word before it returns
    what it has found, the cheapest first. None is no limit, which is exact
    but can take seconds for a word with no correction near it."""
    word_split_weight: float | None = None
    """What putting a space back into a run-together word costs, on top of
    the halves' lexicon weights; None never does."""
    boundary_edit_weight: float | None = None
    """What a lexicon form one inserted, deleted or replaced separator away
    costs, such as ``DNA-prosessajda`` for ``dnaprosessajda``; None never
    offers one."""
    initial_case_weight: float | None = None
    """What starting a capitalised lexicon form from a lower-case letter
    costs, on top of the edits, as in ``Skottlánda`` for ``skotlánda``; None
    never does."""
    verbose: bool = False
    """Whether each suggestion carries its ``weight_details``."""

    def __post_init__(self) -> None:
        _check_speller_config(self)

    def __repr__(self) -> str:
        return _repr(self)


def _repr(config: ReweightingConfig | SpellerConfig) -> str:
    """Names only the fields that differ from the built-in config, as a call
    that makes the config would."""
    given = ", ".join(
        f"{f.name}={getattr(config, f.name)!r}"
        for f in fields(config)
        if getattr(config, f.name) != f.default
    )
    return f"{type(config).__name__}({given})"
