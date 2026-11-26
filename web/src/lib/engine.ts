// Page side of the worker: one job at a time, progress texts, cancel by restarting the worker.

import type { Request, Response } from './protocol';
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

export class Engine {
  private worker: Worker;
  private job: Job | undefined;
  private nextId = 1;

  constructor() {
    this.worker = this.spawn();
  }

  private spawn(): Worker {
    const w = new Worker(new URL('./worker.ts', import.meta.url), { type: 'module' });
    w.onmessage = (e: MessageEvent<Response>) => this.handle(e.data);
    w.onerror = (e) => this.fail(new Error(e.message || 'the solver worker failed'));
    return w;
  }

  private handle(r: Response) {
    const job = this.job;
    if (!job || job.id !== r.id) return;
    if (r.type === 'progress') job.onProgress(r.text);
    else if (r.type === 'error') this.fail(new Error(r.message));
    else {
      this.job = undefined;
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

  synthesize(line: string, params: ParamSet, target: string, z0: number, onProgress: (text: string) => void) {
    return this.run(
      (id) => ({ id, op: 'synthesize', line, params: JSON.stringify(params), target, z0 }),
      onProgress,
    );
  }

  /** Stop the running job: the worker is terminated and replaced (no shared memory needed). */
  cancel() {
    if (!this.job) return;
    this.worker.terminate();
    this.worker = this.spawn();
    this.fail(new Cancelled());
  }
}
