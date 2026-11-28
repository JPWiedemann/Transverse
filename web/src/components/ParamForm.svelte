<script lang="ts">
  import type { FormState } from '../lib/form';
  import { isVisible } from '../lib/form';
  import type { ParamSet, ParamSpec, Preset } from '../lib/types';
  import ParamField from './ParamField.svelte';

  interface Props {
    specs: ParamSpec[];
    form: FormState;
    controls: ParamSet;
    presets: Preset[];
    onPreset: (preset: Preset) => void;
    invalidKey: string | null;
    onText: (key: string, text: string) => void;
    onUnit: (key: string, unit: string) => void;
    onFlag: (key: string, value: boolean) => void;
    onChoice: (key: string, index: number) => void;
  }
  let { specs, form, controls, presets, onPreset, invalidKey, onText, onUnit, onFlag, onChoice }: Props = $props();

  // only what the structure uses, in groups in the order of their first parameter; empty groups are left out
  const groups = $derived.by(() => {
    const out: { name: string; specs: ParamSpec[] }[] = [];
    for (const s of specs.filter((s) => isVisible(s.visible_when, controls))) {
      let g = out.find((x) => x.name === s.group);
      if (!g) out.push((g = { name: s.group, specs: [] }));
      g.specs.push(s);
    }
    return out;
  });
</script>

{#each groups as g, index (g.name)}
  <fieldset>
    <legend>{g.name}</legend>
    <!-- the presets head the first group, which holds the switches they set -->
    {#if index === 0 && presets.length > 0}
      <div class="presets" role="group" aria-label="Presets">
        {#each presets as p (p.id)}
          <button type="button" onclick={() => onPreset(p)}>{p.label}</button>
        {/each}
      </div>
    {/if}
    <div class="grid">
      {#each g.specs as spec (spec.key)}
        {@const st = form[spec.key]}
        {#if st}
          <ParamField
            {spec}
            state={st}
            invalid={invalidKey === spec.key}
            onText={(t) => onText(spec.key, t)}
            onUnit={(u) => onUnit(spec.key, u)}
            onFlag={(v) => onFlag(spec.key, v)}
            onChoice={(i) => onChoice(spec.key, i)}
          />
        {/if}
      {/each}
    </div>
  </fieldset>
{/each}

<style>
  fieldset {
    border: 1px solid var(--border);
    border-radius: 8px;
    margin: 0 0 12px;
    padding: 8px 12px 12px;
    background: var(--panel);
  }
  legend {
    padding: 0 6px;
    color: var(--muted);
    font-weight: 600;
  }
  .presets {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin: 2px 0 10px;
  }
  .grid {
    display: grid;
    grid-template-columns: minmax(8rem, 1fr) minmax(5rem, 8rem) 5.5rem;
    gap: 6px 8px;
    align-items: center;
  }
</style>
