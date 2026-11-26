// Runs the wasm module off the page thread, so a long solve never blocks the UI.
/// <reference lib="webworker" />
import init, { analyze, synthesize, type AnalysisHandle } from '../../pkg/transverse_wasm.js';
import type { Request, Response } from './protocol';

const ready = init();
let last: AnalysisHandle | undefined;

function send(r: Response) {
  postMessage(r);
}

self.onmessage = async (e: MessageEvent<Request>) => {
  const req = e.data;
  try {
    await ready;
    if (req.op === 'sample') {
      if (!last) throw new Error('no result to sample');
      const data = last.sample(req.freqs);
      postMessage({ id: req.id, type: 'sample', data } satisfies Response, { transfer: [data.buffer] });
      return;
    }
    const progress = (text: string) => send({ id: req.id, type: 'progress', text });
    const handle =
      req.op === 'analyze'
        ? analyze(req.line, req.params, progress)
        : synthesize(req.line, req.params, req.target, req.z0, progress);
    last?.free();
    last = handle;
    send({ id: req.id, type: 'done', summary: handle.summary(), params: handle.params() ?? null });
  } catch (err) {
    send({ id: req.id, type: 'error', message: err instanceof Error ? err.message : String(err) });
  }
};
