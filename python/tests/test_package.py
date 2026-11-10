"""The package as it installs: its name, version, licence, wheel and types."""

from __future__ import annotations

import ast
import dataclasses
import inspect
import os
import re
import subprocess
import sys
import tomllib
import typing
import zipfile
from importlib import metadata
from pathlib import Path

import pytest

import divvun_fst
import divvun_fst._divvun_fst
from conftest import ROOT, Fixtures

PACKAGE = Path(divvun_fst.__file__).parent
STUB = ast.parse((PACKAGE / "_divvun_fst.pyi").read_text(encoding="utf-8"))
CONFIG = ast.parse((PACKAGE / "_config.py").read_text(encoding="utf-8"))
CRATE = ROOT / "crates" / "divvun-fst-python" / "Cargo.toml"


def toml(path: Path) -> dict[str, typing.Any]:
    with path.open("rb") as f:
        return tomllib.load(f)


def stub_names() -> dict[str, ast.stmt]:
    names: dict[str, ast.stmt] = {}
    for node in STUB.body:
        if isinstance(node, (ast.ClassDef, ast.FunctionDef)):
            names[node.name] = node
        elif isinstance(node, ast.AnnAssign) and isinstance(node.target, ast.Name):
            names[node.target.id] = node
    return names


def stub_class(name: str) -> ast.ClassDef:
    node = stub_names()[name]
    assert isinstance(node, ast.ClassDef)
    return node


def stub_function(name: str, cls: str | None = None) -> ast.FunctionDef:
    body = STUB.body if cls is None else stub_class(cls).body
    for node in body:
        if isinstance(node, ast.FunctionDef) and node.name == name:
            return node
    raise AssertionError(f"{name} is not in the stub")


def properties(cls: str) -> dict[str, str]:
    """Each property of `cls` in the stub, with its return annotation."""
    found: dict[str, str] = {}
    for node in stub_class(cls).body:
        if isinstance(node, ast.FunctionDef) and node.returns is not None:
            if any(isinstance(d, ast.Name) and d.id == "property" for d in node.decorator_list):
                found[node.name] = ast.unparse(node.returns)
    return found


def test_the_version_is_the_binding_crates() -> None:
    version = toml(CRATE)["package"]["version"]
    assert isinstance(version, str)
    assert divvun_fst.__version__ == version
    assert metadata.version("divvun-fst") == version


@pytest.fixture(scope="module")
def wheel(tmp_path_factory: pytest.TempPathFactory) -> Path:
    """The wheel `DIVVUN_FST_WHEEL` names, which CI sets to the one it tests
    and publishes, or else one maturin builds from python/ as a release
    would."""
    given = os.environ.get("DIVVUN_FST_WHEEL")
    if given:
        named = Path(given)
        assert named.is_file(), f"DIVVUN_FST_WHEEL names {given}, not a file from {Path.cwd()}"
        return named
    out = tmp_path_factory.mktemp("wheel")
    subprocess.run(
        [sys.executable, "-m", "maturin", "build", "--quiet", "--out", out],
        cwd=ROOT / "python",
        check=True,
    )
    [built] = out.glob("*.whl")
    return built


def test_the_wheel_is_divvun_fst_under_mit_or_apache(wheel: Path) -> None:
    meta = metadata.metadata("divvun-fst")
    assert meta["Name"] == "divvun-fst"
    assert meta["License-Expression"] == "MIT OR Apache-2.0"
    assert meta["Requires-Python"] == ">=3.11"
    version = divvun_fst.__version__
    dist_info = f"divvun_fst-{version}.dist-info"
    with zipfile.ZipFile(wheel) as built:
        files = set(built.namelist())
        licences = {
            name: built.read(f"{dist_info}/licenses/{name}") for name in ["LICENSE-MIT", "LICENSE-APACHE"]
        }
    assert {
        "divvun_fst/__init__.py",
        "divvun_fst/_config.py",
        "divvun_fst/_divvun_fst.pyi",
        "divvun_fst/py.typed",
    } <= files
    assert {f.split("/")[0] for f in files} == {"divvun_fst", dist_info}
    assert not [f for f in files if "__pycache__" in f or f.endswith(".pyc")]
    for name, text in licences.items():
        assert text.decode().strip() == (ROOT / name).read_text().strip()


def test_a_named_wheel_is_the_one_imported() -> None:
    if not os.environ.get("DIVVUN_FST_WHEEL"):
        pytest.skip("no DIVVUN_FST_WHEEL: the tests run against the checkout's build")
    assert PACKAGE.resolve() != (ROOT / "python" / "divvun_fst").resolve()
    assert Path(divvun_fst._divvun_fst.__file__ or "").parent == PACKAGE


def test_one_wheel_serves_cpython_311_on(wheel: Path) -> None:
    pyo3 = toml(CRATE)["dependencies"]["pyo3"]
    assert pyo3["features"] == ["abi3-py311"]
    assert f"-{divvun_fst.__version__}-cp311-abi3-" in wheel.name
    with zipfile.ZipFile(wheel) as built:
        [native] = [
            f for f in built.namelist() if f.startswith("divvun_fst/_divvun_fst.") and not f.endswith(".pyi")
        ]
    assert native in ("divvun_fst/_divvun_fst.abi3.so", "divvun_fst/_divvun_fst.pyd")


def test_the_stub_declares_the_whole_api() -> None:
    assert (PACKAGE / "py.typed").is_file()
    assert set(divvun_fst.__all__) <= set(stub_names()) | {"SpellerConfig", "ReweightingConfig"}
    declared = {n for n in stub_names() if not n.startswith("_")} | {"__version__"}
    native = {n for n in dir(divvun_fst._divvun_fst) if not n.startswith("_")} | {"__version__"}
    assert declared == native
    assert all(hasattr(divvun_fst._divvun_fst, n) for n in stub_names())
    exported = {n for n in dir(divvun_fst) if not n.startswith("_")} - {"__version__"}
    assert exported - {"SpellerConfig", "ReweightingConfig"} <= native


def test_no_public_signature_uses_any() -> None:
    for node in [*ast.walk(STUB), *ast.walk(CONFIG)]:
        assert not (isinstance(node, ast.Name) and node.id == "Any")
        assert not (isinstance(node, ast.Attribute) and node.attr == "Any")


def runtime_signature(function: object) -> str:
    return str(inspect.signature(function))  # type: ignore[arg-type]


@pytest.mark.parametrize(
    ("name", "cls", "signature"),
    [
        ("is_correct", "Speller", "(self, word, /, config=None)"),
        ("suggest", "Speller", "(self, word, /, config=None)"),
        ("analyze_input", "Speller", "(self, word, /, config=None)"),
        ("analyze_output", "Speller", "(self, word, /, config=None)"),
        ("lookup", "Transducer", "(self, word, /, limit=None)"),
        ("read_speller", None, "(data)"),
        ("load_speller", None, "(path)"),
        ("read_transducer", None, "(data, member=None)"),
        ("load_transducer", None, "(path, member=None)"),
        ("word_indices", None, "(text)"),
        ("word_bound_indices", None, "(text)"),
        ("cursor_context", None, "(before, after)"),
    ],
)
def test_the_stubs_signatures_are_the_runtimes(name: str, cls: str | None, signature: str) -> None:
    owner = divvun_fst if cls is None else getattr(divvun_fst, cls)
    assert runtime_signature(getattr(owner, name)) == signature
    declared = stub_function(name, cls)
    names = [a.arg for a in [*declared.args.posonlyargs, *declared.args.args]]
    defaults: list[ast.expr | None] = [None] * (len(names) - len(declared.args.defaults))
    defaults += declared.args.defaults
    shown = [
        n if d is None else f"{n}={ast.unparse(d)}" for n, d in zip(names, defaults, strict=True)
    ]
    if declared.args.posonlyargs:
        shown.insert(len(declared.args.posonlyargs), "/")
    assert f"({', '.join(shown)})" == signature


def test_the_literals_are_the_runtimes() -> None:
    for name in ["TransducerFormat", "FstErrorCode"]:
        node = stub_names()[name]
        assert isinstance(node, ast.AnnAssign) and node.value is not None
        runtime = typing.get_args(getattr(divvun_fst, name))
        assert ast.unparse(node.value) == f"Literal[{', '.join(repr(v) for v in runtime)}]"


def test_results_carry_their_declared_types(fixtures: Fixtures) -> None:
    kinds: dict[str, object] = {
        "str": str,
        "float": float,
        "bool | None": (bool, type(None)),
        "str | None": (str, type(None)),
        "list[str]": list,
        "list[SpellerTitle]": list,
        "WeightDetails | None": (divvun_fst.WeightDetails, type(None)),
        "SpellerInfo": divvun_fst.SpellerInfo,
        "AcceptorMetadata": divvun_fst.AcceptorMetadata,
        "ErrorModelMetadata": divvun_fst.ErrorModelMetadata,
        "tuple[int, str]": tuple,
        "tuple[int, str] | None": (tuple, type(None)),
        "TransducerFormat": str,
    }
    speller = divvun_fst.read_speller(fixtures.read("small.bhfst"))
    metadata_ = speller.metadata
    assert metadata_ is not None
    [suggestion] = speller.suggest("kat", divvun_fst.SpellerConfig(n_best=1, verbose=True))
    details = suggestion.weight_details
    [result] = divvun_fst.read_transducer(fixtures.read("lexicon.box")).lookup("cat")
    values: dict[str, object] = {
        "Suggestion": suggestion,
        "WeightDetails": details,
        "SpellerMetadata": metadata_,
        "SpellerInfo": metadata_.info,
        "SpellerTitle": metadata_.info.title[0],
        "AcceptorMetadata": metadata_.acceptor,
        "ErrorModelMetadata": metadata_.error_model,
        "LookupResult": result,
        "WordContext": divvun_fst.cursor_context("a b", " c"),
    }
    for cls, value in values.items():
        for name, annotation in properties(cls).items():
            kind = kinds[annotation]
            assert isinstance(getattr(value, name), kind), f"{cls}.{name}"  # type: ignore[arg-type]


def test_the_config_fields_are_divvun_fsts() -> None:
    names = [f.name for f in dataclasses.fields(divvun_fst.SpellerConfig)]
    assert names == [
        "n_best",
        "max_weight",
        "beam",
        "reweight",
        "recase",
        "completion_marker",
        "astar_lookahead",
        "search_dedup",
        "mutator_subsets",
        "search_budget",
        "word_split_weight",
        "boundary_edit_weight",
        "initial_case_weight",
        "verbose",
    ]


def test_names_follow_python_conventions() -> None:
    snake = re.compile(r"^_?_?[a-z][a-z0-9_]*$")
    pascal = re.compile(r"^[A-Z][A-Za-z0-9]*$")
    for node in [*ast.walk(STUB), *ast.walk(CONFIG)]:
        if isinstance(node, ast.ClassDef):
            assert pascal.match(node.name), node.name
            for field in node.body:
                if isinstance(field, ast.AnnAssign) and isinstance(field.target, ast.Name):
                    assert snake.match(field.target.id), field.target.id
        elif isinstance(node, ast.FunctionDef):
            assert snake.match(node.name), node.name
            for arg in [*node.args.posonlyargs, *node.args.args, *node.args.kwonlyargs]:
                assert snake.match(arg.arg), arg.arg
