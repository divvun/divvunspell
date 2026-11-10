"""The library's events reach Python's logging module, on loggers under
divvun_fst named after the Rust module each came from."""

from __future__ import annotations

import logging

import pytest

import divvun_fst
from conftest import Made, busy_speller

BUDGET = divvun_fst.SpellerConfig(search_budget=10)


@pytest.fixture(scope="module")
def busy() -> divvun_fst.Speller:
    return divvun_fst.read_speller(busy_speller())


def test_a_calls_events_reach_their_modules_logger(
    busy: divvun_fst.Speller, caplog: pytest.LogCaptureFixture
) -> None:
    with caplog.at_level(logging.DEBUG, logger="divvun_fst"):
        busy.suggest("cat", BUDGET)
    reached = [r for r in caplog.records if "search budget reached" in r.getMessage()]
    [record] = reached
    assert record.name == "divvun_fst.speller.worker"
    assert record.levelno == logging.DEBUG
    assert "word=cat pops=10" in record.getMessage()


def test_a_loads_warning_reaches_logging_by_default(
    made: Made, caplog: pytest.LogCaptureFixture
) -> None:
    with caplog.at_level(logging.WARNING):
        speller = divvun_fst.read_speller(made.read("small.broken-config.zhfst"))
    [record] = caplog.records
    assert record.name == "divvun_fst.archive"
    assert record.levelno == logging.WARNING
    assert "ignoring malformed bundled config in archive 'speller'" in record.getMessage()
    assert speller.bundled_config is None


def test_events_below_the_divvun_fst_level_are_dropped(
    busy: divvun_fst.Speller, caplog: pytest.LogCaptureFixture
) -> None:
    with caplog.at_level(logging.INFO, logger="divvun_fst"):
        busy.suggest("cat", BUDGET)
    assert caplog.records == []


def test_trace_events_come_at_level_five(
    busy: divvun_fst.Speller, caplog: pytest.LogCaptureFixture
) -> None:
    with caplog.at_level(5, logger="divvun_fst"):
        busy.is_correct("cat")
    trace = [r for r in caplog.records if r.levelno == 5]
    assert "beginning is_correct" in " ".join(r.getMessage() for r in trace)
    assert {r.name for r in trace} == {"divvun_fst.speller", "divvun_fst.speller.worker"}
    assert {r.levelno for r in caplog.records} == {5, logging.DEBUG}


def test_logging_disabled_hands_over_nothing(
    made: Made, caplog: pytest.LogCaptureFixture
) -> None:
    with caplog.at_level(logging.DEBUG, logger="divvun_fst"):
        # Inside the block, which takes back a disable made before it.
        logging.disable(logging.CRITICAL)
        try:
            divvun_fst.read_speller(made.read("small.broken-config.zhfst"))
        finally:
            logging.disable(logging.NOTSET)
    assert caplog.records == []
