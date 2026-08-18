fn solve(values: Vec<i64>, mut limit: usize) -> std::collections::HashMap<i64, usize> {
    let seed = [1, 2, 3];
    let mut seen = std::collections::HashMap::new();
    let unique = std::collections::HashSet::from_iter(values.clone());
    let prefix = values[..limit].to_vec();
    for value in &prefix {
        if *value > 0 {
            seen.insert(*value, solve(prefix.clone(), limit - 1).len());
        }
    }
    while limit > 0 {
        limit -= 1;
    }
    seen
}
