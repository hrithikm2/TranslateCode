# AGENTS.md — TranslateCode Engine Instructions

## 1. Project Overview & Architecture
TranslateCode converts code across 7 languages (JavaScript, Java, Dart, Swift, Python, Go, Rust) locally via client-side Wasm.
- **Pipeline:** `Source Code` → `AST Parser` → `Universal IR` → `Target Emitter` → `Target Code`.
- **Core Principle:** Strict Hub-and-Spoke. All translations MUST pass through the Universal Intermediate Representation (IR). Never implement direct language-to-language string translations or ad-hoc regex swaps.

---

## 2. Directory Layout & Key Files
- `src/engine/parsers/` — Language-specific AST parsers lowering into Universal IR (`python.ts`, `rust.ts`, `js.ts`, `go.ts`, `java.ts`, `dart.ts`, `swift.ts`).
- `src/engine/ir/` — Universal IR type definitions, nodes, AST walker, and symbol tables (`types.ts`, `nodes.ts`, `builder.ts`).
- `src/engine/emitters/` — Target code generators consuming Universal IR (`python.ts`, `rust.ts`, `js.ts`, `go.ts`, `java.ts`, `dart.ts`, `swift.ts`).
- `src/components/` — UI frontend (Editor, Theme Switcher, Converter Box).
- `scripts/` — Automated test harnesses and matrix runners.

---

## 3. Build & Test Commands
- **Dev Server:** `npm run dev` (Runs Vite on `http://localhost:5173/`).
- **Engine Unit Tests:** `npm test` or `npx vitest run src/engine`.
- **Single Parser Test:** `npx vitest run src/engine/parsers/python.test.ts`.
- **Single Emitter Test:** `npx vitest run src/engine/emitters/rust.test.ts`.
- **Matrix Runner:** `node scripts/test-matrix.mjs --summary-only`.

---

## 4. Universal IR & Type Mapping Rules
When parsing into or emitting from Universal IR:
1. **Dynamic Collections:**
   - Universal Map (`IR_Map`) ↔ JS `Map`/Object, Python `dict`, Java `HashMap`, Dart `Map`, Swift `Dictionary`, Go `map[K]V`, Rust `HashMap<K, V>`.
   - Universal List (`IR_List`) ↔ JS `Array`, Python `list`, Java `ArrayList`, Dart `List`, Swift `Array`, Go `[]T`, Rust `Vec<T>`.
   - Universal Set (`IR_Set`) ↔ JS `Set`, Python `set`, Java `HashSet`, Dart `Set`, Swift `Set`, Go `map[T]struct{}`, Rust `HashSet<T>`.
2. **Indexing & Slicing:**
   - Negative indexes (e.g., Python `arr[-1]`) must lower to `IR_Index(arr, len - 1)`.
   - Slices (`arr[a:b]`) must map to explicit target slice methods (e.g., `.slice()`, `&arr[a..b]`, `arr[a:b]`, `.subList()`).

---

## 5. Strict Emitter Invariants
- **Rust:** Generated code must satisfy the borrow checker. Use explicit `&`/`&mut` references, derive macros (`#[derive(Debug, Clone)]`), and add `.clone()` where ownership transfer would otherwise fail.
- **Go:** Allocate maps/slices with `make()` where sizes are known. Handle multiple returns and zero-values idiomatically.
- **Dart:** Comply strictly with Sound Null Safety (declare non-nullable types, use `?` only when necessary).
- **Swift:** Handle optional unwrapping cleanly (`guard let` / `if let`); avoid force-unwraps (`!`).
- **Java:** Generate valid public wrapper classes, standard method headers, and boxed generic type parameters (`Integer`, not `int`).

---

## 6. Constraints & Coding Rules (Do Not Violate)
- **Smallest Viable Diff:** When fixing a translation bug, patch the respective parser node or emitter function in `src/engine/`. Do not refactor unrelated UI files or rewrite entire modules.
- **No Mock Placeholders:** Do not emit `// TODO: implement` or `any` unless the IR explicitly declares an unknown type.
- **Deterministic Output:** Code formatting must follow canonical styling for the target language (indentation, semicolons, naming cases).