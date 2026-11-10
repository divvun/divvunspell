"""SpellerConfig and ReweightingConfig: divvunspell's config as immutable
values, checked when they are made."""

from __future__ import annotations

import dataclasses
import math

import pytest

import divvun_fst
from divvun_fst import ReweightingConfig, SpellerConfig
from conftest import Fixtures
from divvun_fst._divvun_fst import _builtin_speller_config, _resolve_speller_config

EVERY_FIELD = SpellerConfig(
    n_best=3,
    max_weight=40.5,
    beam=15.0,
    reweight=ReweightingConfig(start_penalty=2.0, end_penalty=3.0, mid_penalty=1.25, curve=5.0),
    recase=False,
    completion_marker="+",
    astar_lookahead=True,
    search_dedup=False,
    mutator_subsets=False,
    search_budget=5000,
    word_split_weight=20.0,
    boundary_edit_weight=15.0,
    initial_case_weight=8.5,
    verbose=True,
)


def test_the_defaults_are_divvun_fsts_built_in_config() -> None:
    assert SpellerConfig() == _builtin_speller_config()
    assert SpellerConfig().reweight == ReweightingConfig()


def test_every_field_reaches_divvun_fst_as_given() -> None:
    assert _resolve_speller_config(EVERY_FIELD) == EVERY_FIELD
    defaults = {f.name: f.default for f in dataclasses.fields(SpellerConfig)}
    assert all(getattr(EVERY_FIELD, name) != default for name, default in defaults.items())


def test_a_field_left_out_takes_its_default() -> None:
    resolved = _resolve_speller_config(SpellerConfig(reweight=ReweightingConfig(mid_penalty=1.0)))
    assert resolved.reweight == ReweightingConfig(start_penalty=10.0, end_penalty=10.0, mid_penalty=1.0)
    assert resolved.max_weight == SpellerConfig().max_weight


def test_none_means_none() -> None:
    resolved = _resolve_speller_config(SpellerConfig(n_best=None, reweight=None, max_weight=None))
    assert (resolved.n_best, resolved.reweight, resolved.max_weight) == (None, None, None)


def test_weights_come_back_as_written() -> None:
    resolved = _resolve_speller_config(SpellerConfig(max_weight=0.1, beam=16777217))
    assert resolved.max_weight == 0.1
    assert resolved.beam == 16777216.0


def test_a_config_is_frozen_and_keyword_only() -> None:
    config = SpellerConfig(n_best=3)
    with pytest.raises(dataclasses.FrozenInstanceError):
        config.n_best = 4  # type: ignore[misc]
    with pytest.raises(TypeError):
        SpellerConfig(3)  # type: ignore[call-arg]
    with pytest.raises(TypeError):
        SpellerConfig(nbest=3)  # type: ignore[call-arg]
    assert not hasattr(config, "__dict__")
    assert dataclasses.replace(config, n_best=4).n_best == 4
    assert hash(config) == hash(SpellerConfig(n_best=3))


def test_repr_names_only_what_differs() -> None:
    assert repr(SpellerConfig()) == "SpellerConfig()"
    config = SpellerConfig(n_best=None, reweight=ReweightingConfig(curve=5.0))
    assert repr(config) == "SpellerConfig(n_best=None, reweight=ReweightingConfig(curve=5.0))"


@pytest.mark.parametrize(
    ("fields", "said"),
    [
        ({"n_best": 2.5}, "'n_best' must be int or None, not float"),
        ({"n_best": True}, "'n_best' must be int or None, not bool"),
        ({"n_best": "3"}, "'n_best' must be int or None, not str"),
        ({"max_weight": "10"}, "'max_weight' must be float or None, not str"),
        ({"beam": False}, "'beam' must be float or None, not bool"),
        ({"recase": None}, "'recase' must be bool, not NoneType"),
        ({"verbose": 1}, "'verbose' must be bool, not int"),
        ({"completion_marker": 1}, "'completion_marker' must be str or None, not int"),
        ({"reweight": {"mid_penalty": 1}}, "'reweight' must be ReweightingConfig or None, not dict"),
    ],
)
def test_a_value_of_the_wrong_type_is_refused(fields: dict[str, object], said: str) -> None:
    with pytest.raises(TypeError, match=f"SpellerConfig\\(\\) argument {said}"):
        SpellerConfig(**fields)  # type: ignore[arg-type]


@pytest.mark.parametrize("value", [math.nan, math.inf, -math.inf])
def test_nan_and_infinities_are_refused(value: float) -> None:
    with pytest.raises(ValueError, match="'max_weight' must be finite"):
        SpellerConfig(max_weight=value)
    with pytest.raises(ValueError, match="'curve' must be finite"):
        ReweightingConfig(curve=value)
    with pytest.raises(ValueError, match="'start_penalty' must be finite"):
        ReweightingConfig(start_penalty=value)


def test_numbers_out_of_range_overflow() -> None:
    with pytest.raises(OverflowError, match="'n_best' must be from 0 to"):
        SpellerConfig(n_best=-1)
    with pytest.raises(OverflowError, match="'search_budget' must be from 0 to 18446744073709551615"):
        SpellerConfig(search_budget=2**64)
    with pytest.raises(OverflowError, match="32-bit float"):
        SpellerConfig(beam=1e39)


def test_a_penalty_cannot_be_none() -> None:
    with pytest.raises(TypeError, match="'mid_penalty' must be float, not NoneType"):
        ReweightingConfig(mid_penalty=None)  # type: ignore[arg-type]


def test_a_call_takes_only_a_speller_config(fixtures: Fixtures) -> None:
    speller = divvun_fst.read_speller(fixtures.read("small.bhfst"))
    with pytest.raises(TypeError, match="suggest\\(\\) argument 'config' must be SpellerConfig or None, not dict"):
        speller.suggest("kat", {"n_best": 3})  # type: ignore[arg-type]
    with pytest.raises(TypeError, match="is_correct\\(\\) argument 'config'"):
        speller.is_correct("kat", ReweightingConfig())  # type: ignore[arg-type]
