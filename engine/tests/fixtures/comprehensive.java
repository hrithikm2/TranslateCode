import java.util.List;
import java.util.Map;
import java.util.Set;

interface Solvable<T> {
  Map<T, Integer> solve(int limit);
}

enum Mode { FAST, SAFE }

final class Solver extends BaseSolver implements Solvable<Integer> {
  static final int VERSION = 1;
  private final List<Integer> values;

  Solver(List<Integer> values) {
    this.values = values;
  }

  @Override
  public Map<Integer, Integer> solve(int limit) {
    Map<Integer, Integer> seen = new java.util.HashMap<>();
    Set<Integer> unique = new java.util.HashSet<>(values);
    List<Integer> prefix = values.subList(0, limit);
    for (int value : prefix) {
      if (value > 0) seen.put(value, solve(limit - 1).size());
    }
    while (limit > 0) limit -= 1;
    return seen;
  }
}
