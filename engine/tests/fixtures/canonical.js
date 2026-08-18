function solve(values, limit) {
  const seed = [1, 2, 3];
  const seen = new Map();
  const unique = new Set(values);
  const prefix = values.slice(0, limit);
  for (const value of prefix) {
    if (value > 0) seen.set(value, solve(prefix, limit - 1));
  }
  while (limit > 0) limit -= 1;
  return seen;
}
