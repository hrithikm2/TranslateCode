func solve(_ values: [Int], _ limit: Int) -> [Int: Int] {
    let seed = [1, 2, 3]
    var seen: [Int: Int] = [:]
    let unique = Set(values)
    let prefix = Array(values[0..<limit])
    for value in prefix {
        if value > 0 {
            seen[value] = solve(prefix, limit - 1).count
        }
    }
    var remaining = limit
    while remaining > 0 {
        remaining -= 1
    }
    return seen
}
