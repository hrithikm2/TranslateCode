use std::collections::{HashMap, HashSet};

enum Mode {
    Fast,
    Safe,
}

trait Solvable {
    fn solve(&self, limit: usize) -> HashMap<i64, usize>;
}

struct Solver {
    values: Vec<i64>,
}

impl Solver {
    const VERSION: i64 = 1;

    fn new(values: Vec<i64>) -> Self {
        Self { values }
    }
}

impl Solvable for Solver {
    fn solve(&self, mut limit: usize) -> HashMap<i64, usize> {
        let mut seen = HashMap::new();
        let unique = HashSet::from_iter(self.values.clone());
        let prefix = self.values[..limit].to_vec();
        for value in &prefix {
            if *value > 0 {
                seen.insert(*value, self.solve(limit - 1).len());
            }
        }
        while limit > 0 {
            limit -= 1;
        }
        seen
    }
}
