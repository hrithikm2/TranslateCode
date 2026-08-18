import { readFile as read } from "node:fs";

class Solver extends BaseSolver {
  static version = 1;
  values = [];

  constructor(values) {
    this.values = values;
  }

  solve(limit = 3) {
    const seen = new Map();
    const unique = new Set(this.values);
    const prefix = this.values.slice(0, limit);
    for (const value of prefix) {
      if (value > 0) seen.set(value, this.solve(limit - 1));
    }
    while (limit > 0) limit -= 1;
    return seen;
  }
}

export function main() {
  return new Solver([1, 2, 3]).solve();
}
