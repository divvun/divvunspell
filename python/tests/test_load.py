"""Loading every format, by path and from bytes, and the same results either
way: BHFST, ZHFST stored and deflated, THFST directories, HFST optimized
lookup and DHFST."""

from __future__ import annotations

import shutil
from pathlib import Path

import pytest

import divvun_fst
from conftest import REPOSITORY_FIXTURES, Fixtures, Made, NativeSpeller
from test_parity import LOADS, configs, lookup_json, suggestions_json

WORDS = ["cat", "kat", "cet", "kare", "Kat", "xyz", ""]


def same_as_native(speller: divvun_fst.Speller, native: NativeSpeller, fixtures: Fixtures) -> None:
    """Asserts that `speller` gives every result `native` holds, metadata
    and bundled config aside."""
    config = configs(fixtures)
    for result in native["results"]:
        given = config(result["config"])
        word = result["word"]
        assert speller.is_correct(word, given) == result["isCorrect"], word
        assert suggestions_json(speller.suggest(word, given)) == result["suggest"], word
        assert suggestions_json(speller.analyze_output(word, given)) == result["analyzeOutput"], word
    for analysis in native["analyses"]:
        found = speller.analyze_input(analysis["word"], config(analysis["config"]))
        assert suggestions_json(found) == analysis["analyzeInput"], analysis["word"]


@pytest.mark.parametrize("load", LOADS)
@pytest.mark.parametrize("name", ["small.bhfst", "small-bare.bhfst", "small-dhfst.bhfst"])
def test_bhfst_loads_by_path_and_from_bytes(fixtures: Fixtures, load: str, name: str) -> None:
    speller = LOADS[load](fixtures, name)
    same_as_native(speller, fixtures.speller(name), fixtures)


@pytest.mark.parametrize("name", ["small.zhfst", "small.stored.zhfst"])
def test_zhfst_stored_and_deflated_load_alike(fixtures: Fixtures, made: Made, name: str) -> None:
    # The archive holds the THFST fixtures of small-bare.bhfst as optimized
    # lookup, so it gives the native results for that speller.
    native = fixtures.speller("small-bare.bhfst")
    for speller in [divvun_fst.load_speller(made.path(name)), divvun_fst.read_speller(made.read(name))]:
        assert speller.locale == "zxx"
        assert speller.bundled_config is None
        same_as_native(speller, native, fixtures)


def native_lexicon(fixtures: Fixtures) -> dict[str, list[dict[str, object]]]:
    """The native results for lexicon.box, the repository's lexicon.thfst,
    by word, with no limit."""
    return {
        entry["word"]: entry["results"]
        for entry in fixtures.parity["lookups"]
        if entry["file"] == "lexicon.box" and entry["limit"] is None
    }


def test_thfst_directory_loads_by_path(fixtures: Fixtures) -> None:
    transducer = divvun_fst.load_transducer(REPOSITORY_FIXTURES / "lexicon.thfst")
    assert transducer.format == "thfst"
    for word, results in native_lexicon(fixtures).items():
        assert lookup_json(transducer.lookup(word)) == results, word


def test_optimized_lookup_loads_by_path_and_bytes(fixtures: Fixtures, made: Made) -> None:
    for transducer in [
        divvun_fst.load_transducer(made.path("lexicon.hfst")),
        divvun_fst.read_transducer(made.read("lexicon.hfst")),
    ]:
        assert transducer.format == "optimized-lookup"
        for word, results in native_lexicon(fixtures).items():
            assert lookup_json(transducer.lookup(word)) == results, word


@pytest.mark.parametrize(
    ("name", "member", "format"),
    [
        ("small-acceptor.dhfst", None, "dhfst-acceptor"),
        ("small-errmodel.dhfst", None, "dhfst-error-model"),
        ("small-dhfst.bhfst", "acceptor.default.dhfst", "dhfst-acceptor"),
        ("small-dhfst.bhfst", "errmodel.default.dhfst", "dhfst-error-model"),
        ("lexicon.box", None, "thfst"),
        ("lexicon.box", ".", "thfst"),
        ("two-lexicons.box", "second.thfst", "thfst"),
        ("small.bhfst", "acceptor.default.thfst", "thfst"),
    ],
)
def test_transducers_load_alike_both_ways(
    fixtures: Fixtures, name: str, member: str | None, format: str
) -> None:
    by_path = divvun_fst.load_transducer(fixtures.path(name), member)
    from_bytes = divvun_fst.read_transducer(fixtures.read(name), member)
    assert by_path.format == from_bytes.format == format
    for word in WORDS:
        assert by_path.lookup(word) == from_bytes.lookup(word), word


def test_paths_may_be_str_or_pathlike(fixtures: Fixtures) -> None:
    path = fixtures.path("small.bhfst")
    assert divvun_fst.load_speller(str(path)).locale == "zxx"
    assert divvun_fst.load_speller(path).locale == "zxx"
    lexicon = fixtures.path("lexicon.box")
    assert divvun_fst.load_transducer(str(lexicon)).format == "thfst"


def test_bytes_may_be_a_bytearray(fixtures: Fixtures) -> None:
    speller = divvun_fst.read_speller(bytearray(fixtures.read("small.bhfst")))
    assert speller.is_correct("cat")
    transducer = divvun_fst.read_transducer(bytearray(fixtures.read("lexicon.box")))
    assert [r.output for r in transducer.lookup("cat")] == ["cat"]


def test_loading_from_bytes_takes_bytes_only(fixtures: Fixtures) -> None:
    with pytest.raises(TypeError):
        divvun_fst.read_speller(str(fixtures.path("small.bhfst")))  # type: ignore[arg-type]
    with pytest.raises(TypeError):
        divvun_fst.read_transducer(fixtures.read("lexicon.box"), 3)  # type: ignore[arg-type]


def test_an_archive_loads_by_its_extension(fixtures: Fixtures, tmp_path: Path) -> None:
    renamed = tmp_path / "small.zip"
    shutil.copy(fixtures.path("small.bhfst"), renamed)
    with pytest.raises(divvun_fst.FstError, match="expected .zhfst or .bhfst") as raised:
        divvun_fst.load_speller(renamed)
    assert raised.value.code == "unsupported-format"
    assert divvun_fst.read_speller(renamed.read_bytes()).locale == "zxx"

