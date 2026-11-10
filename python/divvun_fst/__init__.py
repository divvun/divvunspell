"""divvunspell for Python.

Loads BHFST and ZHFST speller archives and checks and corrects words with
them, and looks words up in optimized-lookup, THFST and DHFST transducers,
giving the results native divvunspell gives. It is divvunspell's Rust
library, divvun-fst, as a native extension module, and mirrors the npm
package @divvun/fst.
"""

# The package is imported as divvun_fst, and its version is the one
# crates/divvun-fst-python carries, apart from the library's.
from divvun_fst._config import ReweightingConfig, SpellerConfig
from divvun_fst._divvun_fst import (
    AcceptorMetadata,
    ErrorModelMetadata,
    FstError,
    FstErrorCode,
    LookupResult,
    Speller,
    SpellerInfo,
    SpellerMetadata,
    SpellerTitle,
    Suggestion,
    Transducer,
    TransducerFormat,
    WeightDetails,
    WordContext,
    __version__,
    cursor_context,
    load_speller,
    load_transducer,
    read_speller,
    read_transducer,
    word_bound_indices,
    word_indices,
)

__all__ = [
    "AcceptorMetadata",
    "ErrorModelMetadata",
    "FstError",
    "FstErrorCode",
    "LookupResult",
    "ReweightingConfig",
    "Speller",
    "SpellerConfig",
    "SpellerInfo",
    "SpellerMetadata",
    "SpellerTitle",
    "Suggestion",
    "Transducer",
    "TransducerFormat",
    "WeightDetails",
    "WordContext",
    "__version__",
    "cursor_context",
    "load_speller",
    "load_transducer",
    "read_speller",
    "read_transducer",
    "word_bound_indices",
    "word_indices",
]
