import React from 'react';
import { createRoot } from 'react-dom/client';
import './styles.css';

function TranslatePreview() {
  return (
    <div className="tool-preview translate-preview" aria-hidden="true">
      <div className="preview-toolbar"><span>Python</span><i>→</i><span>JavaScript</span></div>
      <div className="translation-lines">
        <div><code><b>def</b> greet(name):</code><code><b>function</b> greet(name) {'{'}</code></div>
        <div><code>&nbsp;&nbsp;print(<em>"Hi "</em> + name)</code><code>&nbsp;&nbsp;console.log(<em>"Hi "</em> + name);</code></div>
        <div><code>&nbsp;</code><code>{'}'}</code></div>
      </div>
    </div>
  );
}

function VisualisePreview() {
  return (
    <div className="tool-preview visualise-preview" aria-hidden="true">
      <div className="preview-toolbar"><span>nums</span><small>line 8</small></div>
      <div className="array-preview">
        {[1, 3, 5, 7, 9].map((value, index) => (
          <div className={index === 3 ? 'is-active' : ''} key={value}>
            <small>{index}</small><strong>{value}</strong>{index === 3 ? <i>↑<b>mid</b></i> : null}
          </div>
        ))}
      </div>
    </div>
  );
}

function ToolCard({
  href,
  number,
  eyebrow,
  title,
  description,
  tags,
  className,
  children,
}: {
  href: string;
  number: string;
  eyebrow: string;
  title: string;
  description: string;
  tags: string[];
  className: string;
  children: React.ReactNode;
}) {
  return (
    <a className={`tool-card ${className}`} href={href}>
      <span className="card-number">{number}</span>
      <div className="card-copy">
        <span className="card-eyebrow">{eyebrow}</span>
        <h2>{title}</h2>
        <p>{description}</p>
        <div className="tag-row">{tags.map((tag) => <span key={tag}>{tag}</span>)}</div>
      </div>
      {children}
      <div className="card-action"><span>Open workspace</span><i aria-hidden="true">↗</i></div>
    </a>
  );
}

function Home() {
  return (
    <main className="home-shell">
      <div className="home-mesh" aria-hidden="true"><i /><i /><i /></div>
      <nav className="home-nav" aria-label="Primary navigation">
        <a className="home-brand" href="/" aria-label="TranslateCode home">
          <span><img src="/translatecode-mark.svg" alt="" /></span>
          <strong>Translate<em>Code</em></strong>
        </a>
        <div className="local-status"><i aria-hidden="true" /> Runs locally · Free forever</div>
      </nav>

      <section className="home-intro" aria-labelledby="chooser-title">
        <p><span>Two focused tools</span> · one private workspace</p>
        <h1 id="chooser-title">What would you like to do?</h1>
        <small>Choose a workspace to get started. No sign-in, setup, or uploads required.</small>
      </section>

      <section className="tool-grid" aria-label="Choose a workspace">
        <ToolCard
          href="/translate"
          number="01"
          eyebrow="Universal transpiler"
          title="Translate Code"
          description="Convert algorithms and focused code between seven languages through a shared, typed intermediate representation."
          tags={['7 languages', 'Local Wasm', 'Private']}
          className="translate-card"
        >
          <TranslatePreview />
        </ToolCard>

        <ToolCard
          href="/visualise"
          number="02"
          eyebrow="Execution tracer"
          title="Visualise Code"
          description="Run Python in your browser, then move through every line while arrays, pointers, variables, and calls update."
          tags={['Python', 'Step-by-step', 'No backend']}
          className="visualise-card"
        >
          <VisualisePreview />
        </ToolCard>
      </section>

      <footer className="home-footer">
        <span>Source code stays on your device.</span>
        <span>Built for learning, debugging, and focused translation.</span>
      </footer>
    </main>
  );
}

createRoot(document.getElementById('root')!).render(
  <React.StrictMode><Home /></React.StrictMode>,
);

export { Home };
