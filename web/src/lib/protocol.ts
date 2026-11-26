// Messages between the page and the Web Worker that runs the solver.

export type Request =
  | { id: number; op: 'analyze'; line: string; params: string }
  | { id: number; op: 'synthesize'; line: string; params: string; target: string; z0: number }
  // loss values of the last result at these frequencies [Hz]
  | { id: number; op: 'sample'; freqs: Float64Array };

export type Response =
  | { id: number; type: 'progress'; text: string }
  | { id: number; type: 'done'; summary: string; params: string | null }
  | { id: number; type: 'sample'; data: Float64Array }
  | { id: number; type: 'error'; message: string };
