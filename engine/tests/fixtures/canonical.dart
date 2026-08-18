Map<int, int> solve(List<int> values, int limit) {
  final seed = <int>[1, 2, 3];
  final seen = <int, int>{};
  final unique = <int>{1, 2, 3};
  final prefix = values.sublist(0, limit);
  for (final value in prefix) {
    if (value > 0) {
      seen[value] = solve(prefix, limit - 1).length;
    }
  }
  while (limit > 0) {
    limit -= 1;
  }
  return seen;
}
