from collections.abc import Iterable
from typing import TypeAlias, TypeVar

T = TypeVar("T")
NumberList: TypeAlias = list[int]

class BaseSolver:
    version: int = 1

class Solver(BaseSolver):
    def __init__(self, values: NumberList) -> None:
        self.values = values

    def solve(self, limit: int = 3) -> dict[int, int]:
        seen: dict[int, int] = {}
        unique: set[int] = set(self.values)
        prefix = self.values[:limit]
        last = self.values[-1]
        for value in prefix:
            if value > 0:
                seen[value] = self.solve(limit - 1)
        while limit > 0:
            limit -= 1
        return seen

def main() -> dict[int, int]:
    return Solver([1, 2, 3]).solve()
