"""divvunspell's tokenizer, with offsets a str is indexed by."""

from __future__ import annotations

import divvun_fst

# Letters outside the Basic Multilingual Plane, a combining accent, an emoji
# and right-to-left text, so that a byte offset or a UTF-16 one would not be
# the code point offset.
TEXT = "𝔄𝔟𝔠 ćat, 😀 dál-giella! بِسْمِ 12 x́y"


def test_each_word_is_where_its_offset_says() -> None:
    words = divvun_fst.word_indices(TEXT)
    assert [w for _, w in words] == ["𝔄𝔟𝔠", "ćat", "dál-giella", "بِسْمِ", "12", "x́y"]
    for start, word in words:
        assert TEXT[start : start + len(word)] == word


def test_the_runs_between_boundaries_make_the_text() -> None:
    runs = divvun_fst.word_bound_indices(TEXT)
    assert "".join(run for _, run in runs) == TEXT
    ends = 0
    for start, run in runs:
        assert start == ends
        ends = start + len(run)
    assert (7, ",") in runs


def test_no_text_has_no_words() -> None:
    assert divvun_fst.word_indices("") == []
    assert divvun_fst.word_indices(" ,.; ") == []
    assert divvun_fst.word_bound_indices("") == []


def test_the_cursor_word_and_its_neighbours() -> None:
    context = divvun_fst.cursor_context("😀 one two thr", "ee four five")
    assert context.current == (10, "three")
    assert context.first_before == (6, "two")
    assert context.second_before == (2, "one")
    assert context.first_after == (16, "four")
    assert context.second_after == (21, "five")
    joined = "😀 one two thr" + "ee four five"
    for found in [context.current, context.first_before, context.first_after, context.second_after]:
        assert found is not None
        start, word = found
        assert joined[start : start + len(word)] == word


def test_a_cursor_between_words() -> None:
    context = divvun_fst.cursor_context("én ", "då")
    assert context.current == (3, "då")
    assert context.first_before == (0, "én")
    assert context.first_after is None
    nowhere = divvun_fst.cursor_context("", "")
    assert nowhere.current == (0, "")
    assert nowhere.first_before is None
    assert repr(nowhere).startswith("WordContext(current=(0, ''), first_before=None")
