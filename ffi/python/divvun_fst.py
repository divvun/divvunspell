import ctypes
import platform
import os
from pathlib import Path
from typing import List, Optional, Iterator, Tuple


def _get_library_path():
    """Find the divvun-fst shared library."""
    base_dir = Path(__file__).parent.parent.parent

    system = platform.system()
    if system == "Darwin":
        lib_name = "libdivvun_fst.dylib"
    elif system == "Linux":
        lib_name = "libdivvun_fst.so"
    elif system == "Windows":
        lib_name = "divvun_fst.dll"
    else:
        raise RuntimeError(f"Unsupported platform: {system}")

    debug_path = base_dir / "target" / "debug" / lib_name
    release_path = base_dir / "target" / "release" / lib_name

    if debug_path.exists():
        return str(debug_path)
    elif release_path.exists():
        return str(release_path)
    else:
        raise FileNotFoundError(
            f"Could not find {lib_name} in target/debug or target/release. "
            "Build the FFI library first with: cd ffi && cargo build"
        )


class RustSlice(ctypes.Structure):
    """Rust slice representation (pointer + length)."""
    _fields_ = [
        ("data", ctypes.c_void_p),
        ("len", ctypes.c_size_t),
    ]

    def to_string(self) -> str:
        """Convert RustSlice to Python string."""
        if self.data is None or self.len == 0:
            return ""
        return ctypes.string_at(self.data, self.len).decode('utf-8')


class CffiTraitObject(ctypes.Structure):
    """CFFI trait object (fat pointer: data + vtable)."""
    _fields_ = [
        ("data", ctypes.c_void_p),
        ("vtable", ctypes.c_void_p),
    ]

    def is_null(self) -> bool:
        return self.data is None


class _DivvunFstLib:
    """Singleton wrapper for the divvun-fst C library."""

    _instance = None

    def __new__(cls):
        if cls._instance is None:
            cls._instance = super().__new__(cls)
            cls._instance._initialize()
        return cls._instance

    def _initialize(self):
        lib_path = _get_library_path()
        self.lib = ctypes.CDLL(lib_path)

        self._last_error = None
        self._error_callback_type = ctypes.CFUNCTYPE(
            None, ctypes.POINTER(ctypes.c_uint8), ctypes.c_size_t
        )
        self._error_callback = self._error_callback_type(self._handle_error)

        self._setup_functions()

    def _handle_error(self, msg_ptr, msg_len):
        """Callback for Rust error handling."""
        error_bytes = ctypes.string_at(msg_ptr, msg_len)
        self._last_error = error_bytes.decode('utf-8')

    def _check_error(self):
        """Check if an error occurred and raise it."""
        if self._last_error is not None:
            error = self._last_error
            self._last_error = None
            raise RuntimeError(error)

    def _setup_functions(self):
        # Archive functions
        self.lib.DFST_SpellerArchive_open.argtypes = [RustSlice, self._error_callback_type]
        self.lib.DFST_SpellerArchive_open.restype = CffiTraitObject

        self.lib.DFST_SpellerArchive_speller.argtypes = [CffiTraitObject, self._error_callback_type]
        self.lib.DFST_SpellerArchive_speller.restype = CffiTraitObject

        self.lib.DFST_SpellerArchive_locale.argtypes = [CffiTraitObject, self._error_callback_type]
        self.lib.DFST_SpellerArchive_locale.restype = RustSlice

        # Speller functions
        self.lib.DFST_Speller_isCorrect.argtypes = [CffiTraitObject, RustSlice, self._error_callback_type]
        self.lib.DFST_Speller_isCorrect.restype = ctypes.c_uint8

        self.lib.DFST_Speller_suggest.argtypes = [CffiTraitObject, RustSlice, self._error_callback_type]
        self.lib.DFST_Speller_suggest.restype = RustSlice

        # Suggestion vector functions
        self.lib.DFST_VecSuggestion_len.argtypes = [RustSlice, self._error_callback_type]
        self.lib.DFST_VecSuggestion_len.restype = ctypes.c_size_t

        self.lib.DFST_VecSuggestion_getValue.argtypes = [RustSlice, ctypes.c_size_t, self._error_callback_type]
        self.lib.DFST_VecSuggestion_getValue.restype = RustSlice

        self.lib.DFST_VecSuggestion_getWeight.argtypes = [RustSlice, ctypes.c_size_t, self._error_callback_type]
        self.lib.DFST_VecSuggestion_getWeight.restype = ctypes.c_float

        self.lib.DFST_VecSuggestion_getCompleted.argtypes = [RustSlice, ctypes.c_size_t, self._error_callback_type]
        self.lib.DFST_VecSuggestion_getCompleted.restype = ctypes.c_uint8

        # Memory management
        self.lib.DFST_cstr_free.argtypes = [ctypes.c_void_p]
        self.lib.DFST_cstr_free.restype = None

        self.lib.cffi_string_free.argtypes = [RustSlice]
        self.lib.cffi_string_free.restype = None

        self.lib.cffi_vec_free.argtypes = [RustSlice]
        self.lib.cffi_vec_free.restype = None

        # Word indices (tokenization)
        self.lib.DFST_WordIndices_new.argtypes = [ctypes.c_char_p]
        self.lib.DFST_WordIndices_new.restype = ctypes.c_void_p

        self.lib.DFST_WordIndices_next.argtypes = [
            ctypes.c_void_p,
            ctypes.POINTER(ctypes.c_uint64),
            ctypes.POINTER(ctypes.c_char_p)
        ]
        self.lib.DFST_WordIndices_next.restype = ctypes.c_uint8

        self.lib.DFST_WordIndices_free.argtypes = [ctypes.c_void_p]
        self.lib.DFST_WordIndices_free.restype = None


_lib = _DivvunFstLib()


class Suggestion:
    """A spelling suggestion with metadata."""

    def __init__(self, value: str, weight: float, completed: Optional[bool]):
        self.value = value
        self.weight = weight
        self.completed = completed

    def __repr__(self) -> str:
        completed_str = "unknown" if self.completed is None else ("completed" if self.completed else "not completed")
        return f"Suggestion(value='{self.value}', weight={self.weight:.4f}, {completed_str})"


class Speller:
    """Spell checker interface."""

    def __init__(self, handle: CffiTraitObject):
        self._handle = handle

    def is_correct(self, word: str) -> bool:
        """Check if a word is spelled correctly."""
        word_bytes = word.encode('utf-8')
        word_slice = RustSlice(ctypes.cast(word_bytes, ctypes.c_void_p), len(word_bytes))
        result = _lib.lib.DFST_Speller_isCorrect(self._handle, word_slice, _lib._error_callback)
        _lib._check_error()
        return bool(result)

    def suggest(self, word: str) -> List[Suggestion]:
        """Get spelling suggestions for a word."""
        word_bytes = word.encode('utf-8')
        word_slice = RustSlice(ctypes.cast(word_bytes, ctypes.c_void_p), len(word_bytes))
        suggestions_slice = _lib.lib.DFST_Speller_suggest(self._handle, word_slice, _lib._error_callback)
        _lib._check_error()

        if suggestions_slice.data is None:
            return []

        length = _lib.lib.DFST_VecSuggestion_len(suggestions_slice, _lib._error_callback)
        _lib._check_error()

        results = []
        for i in range(length):
            value_slice = _lib.lib.DFST_VecSuggestion_getValue(suggestions_slice, i, _lib._error_callback)
            _lib._check_error()
            value = value_slice.to_string()
            _lib.lib.cffi_string_free(value_slice)

            weight = _lib.lib.DFST_VecSuggestion_getWeight(suggestions_slice, i, _lib._error_callback)
            _lib._check_error()

            completed_byte = _lib.lib.DFST_VecSuggestion_getCompleted(suggestions_slice, i, _lib._error_callback)
            _lib._check_error()
            completed = None if completed_byte == 0 else (completed_byte == 2)

            results.append(Suggestion(value, weight, completed))

        _lib.lib.cffi_vec_free(suggestions_slice)
        return results


class SpellerArchive:
    """Spell checker archive (.bhfst file)."""

    def __init__(self, path: str):
        """Open a speller archive from a file path."""
        path_bytes = path.encode('utf-8')
        path_slice = RustSlice(ctypes.cast(path_bytes, ctypes.c_void_p), len(path_bytes))
        self._handle = _lib.lib.DFST_SpellerArchive_open(path_slice, _lib._error_callback)
        _lib._check_error()

        if self._handle.is_null():
            raise RuntimeError(f"Failed to open speller archive: {path}")

    def speller(self) -> Speller:
        """Get the speller from this archive."""
        speller_handle = _lib.lib.DFST_SpellerArchive_speller(self._handle, _lib._error_callback)
        _lib._check_error()

        if speller_handle.is_null():
            raise RuntimeError("Failed to get speller from archive")

        return Speller(speller_handle)

    def locale(self) -> str:
        """Get the locale of this speller archive."""
        locale_slice = _lib.lib.DFST_SpellerArchive_locale(self._handle, _lib._error_callback)
        _lib._check_error()

        locale = locale_slice.to_string()
        _lib.lib.cffi_string_free(locale_slice)
        return locale


class WordIndices:
    """Iterator over word boundaries in a string."""

    def __init__(self, text: str):
        """Create a word tokenizer for the given text."""
        self._text_bytes = text.encode('utf-8') + b'\0'
        self._handle = _lib.lib.DFST_WordIndices_new(ctypes.c_char_p(self._text_bytes))
        if self._handle is None:
            raise RuntimeError("Failed to create word indices iterator")

    def __iter__(self) -> Iterator[Tuple[int, str]]:
        """Iterate over (index, word) pairs."""
        return self

    def __next__(self) -> Tuple[int, str]:
        """Get the next word."""
        index = ctypes.c_uint64()
        word_ptr = ctypes.c_char_p()

        result = _lib.lib.DFST_WordIndices_next(
            self._handle,
            ctypes.byref(index),
            ctypes.byref(word_ptr)
        )

        if result == 0:
            raise StopIteration

        word = word_ptr.value.decode('utf-8')
        _lib.lib.DFST_cstr_free(word_ptr)

        return (index.value, word)

    def __del__(self):
        """Clean up the iterator."""
        if hasattr(self, '_handle') and self._handle is not None:
            _lib.lib.DFST_WordIndices_free(self._handle)


def tokenize(text: str) -> List[Tuple[int, str]]:
    """Tokenize text into words with their byte indices."""
    return list(WordIndices(text))
