"""A speller's calls, its metadata and the config each call runs with."""

from __future__ import annotations

import dataclasses

import pytest

import divvun_fst
from conftest import Fixtures, Made


@pytest.fixture(scope="module")
def small(fixtures: Fixtures) -> divvun_fst.Speller:
    """The small speller, with metadata and a bundled config."""
    return divvun_fst.load_speller(fixtures.path("small.bhfst"))


@pytest.fixture(scope="module")
def bare(fixtures: Fixtures) -> divvun_fst.Speller:
    """The small speller's transducers, with no metadata and no config."""
    return divvun_fst.load_speller(fixtures.path("small-bare.bhfst"))


def test_a_speller_checks_and_corrects_words(small: divvun_fst.Speller) -> None:
    assert small.is_correct("cat")
    assert not small.is_correct("kat")
    suggestions = small.suggest("kat")
    assert [s.value for s in suggestions][:1] == ["cat"]
    assert all(isinstance(s, divvun_fst.Suggestion) for s in suggestions)
    weights = [s.weight for s in suggestions]
    assert weights == sorted(weights)


def test_a_word_with_no_letters_is_correct(small: divvun_fst.Speller) -> None:
    assert small.is_correct("")
    assert small.is_correct("123")


def test_analyses_carry_the_lexicons_forms(small: divvun_fst.Speller) -> None:
    assert [a.value for a in small.analyze_input("cat")] == ["cat"]
    assert small.analyze_input("kat") == []
    assert [a.value for a in small.analyze_output("kat")] == [s.value for s in small.suggest("kat")]


def test_metadata_says_what_the_archive_holds(small: divvun_fst.Speller) -> None:
    metadata = small.metadata
    assert metadata is not None
    assert small.locale == metadata.info.locale == "zxx"
    assert [(t.lang, t.value) for t in metadata.info.title] == [
        (None, "Test speller"),
        ("se", "Geahččalanmálle"),
    ]
    assert metadata.acceptor.type == "general"
    assert metadata.acceptor.id == "acceptor.default.thfst"
    assert metadata.acceptor.continuation is None
    assert metadata.error_model.id == "errmodel.default.thfst"
    assert metadata == small.metadata
    assert repr(metadata.info.title[1]) == "SpellerTitle(lang='se', value='Geahččalanmálle')"


def test_a_bare_archive_has_no_metadata(bare: divvun_fst.Speller) -> None:
    assert bare.metadata is None
    assert bare.locale is None
    assert bare.bundled_config is None
    assert repr(bare) == "<divvun_fst.Speller: no metadata>"


def test_the_bundled_config_comes_back_as_written(small: divvun_fst.Speller) -> None:
    assert small.bundled_config == divvun_fst.SpellerConfig(
        n_best=3,
        max_weight=30.0,
        reweight=divvun_fst.ReweightingConfig(start_penalty=1.0, end_penalty=2.0, mid_penalty=0.5),
    )
    assert repr(small) == "<divvun_fst.Speller: zxx>"


def test_no_config_runs_the_bundled_one(small: divvun_fst.Speller) -> None:
    bundled = small.bundled_config
    assert bundled is not None
    assert small.suggest("kat") == small.suggest("kat", bundled)
    assert len(small.suggest("kat")) <= 3


def test_a_given_config_is_the_whole_config(small: divvun_fst.Speller, bare: divvun_fst.Speller) -> None:
    builtin = divvun_fst.SpellerConfig()
    assert small.suggest("kat", builtin) == bare.suggest("kat")
    assert small.suggest("kat", builtin) != small.suggest("kat")


def test_a_bundled_config_varied_by_replace(small: divvun_fst.Speller) -> None:
    bundled = small.bundled_config
    assert bundled is not None
    one = dataclasses.replace(bundled, n_best=1)
    assert small.suggest("kat", one) == small.suggest("kat")[:1]


def test_a_zhfst_bundles_its_config_too(made: Made) -> None:
    speller = divvun_fst.read_speller(made.read("small.config.zhfst"))
    assert speller.bundled_config == divvun_fst.SpellerConfig(n_best=2, max_weight=30.5)
    assert len(speller.suggest("kat")) <= 2


def test_verbose_suggestions_carry_weight_details(small: divvun_fst.Speller) -> None:
    plain = small.suggest("kat")
    assert all(s.weight_details is None for s in plain)
    verbose = small.suggest("kat", divvun_fst.SpellerConfig(verbose=True))
    details = verbose[0].weight_details
    assert isinstance(details, divvun_fst.WeightDetails)
    assert details.lexicon_weight + details.mutator_weight <= verbose[0].weight
    assert repr(details).startswith("WeightDetails(lexicon_weight=")


def test_a_suggestion_is_a_plain_value(small: divvun_fst.Speller) -> None:
    [first, *_] = small.suggest("kat")
    assert first == small.suggest("kat")[0]
    assert repr(first) == f"Suggestion(value='cat', weight={first.weight!r}, completed=None, weight_details=None)"
    with pytest.raises(AttributeError):
        first.value = "dog"  # type: ignore[misc]
    with pytest.raises(TypeError):
        hash(first)


def test_a_word_must_be_a_str(small: divvun_fst.Speller) -> None:
    with pytest.raises(TypeError):
        small.suggest(b"kat")  # type: ignore[arg-type]
    with pytest.raises(TypeError):
        small.is_correct(3)  # type: ignore[arg-type]


def test_a_speller_comes_only_from_a_loader() -> None:
    with pytest.raises(TypeError):
        divvun_fst.Speller()
