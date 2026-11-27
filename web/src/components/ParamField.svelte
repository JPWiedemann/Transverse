<script lang="ts">
  import type { FieldState } from '../lib/form';
  import { defaultUnit, unitOptions } from '../lib/form';
  import type { ParamSpec } from '../lib/types';

  interface Props {
    spec: ParamSpec;
    state: FieldState;
    invalid: boolean;
    onText: (text: string) => void;
    onUnit: (unit: string) => void;
    onFlag: (value: boolean) => void;
    onChoice: (index: number) => void;
  }
  let { spec, state, invalid, onText, onUnit, onFlag, onChoice }: Props = $props();

  const id = $derived(`p-${spec.key}`);
  const units = $derived(unitOptions(spec));
  const choices = $derived('Choice' in spec.kind ? spec.kind.Choice[0] : []);
</script>

{#if state.kind === 'flag'}
  <label class="flag">
    <input type="checkbox" checked={state.value} onchange={(e) => onFlag(e.currentTarget.checked)} />
    <span>{spec.label}</span>
  </label>
{:else}
  <label class="label" for={id}>{spec.label}</label>
  {#if state.kind === 'choice'}
    <select {id} value={state.index} onchange={(e) => onChoice(Number(e.currentTarget.value))}>
      {#each choices as c, i}
        <option value={i}>{c}</option>
      {/each}
    </select>
    <span></span>
  {:else}
    <input
      {id}
      type="text"
      inputmode="decimal"
      class:invalid
     
      value={state.text}
      oninput={(e) => onText(e.currentTarget.value)}
    />
    {#if units.length > 0}
      <select aria-label="{spec.label} unit" value={state.unit} onchange={(e) => onUnit(e.currentTarget.value)}>
        {#each units as u}
          <option value={u}>{u}</option>
        {/each}
      </select>
    {:else}
      <span class="unit">{state.unit || defaultUnit(spec)}</span>
    {/if}
  {/if}
{/if}

<style>
  .label {
    color: var(--text);
  }
  .flag {
    grid-column: 1 / -1;
    display: flex;
    gap: 6px;
    align-items: center;
  }
  .unit {
    color: var(--muted);
    padding: 0 6px;
  }
</style>
