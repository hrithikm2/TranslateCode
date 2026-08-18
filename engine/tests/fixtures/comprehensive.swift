import Foundation

enum Mode {
    case fast
    case safe
}

protocol Solvable {
    func solve(_ limit: Int) -> [Int: Int]
}

final class Solver: Solvable {
    static let version = 1
    let values: [Int]

    init(values: [Int]) {
        self.values = values
    }

    func solve(_ limit: Int = 3) -> [Int: Int] {
        var seen: [Int: Int] = [:]
        let unique = Set(values)
        let prefix = Array(values[0..<limit])
        for value in prefix {
            if value > 0 {
                seen[value] = solve(limit - 1).count
            }
        }
        var remaining = limit
        while remaining > 0 {
            remaining -= 1
        }
        return seen
    }
}
