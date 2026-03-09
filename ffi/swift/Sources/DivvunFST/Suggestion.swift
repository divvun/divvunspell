import Foundation

public struct Suggestion {
    public let value: String
    public let weight: Float
    public let completed: Bool?

    init(value: String, weight: Float, completed: Bool?) {
        self.value = value
        self.weight = weight
        self.completed = completed
    }
}

extension Suggestion: Equatable {
    public static func == (lhs: Suggestion, rhs: Suggestion) -> Bool {
        return lhs.value == rhs.value &&
               lhs.weight == rhs.weight &&
               lhs.completed == rhs.completed
    }
}

extension Suggestion: CustomStringConvertible {
    public var description: String {
        let completedStr = completed.map { $0 ? "completed" : "not completed" } ?? "unknown"
        return "\(value) (weight: \(weight), \(completedStr))"
    }
}
