export const LANGUAGE_META = {
  javascript: { name: 'JavaScript', extension: 'js' },
  java: { name: 'Java', extension: 'java' },
  dart: { name: 'Dart', extension: 'dart' },
  swift: { name: 'Swift', extension: 'swift' },
  python: { name: 'Python', extension: 'py' },
  go: { name: 'Go', extension: 'go' },
  rust: { name: 'Rust', extension: 'rs' },
};

const signatures = [
  ['python', [/^\s*def\s+\w+\s*\(/m, /^\s*(from|import)\s+\w+/m, /:\s*\n\s{4}\w+/m, /\bprint\s*\(/], [6, 3, 2, 1]],
  ['rust', [/\bfn\s+\w+\s*\(/, /\blet\s+mut\b/, /->\s*[A-Z_a-z]/, /::[A-Za-z]/], [5, 3, 2, 1]],
  ['go', [/\bpackage\s+main\b/, /\bfunc\s+\w+\s*\(/, /fmt\.Print/, /:=/], [7, 5, 2, 1]],
  ['java', [/\bpublic\s+class\b/, /\bpublic\s+static\s+void\s+main\b/, /\bSystem\.out\.print/, /\b(package|import)\s+[\w.]+;/], [7, 5, 2, 1]],
  ['swift', [/\b(import\s+Foundation|import\s+UIKit)\b/, /\bvar\s+\w+\s*:/, /\bfunc\s+\w+\s*\(/, /\bprint\s*\(/], [5, 2, 3, 1]],
  ['dart', [/\bvoid\s+main\s*\(/, /\bfinal\s+\w+\s*=|\bvar\s+\w+\s*=/, /\bString\b/, /\bprint\s*\(/], [7, 2, 1, 1]],
  ['javascript', [/\b(const|let|var)\s+\w+\s*=/, /\bfunction\s+\w+\s*\(/, /console\.log\s*\(/, /=>/], [3, 4, 2, 1]],
];

export function detectLanguage(source) {
  if (!source.trim()) return { language: 'javascript', confidence: 0, scores: {} };
  const scores = Object.fromEntries(Object.keys(LANGUAGE_META).map((key) => [key, 0]));
  signatures.forEach(([language, patterns, weights]) => patterns.forEach((pattern, index) => {
    if (pattern.test(source)) scores[language] += weights[index];
  }));
  const ranked = Object.entries(scores).sort((a, b) => b[1] - a[1]);
  const [language, score] = ranked[0];
  const second = ranked[1]?.[1] ?? 0;
  const confidence = Math.min(99, score ? Math.max(52, Math.round((score / Math.max(score + second, 1)) * 100)) : 0);
  return { language, confidence, scores };
}
