<script lang="ts">
  import type { AnalysisResult } from '../lib/types';
  import { formatG } from '../lib/units';

  let { result }: { result: AnalysisResult } = $props();

  const mode = $derived(result.summary.modes[0]);
  const pair = $derived(result.summary.pair);
  const NP_TO_DB = 20 / Math.LN10;
  // per mm: alpha is in Np/m
  const lossDbPerMm = $derived(mode ? ((mode.at_f.alpha_c + mode.at_f.alpha_d) * NP_TO_DB) / 1000 : 0);
</script>

{#if mode}
  {#if pair}
    <p class="of-mode">Below the impedances: the odd (differential) mode.</p>
  {/if}
  <div class="cards">
    {#if pair}
      <div class="card"><span>Zdiff</span><strong>{pair.z_diff.toFixed(3)} Ω</strong></div>
      <div class="card"><span>Zcomm</span><strong>{pair.z_comm.toFixed(3)} Ω</strong></div>
      <div class="card"><span>Coupling</span><strong>{pair.coupling.toFixed(4)}</strong></div>
    {:else}
      <div class="card"><span>Z0</span><strong>{mode.z0.toFixed(3)} Ω</strong></div>
    {/if}
    <div class="card"><span>ε_eff</span><strong>{mode.eps_eff.toFixed(4)}</strong></div>
    <div class="card"><span>Loss at {(mode.f_hz / 1e9).toPrecision(4)} GHz</span><strong>{lossDbPerMm.toFixed(5)} dB/mm</strong></div>
    <div class="card"><span>Wavelength at {(mode.f_hz / 1e9).toPrecision(4)} GHz</span><strong>{formatG(mode.wavelength_mm)} mm</strong></div>
    <div class="card"><span>Electrical length</span><strong>{mode.phase_deg.toFixed(2)}°</strong></div>
  </div>
{/if}
<pre class="report">{result.summary.report}</pre>

<style>
  .cards {
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
    margin-bottom: 12px;
  }
  .card {
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 8px 14px;
    display: flex;
    flex-direction: column;
    min-width: 8rem;
  }
  .card span {
    color: var(--muted);
    font-size: 12px;
  }
  .card strong {
    font-size: 20px;
    font-variant-numeric: tabular-nums;
  }
  .of-mode {
    margin: 0 0 8px;
    color: var(--muted);
    font-size: 12px;
  }
  .report {
    margin: 0;
    font-family: var(--mono);
    font-size: 13px;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 12px;
    overflow-x: auto;
  }
</style>
