import Foundation
import CDivvunFST

public class Speller {
    private var speller: DFST_Speller

    init(speller: DFST_Speller) {
        self.speller = speller
    }

    public func isCorrect(_ word: String) throws -> Bool {
        let exceptionCallback: cffi_exception_callback = { msg, msgLen in
        }

        let wordBytes = Array(word.utf8)
        let result = wordBytes.withUnsafeBytes { wordPtr in
            let wordSlice = rust_slice_t(data: UnsafeMutableRawPointer(mutating: wordPtr.baseAddress), len: UInt(wordBytes.count))
            return DFST_Speller_isCorrect(speller, wordSlice, exceptionCallback)
        }

        return result != 0
    }

    public func suggest(_ word: String) throws -> SuggestionList {
        let exceptionCallback: cffi_exception_callback = { msg, msgLen in
        }

        let wordBytes = Array(word.utf8)
        let suggestions = wordBytes.withUnsafeBytes { wordPtr in
            let wordSlice = rust_slice_t(data: UnsafeMutableRawPointer(mutating: wordPtr.baseAddress), len: UInt(wordBytes.count))
            return DFST_Speller_suggest(speller, wordSlice, exceptionCallback)
        }

        if suggestions.data == nil {
            throw NSError(domain: "DivvunFST", code: 6, userInfo: [NSLocalizedDescriptionKey: "Failed to get suggestions"])
        }

        return SuggestionList(handle: suggestions)
    }
}
