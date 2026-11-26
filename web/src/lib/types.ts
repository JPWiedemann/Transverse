// The JSON contract of the wasm module (serde output of transverse-lines).

export type ParamKind =
  | { Length: { default_mm: number; unit: string; lambda: boolean } }
  | { Frequency: { default_hz: number; unit: string } }
  | { Number: { default: number; unit: string; scale: number } }
  | { Bool: boolean }
  | { Choice: [string[], number] };

export interface ParamSpec {
  key: string;
  label: string;
  group: string;
  kind: ParamKind;
  enable_when: { key: string; value: number } | null;
}

export interface SynthTarget {
  id: string;
  label: string;
}

export interface LineInfo {
  id: string;
  name: string;
  synth_targets: SynthTarget[];
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
  signal: [number, number] | null;
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

export interface Summary {
  modes: ModeResult[];
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
  | { Line: { x0: number; y0: number; x1: number; y1: number; color: SchemColor; dashed: boolean; arrows: boolean } }
  | { Text: { x: number; y: number; text: string; anchor: 'Center' | 'Left' | 'Right'; bold: boolean; color: SchemColor } };

export interface Schematic {
  width: number;
  height: number;
  items: SchemItem[];
}
