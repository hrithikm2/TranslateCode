package algorithms

import "fmt"

type Mode int

type Solvable interface {
    Solve(limit int) map[int]int
}

const (
    Fast Mode = iota
    Safe
)

type Solver struct {
    values []int
}

func NewSolver(values []int) *Solver {
    return &Solver{values: values}
}

func (solver *Solver) Solve(limit int) map[int]int {
    seen := make(map[int]int)
    unique := make(map[int]struct{})
    prefix := solver.values[:limit]
    for _, value := range prefix {
        if value > 0 {
            seen[value] = len(solver.Solve(limit - 1))
        }
    }
    for limit > 0 {
        limit -= 1
    }
    fmt.Println(unique)
    return seen
}
