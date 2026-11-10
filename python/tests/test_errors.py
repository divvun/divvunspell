"""What a failed load raises."""

from __future__ import annotations

import os
from collections.abc import Callable
from pathlib import Path

import pytest

import divvun_fst
from conftest import Fixtures


def test_the_library_raises_one_exception_class() -> None:
    assert issubclass(divvun_fst.FstError, Exception)
    assert divvun_fst.FstError.__module__ == "divvun_fst"
    assert divvun_fst.FstErrorCode.__args__ == (  # type: ignore[attr-defined]
        "unsupported-format",
        "invalid-format",
        "internal",
        "failed",
    )


def test_foreign_bytes_are_an_unsupported_format() -> None:
    with pytest.raises(divvun_fst.FstError) as raised:
        divvun_fst.read_speller(bytes([0, 1, 2, 3, 4]))
    assert raised.value.code == "unsupported-format"
    assert str(raised.value) == (
        "'speller' is neither a BHFST nor a ZHFST archive: it starts with the bytes 00 01 02 03"
    )
    with pytest.raises(divvun_fst.FstError) as raised:
        divvun_fst.read_transducer(b"PK\x03\x04 a zip")
    assert raised.value.code == "unsupported-format"


def test_broken_bytes_are_an_invalid_format(fixtures: Fixtures) -> None:
    small = fixtures.read("small.bhfst")
    with pytest.raises(divvun_fst.FstError) as raised:
        divvun_fst.read_speller(small[:-40])
    assert raised.value.code == "invalid-format"
    acceptor = fixtures.read("small-acceptor.dhfst")
    with pytest.raises(divvun_fst.FstError) as raised:
        divvun_fst.read_transducer(acceptor[:40])
    assert raised.value.code == "invalid-format"


def test_nothing_is_an_invalid_format(tmp_path: Path) -> None:
    reads: list[Callable[[bytes], object]] = [divvun_fst.read_speller, divvun_fst.read_transducer]
    for read in reads:
        with pytest.raises(divvun_fst.FstError) as raised:
            read(b"")
        assert raised.value.code == "invalid-format"
    empty = tmp_path / "empty.hfst"
    empty.write_bytes(b"")
    with pytest.raises(divvun_fst.FstError, match="is empty") as raised:
        divvun_fst.load_transducer(empty)
    assert raised.value.code == "invalid-format"


def test_a_broken_file_is_an_invalid_format(fixtures: Fixtures, tmp_path: Path) -> None:
    truncated = tmp_path / "truncated.bhfst"
    truncated.write_bytes(fixtures.read("small.bhfst")[:-40])
    with pytest.raises(divvun_fst.FstError) as raised:
        divvun_fst.load_speller(truncated)
    assert raised.value.code == "invalid-format"
    assert str(truncated) in str(raised.value)
    missing_alphabet = tmp_path / "lexicon.thfst"
    missing_alphabet.mkdir()
    with pytest.raises(divvun_fst.FstError, match="'alphabet' missing") as raised:
        divvun_fst.load_transducer(missing_alphabet)
    assert raised.value.code == "invalid-format"


def test_a_missing_file_raises_what_open_raises(tmp_path: Path) -> None:
    missing = tmp_path / "missing.bhfst"
    loads: list[Callable[[Path], object]] = [divvun_fst.load_speller, divvun_fst.load_transducer]
    for load in loads:
        with pytest.raises(FileNotFoundError) as raised:
            load(missing)
        assert raised.value.filename == str(missing)
        assert raised.value.errno is not None


@pytest.mark.skipif(os.name == "nt", reason="Windows refuses to open a directory with its own error")
def test_a_directory_raises_what_open_raises(tmp_path: Path) -> None:
    archive = tmp_path / "speller.bhfst"
    archive.mkdir()
    with pytest.raises(IsADirectoryError) as raised:
        divvun_fst.load_speller(archive)
    assert raised.value.filename == str(archive)


def test_a_failed_load_leaves_others_usable(fixtures: Fixtures) -> None:
    speller = divvun_fst.read_speller(fixtures.read("small.bhfst"))
    with pytest.raises(divvun_fst.FstError):
        divvun_fst.read_speller(b"not a speller")
    assert speller.is_correct("cat")
