import Foundation
import CDivvunFST

public class WordContext {
    private var ctx: CWordContext
    private let firstHalfBytes: [UInt8]
    private let secondHalfBytes: [UInt8]

    public let current: String
    public let firstBefore: String
    public let secondBefore: String
    public let firstAfter: String
    public let secondAfter: String

    init(context: CWordContext, firstHalfBytes: [UInt8], secondHalfBytes: [UInt8]) {
        self.ctx = context
        self.firstHalfBytes = firstHalfBytes
        self.secondHalfBytes = secondHalfBytes

        // Convert to strings immediately since pointers might be invalid after this point
        self.current = RustStr(ptr: context.current.ptr, len: Int(context.current.len)).toString()
        self.firstBefore = RustStr(ptr: context.first_before.ptr, len: Int(context.first_before.len)).toString()
        self.secondBefore = RustStr(ptr: context.second_before.ptr, len: Int(context.second_before.len)).toString()
        self.firstAfter = RustStr(ptr: context.first_after.ptr, len: Int(context.first_after.len)).toString()
        self.secondAfter = RustStr(ptr: context.second_after.ptr, len: Int(context.second_after.len)).toString()
    }

    deinit {
        DFST_WordContext_freeCurrent(ctx.current)
    }
}
