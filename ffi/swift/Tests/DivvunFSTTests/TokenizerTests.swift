import XCTest
@testable import DivvunFST

final class TokenizerTests: XCTestCase {
    func testCursorContext() {
        let ctx = Tokenizer.cursorContext(firstHalf: "hello ", secondHalf: "world foo")

        XCTAssertEqual(ctx.current, "world")
        XCTAssertEqual(ctx.firstBefore, "hello")
        XCTAssertTrue(ctx.secondBefore.isEmpty)
        XCTAssertEqual(ctx.firstAfter, "foo")
        XCTAssertTrue(ctx.secondAfter.isEmpty)
    }

    func testCursorContextMultipleWords() {
        let ctx = Tokenizer.cursorContext(firstHalf: "one two three ", secondHalf: "four five six")

        XCTAssertEqual(ctx.current, "four")
        XCTAssertEqual(ctx.firstBefore, "three")
        XCTAssertEqual(ctx.secondBefore, "two")
        XCTAssertEqual(ctx.firstAfter, "five")
        XCTAssertEqual(ctx.secondAfter, "six")
    }

    func testCursorContextEmpty() {
        let ctx = Tokenizer.cursorContext(firstHalf: "", secondHalf: "word")

        XCTAssertEqual(ctx.current, "word")
        XCTAssertTrue(ctx.firstBefore.isEmpty)
        XCTAssertTrue(ctx.secondBefore.isEmpty)
    }
}
