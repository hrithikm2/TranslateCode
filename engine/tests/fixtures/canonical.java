class Algorithms {
  static Map<Integer, Integer> solve(List<Integer> values, int limit) {
    List<Integer> seed = List.of(1, 2, 3);
    Map<Integer, Integer> seen = new HashMap<>();
    Set<Integer> unique = new HashSet<>(values);
    List<Integer> prefix = values.subList(0, limit);
    for (int value : prefix) {
      if (value > 0) seen.put(value, solve(prefix, limit - 1));
    }
    while (limit > 0) limit -= 1;
    return seen;
  }
}
