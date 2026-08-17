import React, { useMemo, useState } from 'react';
import { createRoot } from 'react-dom/client';
import CodeMirror from '@uiw/react-codemirror';
import { javascript } from '@codemirror/lang-javascript';
import { python } from '@codemirror/lang-python';
import { detectLanguage, LANGUAGE_META } from './languageDetection.js';
import { transpile, warmEngine } from './wasmEngine.js';
import './styles.css';

const starterCode = `def greet(name):
    message = "Hello, " + name
    print(message)

greet("world")`;

function Editor({ value, onChange, language, readOnly = false }) {
  const extension = language === 'python' ? python() : javascript();
  return (
    <CodeMirror
      value={value}
      height="390px"
      theme="dark"
      extensions={[extension]}
      editable={!readOnly}
      onChange={onChange}
      basicSetup={{ lineNumbers: true, foldGutter: true, autocompletion: true }}
    />
  );
}

function App() {
  const [source, setSource] = useState(starterCode);
  const [sourceLanguage, setSourceLanguage] = useState('auto');
  const [targetLanguage, setTargetLanguage] = useState('javascript');
  const [output, setOutput] = useState('');
  const [copied, setCopied] = useState(false);
  const [engineStatus, setEngineStatus] = useState('Wasm engine ready');
  const detected = useMemo(() => detectLanguage(source), [source]);
  const activeSourceLanguage = sourceLanguage === 'auto' ? detected.language : sourceLanguage;

  async function convert() {
    setEngineStatus('Translating…');
    try {
      setOutput(await transpile(source, activeSourceLanguage, targetLanguage));
      setEngineStatus('Translated locally with Rust + Wasm');
    } catch (error) {
      setEngineStatus(error.message);
    }
  }

  async function copyCode() {
    if (!output) return;
    await navigator.clipboard.writeText(output);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1600);
  }

  return (
    <main className="shell">
      <nav className="topbar">
        <a className="brand" href="#top" aria-label="TranslateCode home">
          <span className="brand-mark">T</span>
          <span>Translate<span className="brand-light">Code</span></span>
        </a>
        <div className="nav-note"><span className="pulse" /> {engineStatus}</div>
      </nav>

      <section className="hero" id="top">
        <div>
          <p className="eyebrow">The universal code translator</p>
          <h1>Write once.<br /><em>Translate anywhere.</em></h1>
        </div>
        <p className="hero-copy">Convert core programming constructs between modern languages, privately and instantly. No server. No uploads. Just your code, in your browser.</p>
      </section>

      <section className="translator-card" aria-label="Code translator">
        <div className="pane-grid">
          <section className="pane">
            <header className="pane-header">
              <div className="language-control">
                <label htmlFor="source-language">From</label>
                <select id="source-language" value={sourceLanguage} onChange={(event) => setSourceLanguage(event.target.value)}>
                  <option value="auto">Auto-Detect</option>
                  {Object.entries(LANGUAGE_META).map(([id, meta]) => <option value={id} key={id}>{meta.name}</option>)}
                </select>
              </div>
              <span className="detected">{sourceLanguage === 'auto' ? `${detected.confidence}% match · ${LANGUAGE_META[detected.language].name}` : 'Manual selection'}</span>
            </header>
            <Editor value={source} onChange={setSource} language={activeSourceLanguage} />
            <footer className="pane-footer"><span>Input</span><span>{source.split('\n').length} lines</span></footer>
          </section>

          <div className="swap-rail" aria-hidden="true"><span>→</span></div>

          <section className="pane output-pane">
            <header className="pane-header">
              <div className="language-control">
                <label htmlFor="target-language">To</label>
                <select id="target-language" value={targetLanguage} onChange={(event) => setTargetLanguage(event.target.value)}>
                  {Object.entries(LANGUAGE_META).map(([id, meta]) => <option value={id} key={id}>{meta.name}</option>)}
                </select>
              </div>
              <button className="copy-button" onClick={copyCode} disabled={!output}>{copied ? 'Copied ✓' : 'Copy code'}</button>
            </header>
            {output ? <Editor value={output} onChange={() => {}} language={targetLanguage} readOnly /> : <div className="empty-output"><span className="empty-icon">↗</span><p>Your translated code<br />will appear here.</p></div>}
            <footer className="pane-footer"><span>Output</span><span>{output ? `${output.split('\n').length} lines` : 'Ready when you are'}</span></footer>
          </section>
        </div>
        <div className="action-row">
          <span className="pipeline-note"><span className="spark">✦</span> AST → IR → emitter</span>
          <button className="convert-button" onClick={convert}>Convert code <span>Rust / Wasm</span></button>
        </div>
      </section>

      <section className="feature-strip">
        <div><span className="feature-number">01</span><strong>Parse locally</strong><p>A Rust/Wasm parser turns your source into a structured, language-neutral tree.</p></div>
        <div><span className="feature-number">02</span><strong>Normalize once</strong><p>A small intermediate representation keeps translations predictable.</p></div>
        <div><span className="feature-number">03</span><strong>Emit cleanly</strong><p>Language-specific emitters produce readable, idiomatic starter code.</p></div>
      </section>

      <footer className="site-footer"><span>TranslateCode / 2026</span><span>Private by design · Built for the curious</span></footer>
    </main>
  );
}

createRoot(document.getElementById('root')).render(<App />);

warmEngine().catch(() => {});
