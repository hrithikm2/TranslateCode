const LANGUAGE_IDS = {
  javascript: 0,
  java: 1,
  dart: 2,
  swift: 3,
  python: 4,
  go: 5,
  rust: 6,
};

let modulePromise;

function createWasiImports(memoryRef) {
  const view = () => new DataView(memoryRef.current.buffer);
  return {
    fd_write(_fd, iovecs, count, writtenPointer) {
      let written = 0;
      for (let index = 0; index < count; index += 1) {
        written += view().getUint32(iovecs + index * 8 + 4, true);
      }
      view().setUint32(writtenPointer, written, true);
      return 0;
    },
    environ_sizes_get(countPointer, sizePointer) {
      view().setUint32(countPointer, 0, true);
      view().setUint32(sizePointer, 0, true);
      return 0;
    },
    environ_get() { return 0; },
    fd_close() { return 8; },
    fd_prestat_get() { return 8; },
    fd_prestat_dir_name() { return 8; },
    fd_seek() { return 8; },
    proc_exit(code) { throw new Error(`Wasm engine exited with status ${code}`); },
  };
}

async function loadEngine() {
  if (!modulePromise) {
    modulePromise = fetch(`${import.meta.env.BASE_URL}engine.wasm`)
      .then(async (response) => {
        if (!response.ok) throw new Error(`Unable to load engine.wasm (${response.status})`);
        const bytes = await response.arrayBuffer();
        const memoryRef = { current: null };
        const result = await WebAssembly.instantiate(bytes, {
          wasi_snapshot_preview1: createWasiImports(memoryRef),
        });
        memoryRef.current = result.instance.exports.memory;
        return result;
      })
      .then(({ instance }) => instance.exports);
  }
  return modulePromise;
}

export async function transpile(source, from, to) {
  const engine = await loadEngine();
  const bytes = new TextEncoder().encode(source);
  const pointer = engine.alloc(bytes.length);
  new Uint8Array(engine.memory.buffer, pointer, bytes.length).set(bytes);
  engine.transpile(pointer, bytes.length, LANGUAGE_IDS[from], LANGUAGE_IDS[to]);
  const output = new Uint8Array(engine.memory.buffer, engine.output_ptr(), engine.output_len());
  return new TextDecoder().decode(output);
}

export function warmEngine() {
  return loadEngine();
}
