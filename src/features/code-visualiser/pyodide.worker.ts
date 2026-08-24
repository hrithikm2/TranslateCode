/// <reference lib="webworker" />

const PYODIDE_VERSION = '0.27.7';
const PYODIDE_BASE = `https://cdn.jsdelivr.net/pyodide/v${PYODIDE_VERSION}/full/`;
const PYODIDE_MODULE = `${PYODIDE_BASE}pyodide.mjs`;

let pyodide: any;
let loadingPromise: Promise<any> | null = null;

const TRACE_PROGRAM = String.raw`
import builtins
import json
import math
import sys
import time
import traceback

USER_FILENAME = "<visualizer-user-code>"

class _VisualizerLimit(Exception):
    pass

def _safe_repr(value):
    try:
        text = repr(value)
        return text if len(text) <= 240 else text[:237] + "..."
    except Exception:
        return f"<{type(value).__name__}>"

def _snapshot(value, depth=0, seen=None):
    if seen is None:
        seen = set()
    if value is None or isinstance(value, (str, bool, int)):
        return value
    if isinstance(value, float):
        return value if math.isfinite(value) else _safe_repr(value)
    if depth >= 7:
        return {"$type": "truncated", "value": _safe_repr(value)}

    identity = id(value)
    if isinstance(value, (list, tuple, dict, set)):
        if identity in seen:
            return {"$type": "unsupported", "value": "<circular reference>"}
        seen = seen | {identity}

    try:
        if isinstance(value, list):
            items = [_snapshot(item, depth + 1, seen) for item in value[:250]]
            if len(value) > 250:
                items.append({"$type": "truncated", "value": f"… {len(value) - 250} more items"})
            return items
        if isinstance(value, tuple):
            return {"$type": "tuple", "items": [_snapshot(item, depth + 1, seen) for item in value[:250]]}
        if isinstance(value, dict):
            entries = []
            for index, (key, item) in enumerate(value.items()):
                if index >= 250:
                    entries.append(["…", f"{len(value) - 250} more items"])
                    break
                entries.append([_snapshot(key, depth + 1, seen), _snapshot(item, depth + 1, seen)])
            return {"$type": "dict", "entries": entries}
        if isinstance(value, set):
            return {"$type": "set", "items": [_snapshot(item, depth + 1, seen) for item in list(value)[:250]]}
    except Exception:
        return {"$type": "unsupported", "value": _safe_repr(value)}

    return {"$type": "unsupported", "value": _safe_repr(value)}

def _snapshot_locals(frame):
    result = {}
    for name, value in frame.f_locals.items():
        if name.startswith("__") or callable(value):
            continue
        try:
            result[str(name)] = _snapshot(value)
        except Exception:
            result[str(name)] = {"$type": "unsupported", "value": _safe_repr(value)}
    return result

def _stack(frame):
    frames = []
    current = frame
    while current is not None:
        if current.f_code.co_filename == USER_FILENAME:
            function_name = current.f_code.co_name
            arguments = {}
            count = current.f_code.co_argcount + current.f_code.co_kwonlyargcount
            for name in current.f_code.co_varnames[:count]:
                if name in current.f_locals:
                    arguments[name] = _snapshot(current.f_locals[name])
            frames.append({
                "functionName": "module" if function_name == "<module>" else function_name,
                "line": current.f_lineno,
                "arguments": arguments,
            })
        current = current.f_back
    frames.reverse()
    return frames

def _error_payload(error_type, error_value, line=None):
    return {
        "type": getattr(error_type, "__name__", str(error_type)),
        "message": str(error_value),
        "line": line,
    }

_frames = []
_step_count = 0
_started_at = time.monotonic()

def _flush():
    global _frames
    if _frames:
        __visualizer_publish(json.dumps(_frames, ensure_ascii=False))
        _frames = []

def _trace(frame, event, arg):
    global _step_count
    if frame.f_code.co_filename != USER_FILENAME:
        return None
    # CPython reports the synthetic module call at line 0. It has no matching
    # source line, so omit it instead of corrupting the editor mapping.
    if event == "call" and frame.f_code.co_name == "<module>":
        return _trace
    if event not in ("line", "call", "return", "exception"):
        return _trace
    if _step_count >= __visualizer_max_frames:
        raise _VisualizerLimit("Execution stopped: maximum step limit reached.")
    if time.monotonic() - _started_at >= __visualizer_max_seconds:
        raise _VisualizerLimit("Execution stopped: maximum time limit reached.")

    error = None
    if event == "exception":
        error_type, error_value, _ = arg
        error = _error_payload(error_type, error_value, frame.f_lineno)

    record = {
        "line": frame.f_lineno,
        "event": event,
        "locals": _snapshot_locals(frame),
        "callStack": _stack(frame),
    }
    if error is not None:
        record["error"] = error
    _frames.append(record)
    _step_count += 1
    if len(_frames) >= 80:
        _flush()
    return _trace

_result = {"ok": True, "error": None, "steps": 0}
_namespace = {"__name__": "__main__", "__builtins__": builtins}

try:
    _compiled = compile(__visualizer_source, USER_FILENAME, "exec")
    sys.settrace(_trace)
    exec(_compiled, _namespace, _namespace)
except _VisualizerLimit as error:
    _result = {
        "ok": False,
        "stopped": True,
        "error": {"type": "ExecutionLimit", "message": str(error), "line": None},
        "steps": _step_count,
    }
except BaseException as error:
    line = getattr(error, "lineno", None)
    if line is None:
        extracted = traceback.extract_tb(error.__traceback__)
        user_lines = [entry.lineno for entry in extracted if entry.filename == USER_FILENAME]
        line = user_lines[-1] if user_lines else None
    _result = {
        "ok": False,
        "stopped": False,
        "error": _error_payload(type(error), error, line),
        "steps": _step_count,
    }
finally:
    sys.settrace(None)
    _flush()

json.dumps(_result, ensure_ascii=False)
`;

async function loadRuntime() {
  if (pyodide) return pyodide;
  if (!loadingPromise) {
    loadingPromise = (async () => {
      const module = await import(/* @vite-ignore */ PYODIDE_MODULE);
      pyodide = await module.loadPyodide({ indexURL: PYODIDE_BASE });
      return pyodide;
    })();
  }
  return loadingPromise;
}

async function runCode(message: { runId: number; source: string; maxFrames: number; maxSeconds: number }) {
  const runtime = await loadRuntime();
  const publish = (json: string) => {
    self.postMessage({ type: 'frames', runId: message.runId, frames: JSON.parse(json) });
  };

  runtime.globals.set('__visualizer_source', message.source);
  runtime.globals.set('__visualizer_max_frames', message.maxFrames);
  runtime.globals.set('__visualizer_max_seconds', message.maxSeconds);
  runtime.globals.set('__visualizer_publish', publish);

  try {
    const result = await runtime.runPythonAsync(TRACE_PROGRAM);
    self.postMessage({ type: 'result', runId: message.runId, result: JSON.parse(result) });
  } finally {
    runtime.globals.delete('__visualizer_source');
    runtime.globals.delete('__visualizer_max_frames');
    runtime.globals.delete('__visualizer_max_seconds');
    runtime.globals.delete('__visualizer_publish');
  }
}

self.addEventListener('message', async (event: MessageEvent) => {
  const message = event.data;
  try {
    if (message.type === 'initialize') {
      await loadRuntime();
      self.postMessage({ type: 'ready' });
    } else if (message.type === 'run') {
      await runCode(message);
    }
  } catch (error) {
    self.postMessage({
      type: 'worker-error',
      runId: message.runId,
      error: {
        type: error instanceof Error ? error.name : 'RuntimeError',
        message: error instanceof Error ? error.message : String(error),
      },
    });
  }
});

export {};
