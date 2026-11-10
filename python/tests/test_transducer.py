"""Lookup in a transducer, and choosing one from a box archive."""

from __future__ import annotations

import typing
from pathlib import Path

import pytest

import divvun_fst
from conftest import Fixtures


@pytest.fixture(scope="module")
def lexicon(fixtures: Fixtures) -> divvun_fst.Transducer:
    return divvun_fst.load_transducer(fixtures.path("lexicon.box"))


def test_lookup_gives_every_output_cheapest_first(lexicon: divvun_fst.Transducer) -> None:
    [cat] = lexicon.lookup("cat")
    assert isinstance(cat, divvun_fst.LookupResult)
    assert (cat.output, cat.symbols, cat.weight) == ("cat", ["c", "a", "t"], 0.0)
    assert repr(cat) == "LookupResult(output='cat', symbols=['c', 'a', 't'], weight=0.0)"
    assert lexicon.lookup("kat") == []
    assert lexicon.lookup("") == []


def test_an_error_model_gives_its_outputs(fixtures: Fixtures) -> None:
    errmodel = divvun_fst.load_transducer(fixtures.path("small-errmodel.dhfst"))
    found = errmodel.lookup("kat")
    assert "cat" in [r.output for r in found]
    weights = [r.weight for r in found]
    assert weights == sorted(weights)


def test_a_limit_keeps_the_cheapest(fixtures: Fixtures) -> None:
    errmodel = divvun_fst.load_transducer(fixtures.path("small-errmodel.dhfst"))
    every = errmodel.lookup("kat")
    assert len(every) > 2
    assert errmodel.lookup("kat", 2) == every[:2]
    assert errmodel.lookup("kat", limit=0) == []
    assert errmodel.lookup("kat", 2**70) == every


@pytest.mark.parametrize(("limit", "error"), [(-1, ValueError), (1.5, TypeError), (True, TypeError), ("1", TypeError)])
def test_a_limit_is_a_whole_number(
    lexicon: divvun_fst.Transducer, limit: object, error: type[Exception]
) -> None:
    with pytest.raises(error, match="lookup\\(\\) argument 'limit'"):
        lexicon.lookup("cat", limit)  # type: ignore[arg-type]


def test_formats_are_the_declared_literal(fixtures: Fixtures, lexicon: divvun_fst.Transducer) -> None:
    formats = typing.get_args(divvun_fst.TransducerFormat)
    assert formats == ("optimized-lookup", "thfst", "dhfst-acceptor", "dhfst-error-model")
    assert lexicon.format in formats
    assert repr(lexicon) == "<divvun_fst.Transducer: thfst>"


def load(how: str, path: Path, member: str | None = None) -> divvun_fst.Transducer:
    if how == "path":
        return divvun_fst.load_transducer(path, member)
    return divvun_fst.read_transducer(path.read_bytes(), member)


@pytest.mark.parametrize("how", ["path", "bytes"])
def test_a_box_of_several_needs_a_member(fixtures: Fixtures, how: str) -> None:
    path = fixtures.path("two-lexicons.box")
    with pytest.raises(ValueError, match=r"holds 2 transducers \(acceptor.default.thfst, second.thfst\)"):
        load(how, path)
    with pytest.raises(ValueError, match='no member "third.thfst"; it holds acceptor.default.thfst, second.thfst'):
        load(how, path, "third.thfst")
    assert load(how, path, "second.thfst").format == "thfst"


def test_a_bhfst_names_its_members(fixtures: Fixtures) -> None:
    with pytest.raises(ValueError, match="acceptor.default.dhfst, errmodel.default.dhfst"):
        divvun_fst.load_transducer(fixtures.path("small-dhfst.bhfst"))


def test_only_a_box_has_members(fixtures: Fixtures) -> None:
    with pytest.raises(ValueError, match="only a box archive has members"):
        divvun_fst.load_transducer(fixtures.path("small-acceptor.dhfst"), "acceptor")
    with pytest.raises(ValueError, match="only a box archive has members"):
        divvun_fst.read_transducer(fixtures.read("small-acceptor.dhfst"), "acceptor")


def test_a_transducer_comes_only_from_a_loader() -> None:
    with pytest.raises(TypeError):
        divvun_fst.Transducer()
