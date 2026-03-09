import XCTest
@testable import DivvunFST

final class SpellerTests: XCTestCase {
    private let spellerPath = "../../se.bhfst"

    func testOpenArchive() throws {
        let archive = try SpellerArchive.open(path: spellerPath)
        XCTAssertNotNil(archive)
    }

    func testGetSpeller() throws {
        let archive = try SpellerArchive.open(path: spellerPath)
        let speller = try archive.speller()
        XCTAssertNotNil(speller)
    }

    func testGetLocale() throws {
        let archive = try SpellerArchive.open(path: spellerPath)
        let locale = try archive.locale()
        XCTAssertFalse(locale.isEmpty)
    }

    func testIsCorrect() throws {
        let archive = try SpellerArchive.open(path: spellerPath)
        let speller = try archive.speller()

        XCTAssertTrue(try speller.isCorrect("sámegiella"))
        XCTAssertFalse(try speller.isCorrect("sámegiellla"))
    }

    func testSuggest() throws {
        let archive = try SpellerArchive.open(path: spellerPath)
        let speller = try archive.speller()

        let suggestions = try speller.suggest("sámegiellla")
        XCTAssertGreaterThan(suggestions.count, 0)

        let first = suggestions[0]
        XCTAssertFalse(first.value.isEmpty)
    }

    func testSuggestionListIteration() throws {
        let archive = try SpellerArchive.open(path: spellerPath)
        let speller = try archive.speller()

        let suggestions = try speller.suggest("sámegiellla")
        var count = 0
        for suggestion in suggestions {
            XCTAssertFalse(suggestion.value.isEmpty)
            count += 1
        }
        XCTAssertEqual(count, suggestions.count)
    }

    func testSuggestionWeightAndCompleted() throws {
        let archive = try SpellerArchive.open(path: spellerPath)
        let speller = try archive.speller()

        let suggestions = try speller.suggest("sámegiellla")
        XCTAssertGreaterThan(suggestions.count, 0)

        let first = suggestions[0]
        // Weight should be a non-negative number (may be 0.0 for some suggestions)
        XCTAssertGreaterThanOrEqual(first.weight, 0.0)
        // Value should not be empty
        XCTAssertFalse(first.value.isEmpty)
        // Completed field can be nil, true, or false - just verify it's accessible
        let _ = first.completed
    }
}
