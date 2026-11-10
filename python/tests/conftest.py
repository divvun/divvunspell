"""What the tests read: the fixtures of @divvun/fst's parity check, with what
native divvun-fst gives for them, and small archives made here from the
repository's tests/fixtures.

The parity fixtures come from the directory `DIVVUN_FST_FIXTURES` names, as
`node js/test/fixtures.mjs <dir>` or `node js/build.mjs --test` (in
target/js-test/fixtures) writes it, or else are written by running
js/test/fixtures.mjs with node, which builds the native expectations with
cargo. Real spellers and analysers are local to a machine: the parity
fixtures leave out those that are not there, and a test that needs one is
skipped, or fails when `DIVVUN_FST_REQUIRE_ALL=1`, as js/test/run.mjs
--require-all does.
"""

from __future__ import annotations

import io
import json
import os
import shutil
import struct
import subprocess
import zipfile
from collections.abc import Sequence
from dataclasses import dataclass
from pathlib import Path
from typing import TypedDict

import pytest

ROOT = Path(__file__).resolve().parents[2]
REPOSITORY_FIXTURES = ROOT / "tests" / "fixtures"


def required() -> bool:
    """Whether a real fixture that is not there fails the run."""
    return os.environ.get("DIVVUN_FST_REQUIRE_ALL") == "1"


# What parity.json holds: each native result beside the call that gave it,
# in the shapes @divvun/fst returns. Configs are as the npm package takes
# them, camelCase; `defaultConfig` and `bundledConfig` as divvunspell writes
# them, kebab-case.
JsonObject = dict[str, object]


class NativeSpellerResult(TypedDict):
    word: str
    config: str
    isCorrect: bool
    suggest: list[JsonObject]
    analyzeOutput: list[JsonObject]


class NativeAnalysis(TypedDict):
    word: str
    config: str
    analyzeInput: list[JsonObject]


class NativeSpeller(TypedDict):
    file: str
    metadata: JsonObject | None
    bundledConfig: JsonObject | None
    results: list[NativeSpellerResult]
    analyses: list[NativeAnalysis]


class NativeLookup(TypedDict):
    file: str
    member: str | None
    format: str
    word: str
    limit: int | None
    results: list[JsonObject]


class Parity(TypedDict):
    defaultConfig: JsonObject
    configs: dict[str, JsonObject]
    missing: list[str]
    spellers: list[NativeSpeller]
    lookups: list[NativeLookup]


@dataclass(frozen=True)
class Fixtures:
    """The parity fixtures and parity.json."""

    directory: Path
    parity: Parity

    def path(self, name: str) -> Path:
        return self.directory / name

    def read(self, name: str) -> bytes:
        return self.path(name).read_bytes()

    def speller(self, name: str) -> NativeSpeller:
        [found] = [s for s in self.parity["spellers"] if s["file"] == name]
        return found

    def has(self, name: str) -> bool:
        return self.path(name).exists()


def _written_fixtures(out: Path) -> Path:
    node = shutil.which("node")
    if node is None:
        pytest.fail(
            "the tests need the parity fixtures: set DIVVUN_FST_FIXTURES to a directory "
            "`node js/test/fixtures.mjs <dir>` wrote, or install node to write them"
        )
    subprocess.run([node, ROOT / "js" / "test" / "fixtures.mjs", out], cwd=ROOT, check=True)
    return out


@pytest.fixture(scope="session")
def fixtures(tmp_path_factory: pytest.TempPathFactory) -> Fixtures:
    given = os.environ.get("DIVVUN_FST_FIXTURES")
    if given:
        directory = Path(given)
        assert (directory / "parity.json").is_file(), f"DIVVUN_FST_FIXTURES names {given}, with no parity.json"
    else:
        directory = _written_fixtures(tmp_path_factory.mktemp("fixtures"))
    parity: Parity = json.loads((directory / "parity.json").read_text(encoding="utf-8"))
    return Fixtures(directory, parity)


def needs_real(fixtures: Fixtures, name: str) -> None:
    """Skips the test, or fails it under DIVVUN_FST_REQUIRE_ALL=1, when the
    real fixture `name` was left out of the parity fixtures."""
    if fixtures.has(name):
        return
    reason = f"the real fixture {name} is not there; set its FST_JS_* variable (js/test/fixtures.mjs)"
    if required():
        pytest.fail(f"{reason}, and DIVVUN_FST_REQUIRE_ALL=1")
    pytest.skip(reason)


# HFST optimized lookup, written here because the repository holds its small
# transducers only as THFST, which is the same tables laid out for aligned
# reads: an index record there is padded from 6 bytes to 8, and the rest is
# the same, so the conversion is exact.

_OL_HEADER_TEXT = b"version\x003.3\x00type\x00HFST_OLW\x00"
_TARGET_TABLE = 2**31
_NO_SYMBOL = 0xFFFF
_NO_TARGET = 0xFFFFFFFF


def _ol(symbols: Sequence[str], index: Sequence[tuple[int, int]], transitions: bytes) -> bytes:
    """An optimized-lookup file: the header, the symbols, the index table
    from `index` (input, target) and the weighted transition table, as it is
    on disk."""
    out = bytearray(b"HFST\0")
    out += struct.pack("<HB", len(_OL_HEADER_TEXT), 0) + _OL_HEADER_TEXT
    n = len(symbols)
    transition_count = len(transitions) // 12
    out += struct.pack("<HHIIII", n, n, len(index), transition_count, len(index), transition_count)
    out += struct.pack("<9I", 1, *([0] * 8))
    for symbol in symbols:
        out += symbol.encode("utf-8") + b"\0"
    for input_symbol, target in index:
        out += struct.pack("<HI", input_symbol, target)
    return bytes(out + transitions)


def hfst_from_thfst(directory: Path) -> bytes:
    """The THFST transducer in `directory` as HFST optimized lookup."""
    alphabet = json.loads((directory / "alphabet").read_text(encoding="utf-8"))
    raw = (directory / "index").read_bytes()
    index = [
        (input_symbol, target)
        for input_symbol, _, target in struct.iter_unpack("<HHI", raw)
    ]
    return _ol(alphabet["key_table"], index, (directory / "transition").read_bytes())


OlState = tuple[float | None, list[tuple[int, int, int, float]]]


def ol_bytes(symbols: Sequence[str], states: Sequence[OlState]) -> bytes:
    """An optimized-lookup file for `states` over `symbols`, symbol 0 being
    epsilon and state 0 the start: each state its final weight, if it is
    final, and its arcs as (input, output, target state, weight). It lays the
    tables out as src/transducer/hfst/test_support.rs does: every state in
    the index table, one slot per symbol, and its arcs in the transition
    table sorted by input, closed by an entry with no input."""
    n = len(symbols)
    slot = 1 + n
    transitions = bytearray()
    index: list[tuple[int, int]] = []
    for final, arcs in states:
        base = len(index)
        index.append((_NO_SYMBOL, _NO_TARGET if final is None else struct.unpack("<I", struct.pack("<f", final))[0]))
        index.extend((_NO_SYMBOL, _NO_TARGET) for _ in range(n))
        for input_symbol, output, target, weight in sorted(arcs):
            at = base + 1 + input_symbol
            if index[at][0] == _NO_SYMBOL:
                index[at] = (input_symbol, _TARGET_TABLE + len(transitions) // 12)
            transitions += struct.pack("<HHIf", input_symbol, output, target * slot, weight)
        transitions += struct.pack("<HHIf", _NO_SYMBOL, _NO_SYMBOL, _NO_TARGET, 0.0)
    return _ol(symbols, index, bytes(transitions))


INDEX_XML = """<?xml version="1.0" encoding="UTF-8"?>
<hfstspeller dtdversion="1.0" hfstversion="3">
<info>
  <locale>zxx</locale>
  <title>Test speller</title>
  <title lang="se">Geahččalanmálle</title>
  <description>The repository's test lexicon and error model.</description>
  <producer>divvunspell</producer>
</info>
<acceptor type="general" id="acceptor.default.hfst">
  <title>cat, car, cart, care</title>
  <description>Four words.</description>
</acceptor>
<errmodel id="errmodel.default.hfst">
  <title>k:c e:a d:t</title>
  <description>Three substitutions.</description>
</errmodel>
</hfstspeller>
"""


def zhfst(
    acceptor: bytes,
    errmodel: bytes,
    *,
    compression: int,
    config: str | None = None,
) -> bytes:
    """A ZHFST archive of `acceptor` and `errmodel`, its members stored or
    deflated, bundling `config` as speller-config.json when given."""
    written = io.BytesIO()
    with zipfile.ZipFile(written, "w", compression=compression) as archive:
        archive.writestr("index.xml", INDEX_XML)
        archive.writestr("acceptor.default.hfst", acceptor)
        archive.writestr("errmodel.default.hfst", errmodel)
        if config is not None:
            archive.writestr("speller-config.json", config)
    return written.getvalue()


@dataclass(frozen=True)
class Made:
    """Archives and transducers made here from the repository's THFST
    fixtures, in formats the repository holds none in."""

    directory: Path

    def path(self, name: str) -> Path:
        return self.directory / name

    def read(self, name: str) -> bytes:
        return self.path(name).read_bytes()


@pytest.fixture(scope="session")
def made(tmp_path_factory: pytest.TempPathFactory) -> Made:
    out = tmp_path_factory.mktemp("made")
    lexicon = hfst_from_thfst(REPOSITORY_FIXTURES / "lexicon.thfst")
    errmodel = hfst_from_thfst(REPOSITORY_FIXTURES / "mutator.thfst")
    (out / "lexicon.hfst").write_bytes(lexicon)
    (out / "errmodel.hfst").write_bytes(errmodel)
    (out / "small.zhfst").write_bytes(zhfst(lexicon, errmodel, compression=zipfile.ZIP_DEFLATED))
    (out / "small.stored.zhfst").write_bytes(zhfst(lexicon, errmodel, compression=zipfile.ZIP_STORED))
    (out / "small.broken-config.zhfst").write_bytes(
        zhfst(lexicon, errmodel, compression=zipfile.ZIP_STORED, config='{"n-best": "ten"}')
    )
    (out / "small.config.zhfst").write_bytes(
        zhfst(lexicon, errmodel, compression=zipfile.ZIP_DEFLATED, config='{"n-best": 2, "max-weight": 30.5}')
    )
    return Made(out)


BUSY_SYMBOLS = ["@_EPSILON_SYMBOL_@", "c", "a", "t"]


def busy_speller() -> bytes:
    """A speller whose search never runs out of work: its lexicon accepts
    every string of its letters, and its error model inserts `a` anywhere for
    nothing, so every node the search pops leads to more of the same weight.
    Only a search budget ends a search in it, after as many nodes as the
    budget says."""
    letters = range(1, len(BUSY_SYMBOLS))
    lexicon = ol_bytes(BUSY_SYMBOLS, [(0.0, [(x, x, 0, 0.0) for x in letters])])
    errmodel = ol_bytes(BUSY_SYMBOLS, [(0.0, [(x, x, 0, 0.0) for x in letters] + [(0, 2, 0, 0.0)])])
    return zhfst(lexicon, errmodel, compression=zipfile.ZIP_STORED)
