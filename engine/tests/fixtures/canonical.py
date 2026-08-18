def solve(values: list[int], limit: int) -> dict[int, int]:
    seed = [1, 2, 3]
    seen: dict[int, int] = {}
    unique: set[int] = set(values)
    prefix = values[:limit]
    last = values[-1]
    for value in prefix:
        if value > 0:
            seen[value] = solve(prefix, limit - 1)
    while limit > 0:
        limit -= 1
    return seen
