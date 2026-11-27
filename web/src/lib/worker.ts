// Runs the wasm module off the page thread, so a long solve never blocks the UI.
/// <reference lib="webworker" />
import init, { analyze, synthesize, type AnalysisHandle } from '../../pkg/transverse_wasm.js';
import type { QueryName, RasterResult, Request, Response } from './protocol';

const ready = init();
let last: AnalysisHandle | undefined;

/** Call a method of the result handle; typed arrays in the answer are transferred, not copied. */
function query(h: AnalysisHandle, name: QueryName, args: unknown[]): [unknown, Transferable[]] {
  if (name === 'field_raster') {
    const [mode, view, scale, imperial, db, width] = args as [string, Float64Array, Float64Array, boolean, number, number];
    const r = h.field_raster(mode, view, scale, imperial, db, width);
    const rgba = r.rgba();
    const value: RasterResult = {
      width: r.width(),
      height: r.height(),
      rgba: new Uint8ClampedArray(rgba.buffer as ArrayBuffer),
      legend: r.legend(),
    };
    r.free();
    return [value, [rgba.buffer as ArrayBuffer]];
  }
  const value = (h[name] as (...a: unknown[]) => unknown).apply(h, args);
  return [value, ArrayBuffer.isView(value) ? [value.buffer as ArrayBuffer] : []];
}

function send(r: Response) {
  postMessage(r);
}

self.onmessage = async (e: MessageEvent<Request>) => {
  const req = e.data;
  try {
    await ready;
    if (req.op === 'query') {
      if (!last) throw new Error('no result to query');
      const [value, transfer] = query(last, req.name, req.args);
      postMessage({ id: req.id, type: 'result', value } satisfies Response, { transfer });
      return;
    }
    const progress = (text: string) => send({ id: req.id, type: 'progress', text });
    const handle =
      req.op === 'analyze'
        ? analyze(req.line, req.params, progress)
        : synthesize(req.line, req.params, req.target, req.z0, req.phaseDeg, progress);
    last?.free();
    last = handle;
    send({ id: req.id, type: 'done', summary: handle.summary(), params: handle.params() ?? null });
  } catch (err) {
    send({ id: req.id, type: 'error', message: err instanceof Error ? err.message : String(err) });
  }
};
