"""Calls release the GIL, and threads share one loaded speller or
transducer."""

from __future__ import annotations

import threading
from collections.abc import Callable
from concurrent.futures import ThreadPoolExecutor

import divvun_fst
from conftest import Fixtures, busy_speller

# A search in the busy speller that examines this many nodes takes a few
# tenths of a second, long enough for the calling thread to go round its
# loop thousands of times if the GIL is free.
LONG_SEARCH = divvun_fst.SpellerConfig(search_budget=4000)


def python_runs_during(work: Callable[[], object]) -> int:
    """How many times the calling thread gets round a Python loop while `work`
    runs in another thread. Work that holds the GIL from start to finish leaves
    the loop next to no turns."""
    started = threading.Event()

    def run() -> None:
        started.set()
        work()

    worker = threading.Thread(target=run)
    worker.start()
    started.wait()
    turns = 0
    while worker.is_alive():
        turns += 1
    worker.join()
    return turns


def test_a_suggestion_search_releases_the_gil() -> None:
    speller = divvun_fst.read_speller(busy_speller())
    assert python_runs_during(lambda: speller.suggest("cat", LONG_SEARCH)) > 1000


def test_threads_share_one_speller(fixtures: Fixtures) -> None:
    speller = divvun_fst.load_speller(fixtures.path("small.bhfst"))
    words = ["kat", "cet", "kare", "cat", "Kat", "xyz"]
    alone = [(speller.is_correct(w), speller.suggest(w), speller.analyze_input(w)) for w in words]
    with ThreadPoolExecutor(max_workers=8) as pool:
        together = list(
            pool.map(
                lambda i: (
                    speller.is_correct(words[i % 6]),
                    speller.suggest(words[i % 6]),
                    speller.analyze_input(words[i % 6]),
                ),
                range(96),
            )
        )
    assert together == [alone[i % 6] for i in range(96)]


def test_threads_share_one_transducer(fixtures: Fixtures) -> None:
    errmodel = divvun_fst.load_transducer(fixtures.path("small-errmodel.dhfst"))
    words = ["kat", "cet", "kare", "cat"]
    alone = [errmodel.lookup(w) for w in words]
    with ThreadPoolExecutor(max_workers=8) as pool:
        together = list(pool.map(lambda i: errmodel.lookup(words[i % 4]), range(96)))
    assert together == [alone[i % 4] for i in range(96)]
