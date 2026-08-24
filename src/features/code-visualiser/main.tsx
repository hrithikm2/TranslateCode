import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { createRoot } from 'react-dom/client';
import CodeMirror from '@uiw/react-codemirror';
import { python } from '@codemirror/lang-python';
import { Decoration, EditorView } from '@codemirror/view';
import {
  arrayEntries,
  changedArrayIndexes,
  formatValue,
  isDictionary,
  isPrimitive,
  isTuple,
  pointersForArray,
} from './trace-utils';
import type { ExecutionError, ExecutionFrame, ExecutionStatus, RunnerStatus, SerializedValue } from './types';
import './styles.css';

const DEFAULT_CODE = `def binary_search(nums, target):
    left = 0
    right = len(nums) - 1

    while left <= right:
        mid = (left + right) // 2

        if nums[mid] == target:
            return mid

        if nums[mid] < target:
            left = mid + 1
        else:
            right = mid - 1

    return -1


nums = [1, 3, 5, 7, 9, 11, 13, 15]
target = 11

result = binary_search(nums, target)`;

const MAX_FRAMES = 10_000;
const MAX_SECONDS = 3;
const HARD_TIMEOUT_MS = 5_000;

type WorkerResult = {
  ok: boolean;
  stopped?: boolean;
  error?: ExecutionError;
};

function usePyodideRunner() {
  const workerRef = useRef<Worker | null>(null);
  const runIdRef = useRef(0);
  const watchdogRef = useRef<number | null>(null);
  const mountedRef = useRef(true);
  const [runtimeStatus, setRuntimeStatus] = useState<RunnerStatus>('loading');
  const [runtimeError, setRuntimeError] = useState<ExecutionError | null>(null);
  const [frames, setFrames] = useState<ExecutionFrame[]>([]);
  const [executionStatus, setExecutionStatus] = useState<ExecutionStatus>('idle');
  const [executionError, setExecutionError] = useState<ExecutionError | null>(null);

  const clearWatchdog = useCallback(() => {
    if (watchdogRef.current !== null) window.clearTimeout(watchdogRef.current);
    watchdogRef.current = null;
  }, []);

  const bootWorker = useCallback(() => {
    workerRef.current?.terminate();
    clearWatchdog();
    if (!mountedRef.current) return;

    setRuntimeStatus('loading');
    setRuntimeError(null);
    const worker = new Worker(new URL('./pyodide.worker.ts', import.meta.url), { type: 'module' });
    workerRef.current = worker;

    worker.addEventListener('message', (event) => {
      const message = event.data;
      if (worker !== workerRef.current) return;
      if (message.type === 'ready') {
        setRuntimeStatus('ready');
        return;
      }
      if (message.runId !== undefined && message.runId !== runIdRef.current) return;
      if (message.type === 'frames') {
        setFrames((current) => [...current, ...(message.frames as ExecutionFrame[])]);
        return;
      }
      if (message.type === 'result') {
        clearWatchdog();
        const result = message.result as WorkerResult;
        if (result.ok) {
          setExecutionStatus('complete');
          setExecutionError(null);
        } else {
          setExecutionStatus(result.stopped ? 'stopped' : 'error');
          setExecutionError(result.error ?? { type: 'PythonError', message: 'Execution failed.' });
        }
        return;
      }
      if (message.type === 'worker-error') {
        clearWatchdog();
        const error = message.error as ExecutionError;
        if (message.runId === undefined) {
          setRuntimeStatus('failed');
          setRuntimeError(error);
        } else {
          setExecutionStatus('error');
          setExecutionError(error);
        }
      }
    });

    worker.addEventListener('error', (event) => {
      if (worker !== workerRef.current) return;
      clearWatchdog();
      const error = { type: 'PythonRuntimeError', message: event.message || 'The Python runtime could not start.' };
      setRuntimeStatus('failed');
      setRuntimeError(error);
      setExecutionStatus((current) => current === 'running' ? 'error' : current);
      setExecutionError((current) => current ?? error);
    });

    worker.postMessage({ type: 'initialize' });
  }, [clearWatchdog]);

  useEffect(() => {
    mountedRef.current = true;
    let cancelled = false;
    const start = async () => {
      if ('serviceWorker' in navigator) {
        try {
          await navigator.serviceWorker.register('/pyodide-cache-sw.js');
          await navigator.serviceWorker.ready;
        } catch {
          // Pyodide can still use the browser's normal HTTP cache when service
          // workers are unavailable (for example, in a private browsing mode).
        }
      }
      if (!cancelled) bootWorker();
    };
    start();
    return () => {
      cancelled = true;
      mountedRef.current = false;
      clearWatchdog();
      workerRef.current?.terminate();
    };
  }, [bootWorker, clearWatchdog]);

  const run = useCallback((source: string) => {
    if (!workerRef.current || runtimeStatus !== 'ready') return;
    const runId = runIdRef.current + 1;
    runIdRef.current = runId;
    setFrames([]);
    setExecutionError(null);
    setExecutionStatus('running');
    workerRef.current.postMessage({
      type: 'run',
      runId,
      source,
      maxFrames: MAX_FRAMES,
      maxSeconds: MAX_SECONDS,
    });

    clearWatchdog();
    watchdogRef.current = window.setTimeout(() => {
      if (runId !== runIdRef.current) return;
      runIdRef.current += 1;
      setExecutionStatus('stopped');
      setExecutionError({
        type: 'ExecutionTimeout',
        message: 'Execution stopped: the program did not yield before the safety timeout.',
      });
      bootWorker();
    }, HARD_TIMEOUT_MS);
  }, [bootWorker, clearWatchdog, runtimeStatus]);

  const reset = useCallback(() => {
    const wasRunning = executionStatus === 'running';
    runIdRef.current += 1;
    clearWatchdog();
    setFrames([]);
    setExecutionStatus('idle');
    setExecutionError(null);
    if (wasRunning) bootWorker();
  }, [bootWorker, clearWatchdog, executionStatus]);

  return {
    runtimeStatus,
    runtimeError,
    frames,
    executionStatus,
    executionError,
    run,
    reset,
    retryRuntime: bootWorker,
  };
}

function CodeEditor({ code, activeLine, onChange }: { code: string; activeLine?: number; onChange: (value: string) => void }) {
  const executionLine = useMemo(() => EditorView.decorations.of((view) => {
    if (!activeLine || activeLine < 1 || activeLine > view.state.doc.lines) return Decoration.none;
    const line = view.state.doc.line(activeLine);
    return Decoration.set([Decoration.line({ class: 'cm-execution-line' }).range(line.from)]);
  }), [activeLine]);

  return (
    <CodeMirror
      value={code}
      height="100%"
      theme="dark"
      extensions={[python(), executionLine]}
      onChange={onChange}
      aria-label="Python code editor"
      basicSetup={{
        lineNumbers: true,
        foldGutter: false,
        highlightActiveLine: false,
        highlightActiveLineGutter: false,
        autocompletion: false,
      }}
    />
  );
}

function ArrayVisualizer({
  name,
  values,
  frame,
  previousFrame,
}: {
  name: string;
  values: SerializedValue[];
  frame?: ExecutionFrame;
  previousFrame?: ExecutionFrame;
}) {
  const pointers = pointersForArray(frame, values.length);
  const changed = changedArrayIndexes(name, values, previousFrame);

  return (
    <section className="array-visual" aria-label={`${name} list visualization`}>
      <div className="array-name"><code>{name}</code><span>{values.length} items</span></div>
      <div className="array-scroll" tabIndex={0}>
        <div className="array-row">
          {values.map((value, index) => (
            <div className="array-column" key={`${name}-${index}`}>
              <span className="array-index">{index}</span>
              <div className={`array-cell ${changed.has(index) ? 'is-changed' : ''}`} title={formatValue(value)}>
                {formatValue(value, true)}
              </div>
              <div className="pointer-space">
                {(pointers.get(index) ?? []).map((pointer) => (
                  <span className="pointer" key={pointer}><i aria-hidden="true">↑</i>{pointer}</span>
                ))}
              </div>
            </div>
          ))}
        </div>
      </div>
    </section>
  );
}

function EmptyPanel({ children }: { children: React.ReactNode }) {
  return <div className="empty-panel"><span aria-hidden="true">◇</span><p>{children}</p></div>;
}

function VariablesPanel({ frame }: { frame?: ExecutionFrame }) {
  const variables = frame
    ? Object.entries(frame.locals).filter(([, value]) => isPrimitive(value) || isTuple(value))
    : [];
  return (
    <section className="info-panel">
      <header><h2>Variables</h2><span>{variables.length}</span></header>
      {variables.length ? (
        <dl className="variable-list">
          {variables.map(([name, value]) => (
            <div key={name}><dt>{name}</dt><dd title={formatValue(value)}>{formatValue(value, true)}</dd></div>
          ))}
        </dl>
      ) : <EmptyPanel>Primitive locals will appear here.</EmptyPanel>}
    </section>
  );
}

function DictionariesPanel({ frame }: { frame?: ExecutionFrame }) {
  const dictionaries = frame
    ? Object.entries(frame.locals).filter((entry): entry is [string, SerializedValue & { $type: 'dict' }] => isDictionary(entry[1]))
    : [];
  if (!dictionaries.length) return null;
  return (
    <section className="info-panel dictionary-panel">
      <header><h2>Dictionaries</h2><span>{dictionaries.length}</span></header>
      <div className="dictionary-list">
        {dictionaries.map(([name, dictionary]) => {
          const entries = Array.isArray(dictionary.entries) ? dictionary.entries : [];
          return (
            <section key={name}>
              <h3>{name}</h3>
              {entries.length ? entries.map((entry, index) => {
                const pair = Array.isArray(entry) ? entry : [];
                return (
                  <div className="dictionary-entry" key={`${name}-${index}`}>
                    <code>{formatValue(pair[0] ?? null, true)}</code><span>→</span><code>{formatValue(pair[1] ?? null, true)}</code>
                  </div>
                );
              }) : <small>Empty dictionary</small>}
            </section>
          );
        })}
      </div>
    </section>
  );
}

function CallStackPanel({ frame }: { frame?: ExecutionFrame }) {
  const stack = frame?.callStack ?? [];
  return (
    <section className="info-panel call-stack-panel">
      <header><h2>Call stack</h2><span>depth {stack.length}</span></header>
      {stack.length ? (
        <ol className="call-stack">
          {stack.map((call, index) => {
            const args = Object.entries(call.arguments ?? {});
            return (
              <li className={index === stack.length - 1 ? 'is-active' : ''} key={`${call.functionName}-${index}`}>
                <i>{index + 1}</i>
                <div>
                  <strong>{call.functionName}</strong>
                  <code>({args.map(([name, value]) => `${name}=${formatValue(value, true)}`).join(', ')})</code>
                </div>
                {call.line ? <span>line {call.line}</span> : null}
              </li>
            );
          })}
        </ol>
      ) : <EmptyPanel>Function calls will appear during execution.</EmptyPanel>}
    </section>
  );
}

function PlaybackControls({
  frameIndex,
  frameCount,
  currentLine,
  isPlaying,
  speed,
  executionStatus,
  onFrameChange,
  onPlayToggle,
  onSpeedChange,
}: {
  frameIndex: number;
  frameCount: number;
  currentLine?: number;
  isPlaying: boolean;
  speed: number;
  executionStatus: ExecutionStatus;
  onFrameChange: (index: number) => void;
  onPlayToggle: () => void;
  onSpeedChange: (speed: number) => void;
}) {
  const hasFrames = frameCount > 0;
  const atStart = !hasFrames || frameIndex === 0;
  const atEnd = !hasFrames || frameIndex === frameCount - 1;
  const statusText = executionStatus === 'running'
    ? 'Tracing execution…'
    : executionStatus === 'complete'
      ? 'Execution complete'
      : executionStatus === 'error'
        ? 'Execution stopped with error'
        : executionStatus === 'stopped'
          ? 'Execution stopped by safety limit'
          : 'Run code to create a trace';

  return (
    <footer className="playback-bar">
      <div className="execution-readout" aria-live="polite">
        <strong>{statusText}</strong>
        <span>{hasFrames ? `Step ${frameIndex + 1} / ${frameCount}` : 'Step —'}</span>
        <span>{currentLine ? `Line ${currentLine}` : 'Line —'}</span>
      </div>
      <div className="timeline">
        <input
          type="range"
          min="0"
          max={Math.max(0, frameCount - 1)}
          value={hasFrames ? frameIndex : 0}
          disabled={!hasFrames}
          aria-label="Execution timeline"
          onChange={(event) => onFrameChange(Number(event.target.value))}
          style={{ '--timeline-progress': hasFrames && frameCount > 1 ? `${(frameIndex / (frameCount - 1)) * 100}%` : '0%' } as React.CSSProperties}
        />
      </div>
      <div className="playback-actions">
        <button type="button" onClick={() => onFrameChange(0)} disabled={atStart} title="Restart trace">↤<span>Restart</span></button>
        <button type="button" onClick={() => onFrameChange(frameIndex - 1)} disabled={atStart} title="Step backward">←</button>
        <button className="play-button" type="button" onClick={onPlayToggle} disabled={!hasFrames || (atEnd && !isPlaying)}>
          {isPlaying ? 'Ⅱ' : '▶'}<span>{isPlaying ? 'Pause' : 'Play'}</span>
        </button>
        <button type="button" onClick={() => onFrameChange(frameIndex + 1)} disabled={atEnd} title="Step forward">→</button>
        <label>
          <span className="sr-only">Playback speed</span>
          <select value={speed} onChange={(event) => onSpeedChange(Number(event.target.value))}>
            <option value={1200}>0.5×</option>
            <option value={600}>1×</option>
            <option value={300}>2×</option>
          </select>
        </label>
      </div>
    </footer>
  );
}

function App() {
  const [code, setCode] = useState(DEFAULT_CODE);
  const [currentFrameIndex, setCurrentFrameIndex] = useState(0);
  const [isPlaying, setIsPlaying] = useState(false);
  const [playbackSpeed, setPlaybackSpeed] = useState(600);
  const {
    runtimeStatus,
    runtimeError,
    frames,
    executionStatus,
    executionError,
    run,
    reset,
    retryRuntime,
  } = usePyodideRunner();

  const frame = frames[currentFrameIndex];
  const previousFrame = currentFrameIndex > 0 ? frames[currentFrameIndex - 1] : undefined;
  const lists = arrayEntries(frame);
  const activeLine = frame?.line ?? executionError?.line;

  useEffect(() => {
    if (frames.length && currentFrameIndex >= frames.length) setCurrentFrameIndex(frames.length - 1);
  }, [currentFrameIndex, frames.length]);

  useEffect(() => {
    if ((executionStatus === 'error' || executionStatus === 'stopped') && frames.length) {
      setCurrentFrameIndex(frames.length - 1);
    }
  }, [executionStatus, frames.length]);

  useEffect(() => {
    if (!isPlaying) return undefined;
    if (!frames.length || currentFrameIndex >= frames.length - 1) {
      setIsPlaying(false);
      return undefined;
    }
    const timer = window.setTimeout(() => setCurrentFrameIndex((index) => index + 1), playbackSpeed);
    return () => window.clearTimeout(timer);
  }, [currentFrameIndex, frames.length, isPlaying, playbackSpeed]);

  const changeFrame = (index: number) => {
    setIsPlaying(false);
    setCurrentFrameIndex(Math.max(0, Math.min(index, Math.max(0, frames.length - 1))));
  };

  const clearTrace = () => {
    setIsPlaying(false);
    setCurrentFrameIndex(0);
    reset();
  };

  const handleCodeChange = (value: string) => {
    setCode(value);
    if (frames.length || executionError) clearTrace();
  };

  const execute = () => {
    setIsPlaying(false);
    setCurrentFrameIndex(0);
    run(code);
  };

  return (
    <main className="visualizer-app">
      <header className="app-header">
        <a className="product-mark" href="/" aria-label="Back to workspace chooser">
          <span aria-hidden="true">TC</span>
          <div><strong>Code Visualizer</strong><small>Python execution trace</small></div>
        </a>
        <div className={`runtime-status is-${runtimeStatus}`} role="status">
          <i aria-hidden="true" />
          {runtimeStatus === 'loading' ? 'Loading Python runtime…' : runtimeStatus === 'ready' ? 'Python Ready' : 'Runtime unavailable'}
        </div>
      </header>

      <section className="workspace">
        <section className="code-pane">
          <header className="pane-header">
            <div><span>Python</span><small>Runs locally in your browser</small></div>
            <div className="editor-actions">
              <button type="button" onClick={() => { setCode(DEFAULT_CODE); clearTrace(); }}>Load example</button>
              <button type="button" onClick={clearTrace} disabled={executionStatus === 'idle' && !frames.length}>Reset</button>
              <button className="run-button" type="button" onClick={execute} disabled={runtimeStatus !== 'ready' || executionStatus === 'running' || !code.trim()}>
                {executionStatus === 'running' ? 'Running…' : '▶ Run'}
              </button>
            </div>
          </header>
          <div className="editor-wrap">
            <CodeEditor code={code} activeLine={activeLine} onChange={handleCodeChange} />
          </div>
          <div className="editor-footer">
            <span>{code.split('\n').length} lines</span>
            <span>Step limit {MAX_FRAMES.toLocaleString()}</span>
            <span>Time limit {MAX_SECONDS}s</span>
          </div>
        </section>

        <section className="visual-pane">
          <header className="pane-header visualization-header">
            <div><span>Execution state</span><small>{frame ? `${frame.event} event · line ${frame.line}` : 'Waiting for a trace'}</small></div>
            {frames.length ? <span className="frame-count">{frames.length} frames</span> : null}
          </header>
          <div className="visual-scroll">
            {(executionError || runtimeError) && (
              <section className="error-notice" role="alert">
                <span aria-hidden="true">!</span>
                <div>
                  <strong>{(executionError ?? runtimeError)?.type}</strong>
                  <p>{(executionError ?? runtimeError)?.message}</p>
                  {(executionError ?? runtimeError)?.line ? <small>Line {(executionError ?? runtimeError)?.line}</small> : null}
                  {runtimeStatus === 'failed' ? <button type="button" onClick={retryRuntime}>Retry Python runtime</button> : null}
                </div>
              </section>
            )}

            <section className="arrays-panel">
              <header className="section-title"><h2>Lists</h2><span>{lists.length}</span></header>
              {lists.length
                ? lists.map(([name, values]) => (
                    <ArrayVisualizer key={name} name={name} values={values} frame={frame} previousFrame={previousFrame} />
                  ))
                : <EmptyPanel>Run the code, then step through to inspect lists and pointers.</EmptyPanel>}
            </section>

            <div className="details-grid">
              <VariablesPanel frame={frame} />
              <CallStackPanel frame={frame} />
            </div>
            <DictionariesPanel frame={frame} />
          </div>
        </section>
      </section>

      <PlaybackControls
        frameIndex={currentFrameIndex}
        frameCount={frames.length}
        currentLine={activeLine}
        isPlaying={isPlaying}
        speed={playbackSpeed}
        executionStatus={executionStatus}
        onFrameChange={changeFrame}
        onPlayToggle={() => setIsPlaying((playing) => !playing)}
        onSpeedChange={setPlaybackSpeed}
      />
    </main>
  );
}

createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);

export { App, DEFAULT_CODE };
