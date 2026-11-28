// Messages between the page and the Web Worker that runs the solver.

/** Methods of the result handle the page may call (see transverse-wasm `AnalysisHandle`). */
export type QueryName =
  | 'sample'
  | 'field_info'
  | 'field_x'
  | 'field_y'
  | 'field_mesh_edges'
  | 'field_lines'
  | 'equipotentials'
  | 'field_probe'
  | 'field_raster';

export type Request =
  | { id: number; op: 'analyze'; line: string; params: string }
  | { id: number; op: 'synthesize'; line: string; params: string; target: string; z0: number; phaseDeg: number | undefined }
  // a call on the last result; typed arrays in `value` come back transferred
  | { id: number; op: 'query'; name: QueryName; args: unknown[] };

export type Response =
  | { id: number; type: 'progress'; text: string }
  | { id: number; type: 'done'; summary: string; params: string | null }
  | { id: number; type: 'result'; value: unknown }
  | { id: number; type: 'error'; message: string };

/** Result of the `field_raster` query. */
export interface RasterResult {
  width: number;
  height: number;
  /** RGBA, first row = top of the view */
  rgba: Uint8ClampedArray;
  /** JSON of the colour bar, `null` for the mesh view */
  legend: string;
}
