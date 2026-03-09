import Foundation
import CDivvunFST

public struct RustStr {
    let ptr: UnsafePointer<UInt8>?
    let len: Int

    private var cachedString: String?

    init(ptr: UnsafePointer<UInt8>?, len: Int) {
        self.ptr = ptr
        self.len = len
    }

    public var isEmpty: Bool {
        return ptr == nil || len == 0
    }

    public func toString() -> String {
        if let cached = cachedString {
            return cached
        }

        guard let ptr = ptr, len > 0 else {
            return ""
        }

        let data = Data(bytes: ptr, count: len)
        let str = String(data: data, encoding: .utf8) ?? ""
        return str
    }
}

extension RustStr: Equatable {
    public static func == (lhs: RustStr, rhs: RustStr) -> Bool {
        return lhs.toString() == rhs.toString()
    }
}

extension RustStr: Comparable {
    public static func < (lhs: RustStr, rhs: RustStr) -> Bool {
        return lhs.toString() < rhs.toString()
    }
}

extension RustStr: Hashable {
    public func hash(into hasher: inout Hasher) {
        hasher.combine(toString())
    }
}

extension RustStr: CustomStringConvertible {
    public var description: String {
        return toString()
    }
}
