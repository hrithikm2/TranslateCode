package algorithms

func solve(values []int, limit int) map[int]int {
    seed := []int{1, 2, 3}
    seen := make(map[int]int)
    unique := make(map[int]struct{})
    prefix := values[:limit]
    for _, value := range prefix {
        if value > 0 {
            seen[value] = solve(prefix, limit-1)
        }
    }
    for limit > 0 {
        limit -= 1
    }
    return seen
}
