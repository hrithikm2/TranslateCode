export const LANGUAGE_META = {
  javascript: { name: 'JavaScript', extension: 'js' },
  java: { name: 'Java', extension: 'java' },
  dart: { name: 'Dart', extension: 'dart' },
  swift: { name: 'Swift', extension: 'swift' },
  python: { name: 'Python', extension: 'py' },
  go: { name: 'Go', extension: 'go' },
  rust: { name: 'Rust', extension: 'rs' },
};

// Strong features are close to unique; weak features are shared syntax.
// Negative features prevent common constructs such as print() or => from
// overwhelming more distinctive evidence.
const signatures = {
  python: {
    strong: [[/^\s*def\s+\w+\s*\([^)]*\)\s*:/m, 10], [/^\s*(from|import)\s+[\w.]+/m, 6], [/^\s*class\s+\w+\s*(\([^)]*\))?\s*:/m, 7], [/\b(True|False|None)\b/, 5]],
    weak: [[/^\s*(elif|except|finally|with|async\s+def)\b/m, 3], [/:\s*\n\s{2,}\w+/m, 2], [/\bprint\s*\(/, 1]],
    negative: [[/\bfn\s+\w+\s*\(/, 8], [/\bpackage\s+main\b/, 8], [/\b(let\s+mut|func\s+\w+\s*\()/, 6]],
  },
  rust: {
    strong: [[/\bfn\s+\w+\s*\([^)]*\)\s*(?:->\s*[^\{]+)?\s*\{/, 12], [/\b(use|pub)\s+[\w:]+(?:::\w+)+\s*;/, 9], [/\blet\s+(?:mut\s+)?\w+\s*:/, 7], [/\bimpl(?:<[^>]+>)?\s+\w+/, 7]],
    weak: [[/\blet\s+mut\b/, 4], [/::[A-Za-z_]/, 3], [/\b(match|struct|enum|trait)\s+\w+/, 3], [/\b(Result|Option|Vec|String)<[^>]+>/, 2]],
    negative: [[/^\s*def\s+\w+\s*\(/m, 8], [/\bpackage\s+main\b/, 8], [/\bvoid\s+main\s*\(/, 6]],
  },
  go: {
    strong: [[/^\s*package\s+\w+\s*$/m, 14], [/\bfunc\s+\w+\s*\([^)]*\)/, 8], [/\b(?:go|defer)\s+\w+/, 7]],
    weak: [[/:=/, 4], [/\b(?:fmt|http|context)\.\w+/, 3], [/\b(chan|interface|goroutine)\b/, 3]],
    negative: [[/\bfn\s+\w+\s*\(/, 8], [/^\s*def\s+\w+\s*\(/m, 8], [/\bvoid\s+main\s*\(/, 6]],
  },
  java: {
    strong: [[/\bpublic\s+(?:abstract\s+|final\s+)?class\s+\w+/, 12], [/\bpublic\s+static\s+void\s+main\s*\(/, 14], [/\bSystem\.out\.(?:print|println)\s*\(/, 9], [/^\s*package\s+[\w.]+\s*;/m, 8]],
    weak: [[/^\s*(?:public|private|protected)\s+(?:static\s+)?[A-Z][\w<>\[\]]*\s+\w+\s*[=(;]/m, 3], [/\bnew\s+[A-Z]\w*\s*\(/, 2]],
    negative: [[/^\s*package\s+main\s*$/m, 10], [/\bfn\s+\w+\s*\(/, 7], [/^\s*def\s+\w+\s*\(/m, 7]],
  },
  swift: {
    strong: [[/^\s*import\s+(Foundation|UIKit|SwiftUI)\s*$/m, 12], [/\b(?:struct|class|enum)\s+\w+\s*:\s*[A-Z]\w*/, 7], [/\bguard\s+.+\s+else\s*\{/, 7]],
    weak: [[/\b(?:let|var)\s+\w+\s*:/, 3], [/\b(?:func|protocol|extension)\s+\w+/, 3], [/\b(?:String|Int|Bool|Double)\??\b/, 2]],
    negative: [[/\bfn\s+\w+\s*\(/, 8], [/\bpackage\s+main\b/, 8], [/^\s*def\s+\w+\s*\(/m, 7]],
  },
  dart: {
    strong: [[/\bimport\s+['"](?:dart|package):/, 13], [/\bvoid\s+main\s*\(/, 13], [/\b(?:Future|Stream)<[^>]+>/, 8], [/\b(?:required|late|mixin|extension)\b/, 6], [/\b(?:Map|List|Set)<[A-Za-z_$][\w$]*(?:\s*,\s*[A-Za-z_$][\w$]*)?>/, 8], [/\b(?:String|bool|int|double|num|dynamic)\s+\w+\s*\([^)]*\)\s*\{/, 8], [/\b(?:class|abstract\s+class|final\s+class)\s+\w+\s*\{/, 7]],
    weak: [[/\b(?:final|const)\s+(?:[A-Za-z_$][\w$]*\s+)?[A-Za-z_$][\w$]*\s*=/, 4], [/\b(?:String|bool|int|double|num|dynamic)\s+\w+\s*=/, 3], [/\b\w+\?\?\s*[^=]/, 3], [/\.(?:every|contains|isEmpty|toString)\s*\(/, 3], [/\bprint\s*\(/, 1], [/@[A-Za-z_$][\w$]*/, 1]],
    negative: [[/\bfn\s+\w+\s*\(/, 10], [/\bpackage\s+main\b/, 8], [/^\s*def\s+\w+\s*\(/m, 7]],
  },
  javascript: {
    strong: [[/\b(?:const|let|var)\s+\w+\s*=\s*(?:async\s*)?\(?[^\n]+=>/, 8], [/\b(?:console\.log|require\s*\(|module\.exports)\b/, 8], [/\b(?:function\s+\w+|export\s+(?:default\s+)?(?:function|class|const))\b/, 7]],
    weak: [[/\b(?:const|let|var)\s+\w+\s*=/, 3], [/=>/, 2], [/\b(?:undefined|null|true|false)\b/, 1]],
    negative: [[/\bfn\s+\w+\s*\(/, 8], [/^\s*def\s+\w+\s*\(/m, 8], [/^\s*package\s+main\s*$/m, 8], [/\bvoid\s+main\s*\(/, 6]],
  },
};

function withoutNoise(source) {
  return source
    .replace(/\/\*[\s\S]*?\*\//g, ' ')
    .replace(/\/\/[^\n]*/g, ' ')
    .replace(/^\s*#(?!\s*!)[^\n]*/gm, ' ')
    .trim();
}

export function detectLanguage(source) {
  const clean = withoutNoise(source);
  if (!clean) return { language: 'javascript', confidence: 0, reliable: false, scores: {}, evidence: [] };

  const scores = Object.fromEntries(Object.keys(LANGUAGE_META).map((key) => [key, 0]));
  const evidence = [];
  for (const [language, groups] of Object.entries(signatures)) {
    for (const [pattern, weight] of groups.strong) {
      if (pattern.test(clean)) { scores[language] += weight; evidence.push({ language, weight }); }
    }
    for (const [pattern, weight] of groups.weak) if (pattern.test(clean)) scores[language] += weight;
    for (const [pattern, weight] of groups.negative) if (pattern.test(clean)) scores[language] -= weight;
  }

  const ranked = Object.entries(scores).sort((a, b) => b[1] - a[1]);
  const [language, rawScore] = ranked[0];
  const second = ranked[1]?.[1] ?? 0;
  const score = Math.max(rawScore, 0);
  const margin = score - Math.max(second, 0);
  const confidence = score === 0 ? 0 : Math.min(99, Math.round(55 + (margin / Math.max(score, 1)) * 44));
  const reliable = score >= 8 && margin >= 4 && confidence >= 70;
  return { language, confidence, reliable, scores, evidence: evidence.filter((item) => item.language === language) };
}
