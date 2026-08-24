import React, { useEffect, useMemo, useRef, useState } from 'react';
import { createRoot } from 'react-dom/client';
import CodeMirror from '@uiw/react-codemirror';
import { javascript } from '@codemirror/lang-javascript';
import { python } from '@codemirror/lang-python';
import { siDart, siGo, siJavascript, siPython, siRust, siSwift } from 'simple-icons';
import javaLogo from 'devicon/icons/java/java-original.svg';
import { detectLanguage, LANGUAGE_META } from './languageDetection.js';
import { transpile, warmEngine } from './wasmEngine.js';
import './styles.css';

const DONATION_URL = import.meta.env.VITE_DONATION_URL || 'https://github.com/sponsors/hrithikm2';

const LANGUAGE_THEMES = {
  python: { logo: siPython, primary: '#3776AB', secondary: '#FFD43B', rgb: '55, 118, 171' },
  javascript: { logo: siJavascript, primary: '#F7DF1E', secondary: '#FFEA70', rgb: '247, 223, 30' },
  typescript: { icon: 'TS', primary: '#3178C6', secondary: '#76B7F0', rgb: '49, 120, 198' },
  rust: { logo: siRust, primary: '#CE412B', secondary: '#DEA584', rgb: '206, 65, 43' },
  go: { logo: siGo, primary: '#00ADD8', secondary: '#66D9EF', rgb: '0, 173, 216' },
  cpp: { icon: 'C+', primary: '#00599C', secondary: '#659AD2', rgb: '0, 89, 156' },
  csharp: { icon: 'C#', primary: '#9B4F96', secondary: '#239120', rgb: '155, 79, 150' },
  ruby: { icon: 'Rb', primary: '#CC342D', secondary: '#F06D65', rgb: '204, 52, 45' },
  php: { icon: 'php', primary: '#777BB4', secondary: '#B0B3D6', rgb: '119, 123, 180' },
  java: { image: javaLogo, primary: '#E76F00', secondary: '#5382A1', rgb: '231, 111, 0' },
  dart: { logo: siDart, primary: '#0175C2', secondary: '#13B9FD', rgb: '1, 117, 194' },
  swift: { logo: siSwift, primary: '#F05138', secondary: '#FFAC45', rgb: '240, 81, 56' },
};

const starterCode = `def greet(name):
    message = "Hello, " + name
    print(message)

greet("world")`;

const DSA_EXAMPLES = [
  {
    id: 'two-sum',
    title: 'Two Sum',
    topic: 'Arrays · Hash map',
    complexity: 'O(n) time',
    code: `def two_sum(nums, target):
    seen = {}

    for index, value in enumerate(nums):
        complement = target - value
        if complement in seen:
            return [seen[complement], index]
        seen[value] = index

    return []`,
  },
  {
    id: 'binary-search',
    title: 'Binary Search',
    topic: 'Arrays · Search',
    complexity: 'O(log n) time',
    code: `def binary_search(nums, target):
    left, right = 0, len(nums) - 1

    while left <= right:
        middle = (left + right) // 2
        if nums[middle] == target:
            return middle
        if nums[middle] < target:
            left = middle + 1
        else:
            right = middle - 1

    return -1`,
  },
  {
    id: 'valid-parentheses',
    title: 'Valid Parentheses',
    topic: 'Strings · Stack',
    complexity: 'O(n) time',
    code: `def is_valid_parentheses(text):
    pairs = {')': '(', ']': '[', '}': '{'}
    stack = []

    for character in text:
        if character in pairs.values():
            stack.append(character)
        elif not stack or stack.pop() != pairs[character]:
            return False

    return not stack`,
  },
];

function themeFor(language) {
  return LANGUAGE_THEMES[language] ?? LANGUAGE_THEMES.javascript;
}

function themeVariables(prefix, theme) {
  return {
    [`--${prefix}-accent`]: theme.primary,
    [`--${prefix}-secondary`]: theme.secondary,
    [`--${prefix}-rgb`]: theme.rgb,
  };
}

function shouldCopySourceUnchanged(sourceLanguage, targetLanguage, detected) {
  if (sourceLanguage === targetLanguage) return true;
  return sourceLanguage === 'auto'
    && detected.confidence > 90
    && detected.language === targetLanguage;
}

function LanguageBadge({ language, side }) {
  const theme = themeFor(language);
  return (
    <span className={`language-icon ${side}-language-icon`} style={{ color: theme.primary }} aria-hidden="true">
      {theme.image
        ? <img src={theme.image} alt="" />
        : theme.logo
          ? <svg viewBox="0 0 24 24" focusable="false"><path d={theme.logo.path} fill="currentColor" /></svg>
          : <span>{theme.icon}</span>}
    </span>
  );
}

function LanguageSelect({ id, label, value, options, side, onChange }) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef(null);
  const triggerRef = useRef(null);
  const optionRefs = useRef([]);
  const selectedIndex = Math.max(0, options.findIndex((option) => option.value === value));
  const selected = options[selectedIndex] ?? options[0];

  useEffect(() => {
    if (!open) return undefined;
    const closeOnOutsidePress = (event) => {
      if (!rootRef.current?.contains(event.target)) setOpen(false);
    };
    const closeOnEscape = (event) => {
      if (event.key !== 'Escape') return;
      setOpen(false);
      triggerRef.current?.focus();
    };
    document.addEventListener('pointerdown', closeOnOutsidePress);
    document.addEventListener('keydown', closeOnEscape);
    window.requestAnimationFrame(() => optionRefs.current[selectedIndex]?.focus());
    return () => {
      document.removeEventListener('pointerdown', closeOnOutsidePress);
      document.removeEventListener('keydown', closeOnEscape);
    };
  }, [open, selectedIndex]);

  function choose(option) {
    onChange(option.value);
    setOpen(false);
    triggerRef.current?.focus();
  }

  function navigateOptions(event) {
    if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) return;
    event.preventDefault();
    const focusedIndex = optionRefs.current.indexOf(document.activeElement);
    const nextIndex = event.key === 'Home'
      ? 0
      : event.key === 'End'
        ? options.length - 1
        : event.key === 'ArrowDown'
          ? (focusedIndex + 1 + options.length) % options.length
          : (focusedIndex - 1 + options.length) % options.length;
    optionRefs.current[nextIndex]?.focus();
  }

  return (
    <div className={`language-picker ${open ? 'is-open' : ''}`} ref={rootRef}>
      <button
        ref={triggerRef}
        id={id}
        className="language-trigger"
        type="button"
        aria-label={label}
        aria-haspopup="listbox"
        aria-expanded={open}
        onClick={() => setOpen((current) => !current)}
        onKeyDown={(event) => {
          if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
            event.preventDefault();
            setOpen(true);
          }
        }}
      >
        <LanguageBadge language={selected.language} side={side} />
        <span className="language-trigger-label">{selected.label}</span>
        <span className="language-chevron" aria-hidden="true">⌄</span>
      </button>
      {open && (
        <div className="language-options" role="listbox" aria-label={label} onKeyDown={navigateOptions}>
          {options.map((option, index) => (
            <button
              ref={(node) => { optionRefs.current[index] = node; }}
              type="button"
              role="option"
              aria-selected={option.value === value}
              className={option.value === value ? 'is-selected' : ''}
              key={option.value}
              onClick={() => choose(option)}
            >
              <LanguageBadge language={option.language} side={side} />
              <span>{option.label}</span>
              <i aria-hidden="true">{option.value === value ? '✓' : ''}</i>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

function Editor({ value, onChange, language, readOnly = false, onCursorChange }) {
  const extension = language === 'python' ? python() : javascript();
  return (
    <CodeMirror
      value={value}
      height="430px"
      theme="dark"
      extensions={[extension]}
      editable={!readOnly}
      onChange={onChange}
      onUpdate={(update) => {
        if (!onCursorChange || !update.selectionSet) return;
        const head = update.state.selection.main.head;
        const line = update.state.doc.lineAt(head);
        onCursorChange({ line: line.number, column: head - line.from + 1 });
      }}
      basicSetup={{ lineNumbers: true, foldGutter: true, autocompletion: true, highlightActiveLine: true }}
    />
  );
}

function App() {
  const [source, setSource] = useState(starterCode);
  const [sourceLanguage, setSourceLanguage] = useState('auto');
  const [targetLanguage, setTargetLanguage] = useState('javascript');
  const [output, setOutput] = useState('');
  const [copied, setCopied] = useState(false);
  const [isTranslating, setIsTranslating] = useState(false);
  const [benchmark, setBenchmark] = useState('Ready');
  const [sourceCursor, setSourceCursor] = useState({ line: 1, column: 1 });
  const [outputCursor, setOutputCursor] = useState({ line: 1, column: 1 });
  const [engineStatus, setEngineStatus] = useState('Ready');
  const [pendingExample, setPendingExample] = useState(null);
  const [exampleNotice, setExampleNotice] = useState(null);

  const detected = useMemo(() => detectLanguage(source), [source]);
  const activeSourceLanguage = sourceLanguage === 'auto' ? detected.language : sourceLanguage;
  const sourceTheme = themeFor(activeSourceLanguage);
  const targetTheme = themeFor(targetLanguage);
  const sourceMismatch = sourceLanguage !== 'auto' && detected.reliable && detected.language !== sourceLanguage;
  const detectionUncertain = Boolean(source.trim()) && !detected.reliable;
  const copySourceUnchanged = shouldCopySourceUnchanged(sourceLanguage, targetLanguage, detected);
  const sourceValidationMessage = sourceMismatch
    ? `This appears to be ${LANGUAGE_META[detected.language].name}. Select the correct source language or provide ${LANGUAGE_META[sourceLanguage].name} code.`
    : detectionUncertain
      ? 'Language detection is uncertain. Select the input language before converting.'
      : null;
  const shellTheme = {
    ...themeVariables('source', sourceTheme),
    ...themeVariables('target', targetTheme),
  };
  const sourceLanguageOptions = [
    { value: 'auto', label: `Auto · ${LANGUAGE_META[detected.language].name}`, language: detected.language },
    ...Object.entries(LANGUAGE_META).map(([id, meta]) => ({ value: id, label: meta.name, language: id })),
  ];
  const targetLanguageOptions = Object.entries(LANGUAGE_META)
    .map(([id, meta]) => ({ value: id, label: meta.name, language: id }));

  useEffect(() => {
    if (!copySourceUnchanged) return;
    setOutput(source);
    setBenchmark('No changes needed');
    setEngineStatus('Languages match');
  }, [copySourceUnchanged, source]);

  useEffect(() => {
    if (!exampleNotice) return undefined;
    const timeout = window.setTimeout(() => setExampleNotice(null), 5200);
    return () => window.clearTimeout(timeout);
  }, [exampleNotice]);

  useEffect(() => {
    if (!pendingExample) return undefined;
    const handleKeyDown = (event) => {
      if (event.key === 'Escape') setPendingExample(null);
    };
    const previousOverflow = document.body.style.overflow;
    document.body.style.overflow = 'hidden';
    window.addEventListener('keydown', handleKeyDown);
    return () => {
      document.body.style.overflow = previousOverflow;
      window.removeEventListener('keydown', handleKeyDown);
    };
  }, [pendingExample]);

  async function convert(nextTargetLanguage = targetLanguage) {
    if (shouldCopySourceUnchanged(sourceLanguage, nextTargetLanguage, detected)) {
      setOutput(source);
      setBenchmark('No changes needed');
      setEngineStatus('Languages match');
      return;
    }
    if (sourceMismatch) {
      setEngineStatus(sourceValidationMessage);
      return;
    }
    const startedAt = performance.now();
    setIsTranslating(true);
    setEngineStatus('Translating…');
    try {
      setOutput(await transpile(source, activeSourceLanguage, nextTargetLanguage));
      const elapsed = performance.now() - startedAt;
      setBenchmark(`${Math.max(elapsed, 0.1).toFixed(1)} ms`);
      setEngineStatus('Translation ready');
    } catch (error) {
      setEngineStatus('Translation unavailable');
      setBenchmark('Couldn’t translate');
    } finally {
      setIsTranslating(false);
    }
  }

  async function copyCode() {
    if (!output) return;
    await navigator.clipboard.writeText(output);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1600);
  }

  function loadExample(example) {
    setSource(example.code);
    setSourceLanguage('python');
    setOutput('');
    setBenchmark('Ready');
    setEngineStatus('Example loaded');
    setSourceCursor({ line: 1, column: 1 });
    setOutputCursor({ line: 1, column: 1 });
    setExampleNotice({
      title: `${example.title} is ready`,
      message: 'Select a target language to translate.',
    });
    window.requestAnimationFrame(() => window.scrollTo({ top: 0, behavior: 'smooth' }));
  }

  function requestExample(example) {
    if (source.trim()) {
      setPendingExample(example);
      return;
    }
    loadExample(example);
  }

  function confirmExample() {
    if (!pendingExample) return;
    const example = pendingExample;
    setPendingExample(null);
    loadExample(example);
  }

  return (
    <main className="app-shell" style={shellTheme}>
      <div className="mesh-background" aria-hidden="true">
        <span className="mesh-blob mesh-source" />
        <span className="mesh-blob mesh-target" />
        <span className="mesh-blob mesh-blend" />
        <span className="mesh-blob mesh-highlight" />
        <span className="mesh-wave mesh-wave-one" />
        <span className="mesh-wave mesh-wave-two" />
        <span className="mesh-wave mesh-wave-three" />
      </div>

      {exampleNotice && (
        <div className="example-notice" role="status" aria-live="polite">
          <span className="example-notice-icon" aria-hidden="true">✓</span>
          <span className="example-notice-copy"><strong>{exampleNotice.title}</strong><small>{exampleNotice.message}</small></span>
          <button type="button" onClick={() => setExampleNotice(null)} aria-label="Dismiss message">×</button>
        </div>
      )}

      <nav className="topbar">
        <a className="brand" href="/" aria-label="Back to workspace chooser">
          <span className="brand-mark"><img src="/translatecode-mark.svg" alt="" /></span>
          <span>Translate<span className="brand-light">Code</span></span>
        </a>
        <div className="nav-status" role="status" aria-live="polite"><span className="pulse" /><span>{engineStatus}</span></div>
      </nav>

      <section className="hero" id="top">
        <div className="hero-copy-block">
          <p className="eyebrow"><span>TC</span> Built for developers</p>
          <h1>Translate code.<br /><em>Keep the intent.</em></h1>
        </div>
        <div className="hero-aside">
          <p>Move between programming languages with a focused workspace designed for clear, consistent results.</p>
          <div className="hero-metrics"><span><b>Source-aware</b> input</span><span><b>Target-ready</b> output</span><span><b>Free</b> forever</span></div>
        </div>
      </section>

      <section className="translator-shell" aria-label="Code translator">
        <div className="pane-grid">
          <section className="editor-pane source-pane">
            <header className="pane-header">
              <div className="pane-title-row">
                <span className="pane-kicker">Source</span>
                <LanguageSelect id="source-language" label="Source language" value={sourceLanguage} options={sourceLanguageOptions} side="source" onChange={setSourceLanguage} />
              </div>
              <div className="pane-meta">
                <span className={`confidence-pill ${sourceValidationMessage ? 'confidence-error' : ''}`}>{sourceLanguage === 'auto' ? `${detected.confidence}% confidence` : 'selected'}</span>
                <span className="cursor-position">Ln {sourceCursor.line}, Col {sourceCursor.column}</span>
              </div>
            </header>
            <div className="editor-surface">
              <Editor value={source} onChange={setSource} language={activeSourceLanguage} onCursorChange={setSourceCursor} />
            </div>
            {sourceValidationMessage && <p className="validation-message" role="alert">{sourceValidationMessage}</p>}
            <footer className="pane-footer"><span>SOURCE / UTF-8</span><span>{source.split('\n').length} lines · {source.length} characters</span></footer>
          </section>

          <div className="pipeline-connector" aria-hidden="true"><span className="translation-arrow">→</span></div>

          <section className="editor-pane output-pane">
            <header className="pane-header">
              <div className="pane-title-row">
                <span className="pane-kicker">Target</span>
                <LanguageSelect id="target-language" label="Target language" value={targetLanguage} options={targetLanguageOptions} side="target" onChange={(nextTargetLanguage) => {
                    setTargetLanguage(nextTargetLanguage);
                    convert(nextTargetLanguage);
                  }} />
              </div>
              <div className="pane-meta">
                <span className="benchmark-pill"><i />{benchmark}</span>
                <button className={`copy-button ${copied ? 'is-copied' : ''}`} onClick={copyCode} disabled={!output}>
                  <span className="copy-glyph" aria-hidden="true">{copied ? '✓' : '⧉'}</span>{copied ? 'Copied' : 'Copy code'}
                </button>
              </div>
            </header>
            <div className="editor-surface output-surface">
              {output
                ? <Editor value={output} onChange={() => {}} language={targetLanguage} readOnly onCursorChange={setOutputCursor} />
                : <div className="empty-output"><div className="empty-orbit"><span aria-hidden="true">{'</>'}</span></div><strong>Ready to translate</strong><p>Your translated code will appear here.</p></div>}
            </div>
            <footer className="pane-footer"><span>RESULT / {LANGUAGE_META[targetLanguage].extension.toUpperCase()}</span><span>{output ? `Ln ${outputCursor.line}, Col ${outputCursor.column}` : 'Ready when you are'}</span></footer>
          </section>
        </div>

        <div className="action-bar">
          <div className="pipeline-status"><span className="pipeline-led" /><b>Source</b><i>→</i><b>Translate</b><i>→</i><b>Result</b><span className="privacy-note">Processed privately</span></div>
          <button className={`convert-button ${isTranslating ? 'is-loading' : ''}`} onClick={() => convert()} disabled={isTranslating}>
            <span>{isTranslating ? 'Translating…' : copySourceUnchanged ? 'Use source code' : 'Translate code'}</span>
            <i aria-hidden="true">{isTranslating ? '···' : '⌘ ↵'}</i>
          </button>
        </div>
      </section>

      <section className="use-cases-section" aria-labelledby="use-cases-title">
        <header className="use-cases-heading">
          <div>
            <span>Before you translate</span>
            <h2 id="use-cases-title">Where TranslateCode fits.</h2>
          </div>
          <p>Best results come from self-contained code whose behavior can be understood without the rest of a project.</p>
        </header>
        <div className="use-case-grid">
          <article className="use-case-card use-case-ideal">
            <span className="use-case-label"><i aria-hidden="true">✓</i> Ideal use cases</span>
            <h3>Focused, language-level code</h3>
            <ul>
              <li>Small functions, utility classes, and reusable helpers</li>
              <li>Algorithms and data-structure solutions</li>
              <li>Standard collections, conditions, loops, slices, and recursion</li>
              <li>Self-contained examples with no framework or project dependencies</li>
            </ul>
          </article>
          <article className="use-case-card use-case-caution">
            <span className="use-case-label"><i aria-hidden="true">!</i> Not ideal use cases</span>
            <h3>Project- or runtime-dependent code</h3>
            <ul>
              <li>Entire applications, repositories, or multi-file features</li>
              <li>Framework code such as Flutter, React, Spring, or SwiftUI</li>
              <li>Build configuration, dependency injection, generated code, or platform APIs</li>
              <li>Macros, reflection, FFI, or runtime-specific concurrency and memory behavior</li>
            </ul>
          </article>
        </div>
        <p className="use-cases-note"><strong>Always review the result.</strong> Format, compile, and test translated code in the target language's own toolchain.</p>
      </section>

      <section className="examples-section" aria-labelledby="examples-title">
        <header className="examples-heading">
          <div><span>Python examples</span><h2 id="examples-title">Start with a familiar problem.</h2></div>
          <p>Load a complete DSA solution into the workspace, choose your target language, and continue from there.</p>
        </header>
        <div className="example-grid">
          {DSA_EXAMPLES.map((example) => (
            <article className="example-card" key={example.id}>
              <header className="example-card-header">
                <div><span>{example.topic}</span><h3>{example.title}</h3></div>
                <small>{example.complexity}</small>
              </header>
              <pre aria-label={`${example.title} Python code`}>
                {example.code.split('\n').map((line, index) => (
                  <span className="example-code-line" key={`${example.id}-${index}`}>
                    <i>{String(index + 1).padStart(2, '0')}</i><code>{line || ' '}</code>
                  </span>
                ))}
              </pre>
              <footer className="example-card-footer">
                <span>Python</span>
                <button type="button" onClick={() => requestExample(example)}>Translate <i aria-hidden="true">↗</i></button>
              </footer>
            </article>
          ))}
        </div>
      </section>

      <footer className="site-footer">
        <span>TranslateCode · Free forever</span>
        <a href={DONATION_URL} target="_blank" rel="noreferrer">Donate to support development <i aria-hidden="true">↗</i></a>
      </footer>

      {pendingExample && (
        <div className="confirmation-layer" role="presentation" onMouseDown={(event) => {
          if (event.target === event.currentTarget) setPendingExample(null);
        }}>
          <section className="confirmation-dialog" role="dialog" aria-modal="true" aria-labelledby="confirmation-title" aria-describedby="confirmation-copy">
            <span className="confirmation-eyebrow">Replace source code</span>
            <h2 id="confirmation-title">Clear your current code?</h2>
            <p id="confirmation-copy">Loading <strong>{pendingExample.title}</strong> will replace everything currently in the Source editor.</p>
            <div className="confirmation-actions">
              <button type="button" className="confirmation-cancel" onClick={() => setPendingExample(null)}>Keep current code</button>
              <button type="button" className="confirmation-confirm" onClick={confirmExample}>Clear and load example</button>
            </div>
          </section>
        </div>
      )}
    </main>
  );
}

createRoot(document.getElementById('root')).render(<App />);

warmEngine().catch(() => {});
