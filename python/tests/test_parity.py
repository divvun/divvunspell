"""The parity check: the package against native divvun-fst on @divvun/fst's
fixtures.

js/test/fixtures.mjs writes parity.json from native divvun-fst, reading
every fixture from its file through the memory-mapped readers. Here every
fixture is loaded twice, by path and from its bytes, every call in
parity.json is made through the package, and each result must equal the
native one value for value: weights are the f32 values divvun-fst computed on
both sides, so no tolerance is needed, and any would hide a difference.
"""

from __future__ import annotations

import dataclasses
import re
from collections.abc import Callable, Iterator

import pytest

import divvun_fst
from conftest import Fixtures, JsonObject, NativeLookup, needs_real, required

NO_CONFIG = "none"


def snake(name: str) -> str:
    return re.sub(r"[A-Z]", lambda upper: f"_{upper.group().lower()}", name)


def kebab(name: str) -> str:
    return name.replace("_", "-")


def config_of(js: JsonObject) -> divvun_fst.SpellerConfig:
    """A parity config, as @divvun/fst takes it, as a SpellerConfig."""
    fields: dict[str, object] = {}
    for name, value in js.items():
        if name == "reweight" and isinstance(value, dict):
            value = divvun_fst.ReweightingConfig(**{snake(k): v for k, v in value.items()})
        fields[snake(name)] = value
    return divvun_fst.SpellerConfig(**fields)  # type: ignore[arg-type]


def native_config(config: divvun_fst.SpellerConfig | divvun_fst.ReweightingConfig) -> JsonObject:
    """A config under the names divvunspell writes it with."""
    out: JsonObject = {}
    for field in dataclasses.fields(config):
        value = getattr(config, field.name)
        if isinstance(value, divvun_fst.ReweightingConfig):
            value = native_config(value)
        out[kebab(field.name)] = value
    return out


def suggestion_json(suggestion: divvun_fst.Suggestion) -> JsonObject:
    out: JsonObject = {"value": suggestion.value, "weight": suggestion.weight}
    if suggestion.completed is not None:
        out["completed"] = suggestion.completed
    details = suggestion.weight_details
    if details is not None:
        out["weightDetails"] = {
            "lexiconWeight": details.lexicon_weight,
            "mutatorWeight": details.mutator_weight,
            "reweightStart": details.reweight_start,
            "reweightMid": details.reweight_mid,
            "reweightEnd": details.reweight_end,
        }
    return out


def suggestions_json(suggestions: list[divvun_fst.Suggestion]) -> list[JsonObject]:
    return [suggestion_json(s) for s in suggestions]


def titles_json(titles: list[divvun_fst.SpellerTitle]) -> list[JsonObject]:
    return [{"lang": t.lang, "value": t.value} for t in titles]


def metadata_json(metadata: divvun_fst.SpellerMetadata | None) -> JsonObject | None:
    if metadata is None:
        return None
    info, acceptor, errmodel = metadata.info, metadata.acceptor, metadata.error_model
    return {
        "info": {
            "locale": info.locale,
            "title": titles_json(info.title),
            "description": info.description,
            "producer": info.producer,
        },
        "acceptor": {
            "type": acceptor.type,
            "id": acceptor.id,
            "title": titles_json(acceptor.title),
            "description": acceptor.description,
            "continuation": acceptor.continuation,
        },
        "errorModel": {
            "id": errmodel.id,
            "title": titles_json(errmodel.title),
            "description": errmodel.description,
        },
    }


def native_locale(metadata: JsonObject | None) -> object:
    if metadata is None:
        return None
    info = metadata["info"]
    assert isinstance(info, dict)
    return info["locale"]


def lookup_json(results: list[divvun_fst.LookupResult]) -> list[JsonObject]:
    return [{"output": r.output, "symbols": r.symbols, "weight": r.weight} for r in results]


class Differences:
    """Every result compared, and each that is not the native one."""

    SHOWN = 20

    def __init__(self) -> None:
        self.compared = 0
        self.found: list[str] = []

    def compare(self, what: str, actual: object, expected: object) -> None:
        self.compared += 1
        if actual != expected:
            self.found.append(f"{what}: {actual!r}, native {expected!r}")

    def check(self) -> None:
        print(f"{self.compared} results the same as native")
        assert self.compared > 0, "parity.json has no native results to compare"
        shown = "\n".join(self.found[: self.SHOWN])
        assert not self.found, f"{len(self.found)} of {self.compared} results differ:\n{shown}"


Load = Callable[[Fixtures, str], divvun_fst.Speller]
LOADS: dict[str, Load] = {
    "path": lambda fixtures, name: divvun_fst.load_speller(fixtures.path(name)),
    "bytes": lambda fixtures, name: divvun_fst.read_speller(fixtures.read(name)),
}


def configs(fixtures: Fixtures) -> Callable[[str], divvun_fst.SpellerConfig | None]:
    made = {name: config_of(js) for name, js in fixtures.parity["configs"].items()}
    return lambda name: None if name == NO_CONFIG else made[name]


@pytest.mark.parametrize("load", LOADS)
def test_every_speller_gives_native_results(fixtures: Fixtures, load: str) -> None:
    config = configs(fixtures)
    differences = Differences()
    for native in fixtures.parity["spellers"]:
        name = native["file"]
        speller = LOADS[load](fixtures, name)
        metadata = metadata_json(speller.metadata)
        differences.compare(f"{name} metadata", metadata, native["metadata"])
        differences.compare(f"{name} locale", speller.locale, native_locale(native["metadata"]))
        bundled = speller.bundled_config
        differences.compare(
            f"{name} bundled config",
            None if bundled is None else native_config(bundled),
            native["bundledConfig"],
        )
        for result in native["results"]:
            word, given = result["word"], config(result["config"])
            what = f"{name} {word!r} config {result['config']}"
            differences.compare(f"{what} is_correct", speller.is_correct(word, given), result["isCorrect"])
            differences.compare(
                f"{what} suggest", suggestions_json(speller.suggest(word, given)), result["suggest"]
            )
            differences.compare(
                f"{what} analyze_output",
                suggestions_json(speller.analyze_output(word, given)),
                result["analyzeOutput"],
            )
        for analysis in native["analyses"]:
            word, given = analysis["word"], config(analysis["config"])
            differences.compare(
                f"{name} {word!r} config {analysis['config']} analyze_input",
                suggestions_json(speller.analyze_input(word, given)),
                analysis["analyzeInput"],
            )
    differences.check()


def lookup_groups(fixtures: Fixtures) -> Iterator[list[NativeLookup]]:
    """The native lookups by transducer: file and member."""
    groups: dict[tuple[str, str | None], list[NativeLookup]] = {}
    for entry in fixtures.parity["lookups"]:
        groups.setdefault((entry["file"], entry["member"]), []).append(entry)
    yield from groups.values()


@pytest.mark.parametrize("load", ["path", "bytes"])
def test_every_lookup_gives_native_results(fixtures: Fixtures, load: str) -> None:
    differences = Differences()
    for entries in lookup_groups(fixtures):
        first = entries[0]
        name, member = first["file"], first["member"]
        if load == "path":
            transducer = divvun_fst.load_transducer(fixtures.path(name), member)
        else:
            transducer = divvun_fst.read_transducer(fixtures.read(name), member)
        label = name if member is None else f"{name} member {member}"
        differences.compare(f"{label} format", transducer.format, first["format"])
        for entry in entries:
            found = transducer.lookup(entry["word"], entry["limit"])
            differences.compare(
                f"{label} {entry['word']!r} limit {entry['limit']}", lookup_json(found), entry["results"]
            )
    differences.check()


def test_defaults_are_divvun_fsts_own(fixtures: Fixtures) -> None:
    assert native_config(divvun_fst.SpellerConfig()) == fixtures.parity["defaultConfig"]


def test_the_parity_configs_set_every_field(fixtures: Fixtures) -> None:
    set_fields = set()
    for js in fixtures.parity["configs"].values():
        for name, value in js.items():
            set_fields.add(snake(name))
            if isinstance(value, dict):
                set_fields |= {f"{name}.{snake(k)}" for k in value}
    every = {f.name for f in dataclasses.fields(divvun_fst.SpellerConfig)}
    every |= {f"reweight.{f.name}" for f in dataclasses.fields(divvun_fst.ReweightingConfig)}
    assert every - set_fields == set()


def test_real_fixtures_are_there(fixtures: Fixtures) -> None:
    missing = fixtures.parity["missing"]
    if not missing:
        return
    reason = f"the parity fixtures leave out {', '.join(missing)}"
    if required():
        pytest.fail(f"{reason}, and DIVVUN_FST_REQUIRE_ALL=1")
    pytest.skip(reason)


@pytest.mark.parametrize("name", ["sma.bhfst", "se.bhfst", "ga.zhfst"])
def test_real_spellers_take_part(fixtures: Fixtures, name: str) -> None:
    needs_real(fixtures, name)
    assert name in {s["file"] for s in fixtures.parity["spellers"]}
