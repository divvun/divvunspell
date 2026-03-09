import Foundation
import CDivvunFST

public enum Tokenizer {
    public static func cursorContext(firstHalf: String, secondHalf: String) -> WordContext {
        let firstBytes = Array(firstHalf.utf8)
        let secondBytes = Array(secondHalf.utf8)

        let ctx = firstBytes.withUnsafeBytes { firstPtr in
            secondBytes.withUnsafeBytes { secondPtr in
                DFST_Tokenizer_cursorContext(
                    firstPtr.baseAddress!.assumingMemoryBound(to: UInt8.self),
                    UInt(firstBytes.count),
                    secondPtr.baseAddress!.assumingMemoryBound(to: UInt8.self),
                    UInt(secondBytes.count)
                )
            }
        }

        return WordContext(context: ctx, firstHalfBytes: firstBytes, secondHalfBytes: secondBytes)
    }
}
