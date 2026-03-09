import Foundation
import CDivvunFST

public class SuggestionList {
    private var handle: rust_slice_t
    private var cache: [Int: Suggestion] = [:]
    public let count: Int

    init(handle: rust_slice_t) {
        self.handle = handle

        let exceptionCallback: cffi_exception_callback = { msg, msgLen in
        }

        self.count = Int(DFST_VecSuggestion_len(handle, exceptionCallback))
    }

    public subscript(index: Int) -> Suggestion {
        if index < 0 || index >= count {
            fatalError("Index out of bounds: \(index)")
        }

        if let cached = cache[index] {
            return cached
        }

        let exceptionCallback: cffi_exception_callback = { msg, msgLen in
        }

        let valueSlice = DFST_VecSuggestion_getValue(handle, index, exceptionCallback)
        let value = RustStr(ptr: valueSlice.data?.assumingMemoryBound(to: UInt8.self), len: Int(valueSlice.len)).toString()
        cffi_string_free(valueSlice)

        let weight = DFST_VecSuggestion_getWeight(handle, index, exceptionCallback)

        let completedByte = DFST_VecSuggestion_getCompleted(handle, index, exceptionCallback)
        let completed: Bool? = completedByte == 0 ? nil : (completedByte == 2)

        let suggestion = Suggestion(value: value, weight: weight, completed: completed)
        cache[index] = suggestion
        return suggestion
    }

    deinit {
        cffi_vec_free(handle)
    }
}

extension SuggestionList: Sequence {
    public func makeIterator() -> SuggestionListIterator {
        return SuggestionListIterator(list: self)
    }
}

public struct SuggestionListIterator: IteratorProtocol {
    private let list: SuggestionList
    private var index = 0

    init(list: SuggestionList) {
        self.list = list
    }

    public mutating func next() -> Suggestion? {
        guard index < list.count else {
            return nil
        }
        let suggestion = list[index]
        index += 1
        return suggestion
    }
}
