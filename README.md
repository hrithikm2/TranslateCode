# TranslateCode

TranslateCode is a zero-backend code transpiler for JavaScript, Java, Dart, Swift, Python, Go, and Rust. Parsing, intermediate-representation construction, and target emission run in a Rust engine compiled to WebAssembly. Source code never leaves the browser.

The compiler-v2 work now uses Tree-sitter and a typed declaration IR. Dart is the first production frontend: the Dart 3.11 grammar parses classes, modifiers, generics, patterns, records, extensions, null-aware syntax, and the other modern language constructs before semantic lowering.

## Supported engine constructs

- Mutable and immutable variable declarations
- Primitive string, integer, float, and boolean types
- Function declarations and calls
- `if`/`else` blocks
- Typed collection literals and cross-language collection iteration
- `for`, `for-each`, and `while` loops with mutability propagation
- Classes, instance methods, constructor calls, and entry-point normalization
- Return statements
- Print statements for every supported language
- Conventional entry-point normalization for Java, Dart, Go, and Rust
- Explicit target-valid diagnostics for constructs that cannot be lowered safely

Every source language is lowered into a language-neutral structural representation before emission. Type spellings are normalized centrally (`list[int]`, `List<int>`, `[]int`, `[Int]`, and `Vec<i64>` all resolve to the same semantic collection type) so dynamic sources do not silently degrade at static targets.

## Run

```bash
npm install
npm run dev
```

The production build recompiles the Rust engine and packages `engine.wasm` with the site:

```bash
npm run build
```

Install the Wasm compilation target and C toolchain once if needed on macOS:

```bash
rustup target add wasm32-wasip1
brew install llvm wasi-libc
```

## Validate

```bash
cargo test --manifest-path engine/Cargo.toml
```

The test suite compiles the complete 49-pair matrix, executes three scalar behavior fixtures across all 49 pairs, executes typed collection iteration across all 49 pairs, and verifies Dart- and Python-origin class/entry-point programs on every target. It also verifies explicit compile-safe fallbacks, the Tree-sitter Dart frontend, typed class/member inventory, and formatter acceptance.

## Architecture

```text
CodeMirror input
      │
      ▼
Tree-sitter source frontend
      │
      ▼
Typed IR (types, classes, members, functions, source spans, diagnostics)
      │
      ▼
Target-specific emitter
      │
      ▼
CodeMirror output
```

The browser bridge copies UTF-8 source into Wasm memory, calls the engine with source and target language IDs, and decodes the emitted UTF-8 result.
