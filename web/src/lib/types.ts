// The JSON contract of the wasm module (serde output of transverse-lines).

export type ParamKind =
  | { Length: { default_mm: number; unit: string; lambda: boolean } }
  | { Frequency: { default_hz: number; unit: string } }
  | { Number: { default: number; unit: string; scale: number } }
  | { Bool: boolean }
  | { Choice: [string[], number] };

/** Applies when all conditions of one of the alternatives hold (no alternatives: always). */
export type Visibility = { key: string; value: number; not?: boolean }[][];

export interface ParamSpec {
  key: string;
  label: string;
  group: string;
  kind: ParamKind;
  visible_when: Visibility;
}

export interface SynthTarget {
  id: string;
  label: string;
  /** the impedance the target value is for ("Z0", "Zdiff") */
  quantity: string;
  /** a second impedance that is a target at the same time ("Zcomm"), for a search in two dimensions */
  second: string | null;
  visible_when: Visibility;
}

/** A button that sets some parameters at once. */
export interface Preset {
  id: string;
  label: string;
  values: [string, number | boolean][];
}

export interface LineInfo {
  id: string;
  name: string;
  synth_targets: SynthTarget[];
  presets: Preset[];
}

/** Parameters in internal units (lengths in mm, frequencies in Hz); choices by index. */
export type ParamSet = Record<string, number | boolean>;

export interface LossPoint {
  z0: number;
  beta: number;
  alpha_c: number;
  alpha_d: number;
  r: number;
  g: number;
  rs: number;
  krough: number;
  delta: number;
}

export interface SplitRow {
  key: string;
  label: string;
  indent: number;
  field_pct: number;
  current_pct: number;
}

export interface CurrentProfile {
  key: string;
  label: string;
  share_pct: number;
  /** mm */
  x: number[];
  /** J_s / I [1/mm] */
  j: number[];
  /** index into the colour cycle */
  color: number;
  dashed: boolean;
  /** draw the left and right halves as separate lines */
  break_at_zero: boolean;
}

export interface ProfilePlot {
  title: string;
  half_width: number;
  /** shaded x ranges: the signal traces */
  signals: [number, number][];
  markers: number[];
  labels: [number, string][];
}

export interface SplitSummary {
  rows: SplitRow[];
  x90: [string, number][];
  profiles: CurrentProfile[];
  plot: ProfilePlot;
  info: string;
}

export interface ModeResult {
  name: string;
  z0: number;
  eps_eff: number;
  l: number;
  c: number;
  f_hz: number;
  /** guided wavelength at f [mm] */
  wavelength_mm: number;
  /** electrical length of the line for the entered length [deg] */
  phase_deg: number;
  at_f: LossPoint;
  split: SplitSummary | null;
}

export interface Accuracy {
  nodes: number;
  nx: number;
  ny: number;
  dmin_actual: number;
  est_err: number | null;
}

/** What belongs to a differential pair as a whole. */
export interface PairResult {
  z_diff: number;
  z_comm: number;
  coupling: number;
}

export interface Summary {
  /** one mode, or the odd and the even mode of a pair (values per trace) */
  modes: ModeResult[];
  pair: PairResult | null;
  report: string;
  accuracy: Accuracy | null;
  has_field: boolean;
}

export interface AnalysisResult {
  summary: Summary;
  /** Set after a synthesis: the parameters that were found. */
  params: ParamSet | null;
}

// Cross-section sketch (serde output of `Schematic`); y grows downwards.
export type SchemColor =
  | 'UpperDielectric'
  | 'LowerDielectric'
  | 'CustomGap'
  | 'Conductor'
  | 'ConductorEdge'
  | 'Signal'
  | 'Text'
  | 'Muted';

export type SchemItem =
  | { Rect: { x0: number; y0: number; x1: number; y1: number; fill: SchemColor; outline: SchemColor | null } }
  | { Polygon: { points: [number, number][]; fill: SchemColor } }
  | { Line: { x0: number; y0: number; x1: number; y1: number; color: SchemColor; dashed: boolean; arrows: boolean } }
  | { Text: { x: number; y: number; text: string; anchor: 'Center' | 'Left' | 'Right'; bold: boolean; color: SchemColor } };

export interface Schematic {
  width: number;
  height: number;
  items: SchemItem[];
}

// Field views (serde output of the wasm layer).
export interface DrawRect {
  kind: 'Signal' | 'Ground';
  /** etched copper: [x0, x1] of the face at y1 (x0 and x1 are then those at y0) */
  top: [number, number] | null;
  x0: number;
  y0: number;
  x1: number;
  y1: number;
}

/** One way of driving the signals that the field views can show. */
export interface Excitation {
  key: string;
  label: string;
  drive: string;
  voltages: number[];
}

export interface FieldInfo {
  nx: number;
  ny: number;
  /** solution on a triangle mesh: `field_mesh_edges` gives the mesh, nx and ny are only the extent */
  mesh: boolean;
  /** how the signals are driven, with the impedance of that mode */
  drive: string;
  /** the excitations to choose from (empty for one signal) and the shown one */
  excitations: Excitation[];
  excitation: number;
  equipotential_levels: number[];
  conductors: DrawRect[];
  dashed_y: number[];
  dotted_y: number[];
  /** [x0, x1, y0, y1] in mm */
  view_zoom: [number, number, number, number];
  view_full: [number, number, number, number];
}

export interface Legend {
  label: string;
  vmin: number;
  vmax: number;
  cmap: 'Inferno' | 'Viridis' | 'Greys';
  /** base unit of a linear field scale ("V/m", "V/mil"), `null` for the other scales */
  field_base: string | null;
}

export interface Probe {
  v: number;
  /** |E| in V/m, `null` inside a conductor */
  e: number | null;
  er: number;
  in_conductor: boolean;
}
