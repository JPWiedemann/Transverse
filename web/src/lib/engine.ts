// Page side of the worker: one solve at a time, progress texts, cancel by restarting the worker.
// Calls on the last result (loss samples, field images, ...) can be made at any time; they queue behind a running solve.

import type { QueryName, Request, Response } from './protocol';
import type { AnalysisResult, ParamSet, Summary } from './types';

export class Cancelled extends Error {
  constructor() {
    super('cancelled by user');
  }
}

interface Job {
  id: number;
  resolve: (r: AnalysisResult) => void;
  reject: (e: Error) => void;
  onProgress: (text: string) => void;
}

/** Column order of the rows returned by `sample` (see transverse-wasm `SAMPLE_COLUMNS`). */
export const SAMPLE_COLUMNS = ['z0', 'beta', 'alpha_c', 'alpha_d', 'r', 'g', 'rs', 'krough', 'delta'] as const;

export class Engine {
  private worker: Worker;
  private job: Job | undefined;
  private queries = new Map<number, { resolve: (v: unknown) => void; reject: (e: Error) => void }>();
  private nextId = 1;
  private hasResult = false;

  constructor() {
    this.worker = this.spawn();
  }

  private spawn(): Worker {
    const w = new Worker(new URL('./worker.ts', import.meta.url), { type: 'module' });
    w.onmessage = (e: MessageEvent<Response>) => this.handle(e.data);
    w.onerror = (e) => {
      this.hasResult = false;
      this.fail(new Error(e.message || 'the solver worker failed'));
    };
    return w;
  }

  private handle(r: Response) {
    if (r.type === 'result' || (r.type === 'error' && this.queries.has(r.id))) {
      const q = this.queries.get(r.id);
      this.queries.delete(r.id);
      if (r.type === 'result') q?.resolve(r.value);
      else if (r.type === 'error') q?.reject(new Error(r.message));
      return;
    }
    const job = this.job;
    if (!job || job.id !== r.id) return;
    if (r.type === 'progress') job.onProgress(r.text);
    else if (r.type === 'error') this.fail(new Error(r.message));
    else if (r.type === 'done') {
      this.job = undefined;
      this.hasResult = true;
      job.resolve({
        summary: JSON.parse(r.summary) as Summary,
        params: r.params === null ? null : (JSON.parse(r.params) as ParamSet),
      });
    }
  }

  private fail(e: Error) {
    const job = this.job;
    this.job = undefined;
    job?.reject(e);
  }

  get busy(): boolean {
    return this.job !== undefined;
  }

  /** Whether the worker still holds a result to sample (a cancel throws it away). */
  get canSample(): boolean {
    return this.hasResult;
  }

  private run(make: (id: number) => Request, onProgress: (text: string) => void): Promise<AnalysisResult> {
    if (this.job) return Promise.reject(new Error('a calculation is already running'));
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      this.job = { id, resolve, reject, onProgress };
      this.worker.postMessage(make(id));
    });
  }

  analyze(line: string, params: ParamSet, onProgress: (text: string) => void) {
    return this.run((id) => ({ id, op: 'analyze', line, params: JSON.stringify(params) }), onProgress);
  }

  synthesize(line: string, params: ParamSet, target: string, value: number, onProgress: (text: string) => void) {
    return this.run(
      (id) => ({ id, op: 'synthesize', line, params: JSON.stringify(params), target, value }),
      onProgress,
    );
  }

  /** Call a method of the last result (see `QueryName`). Queries queue behind a running solve. */
  query<T>(name: QueryName, ...args: unknown[]): Promise<T> {
    if (!this.hasResult) return Promise.reject(new Error('no result to query'));
    const id = this.nextId++;
    return new Promise<T>((resolve, reject) => {
      this.queries.set(id, { resolve: resolve as (v: unknown) => void, reject });
      this.worker.postMessage({ id, op: 'query', name, args } satisfies Request);
    });
  }

  /** Loss values of the last result: `SAMPLE_COLUMNS.length` numbers per frequency [Hz]. */
  sample(freqs: number[]): Promise<Float64Array> {
    return this.query<Float64Array>('sample', Float64Array.from(freqs));
  }

  /** Stop the running job: the worker is terminated and replaced (no shared memory needed).
   * The result held by the old worker is lost with it. */
  cancel() {
    if (!this.job) return;
    this.worker.terminate();
    this.worker = this.spawn();
    this.hasResult = false;
    for (const q of this.queries.values()) q.reject(new Cancelled());
    this.queries.clear();
    this.fail(new Cancelled());
  }
}
