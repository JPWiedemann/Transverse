<script lang="ts">
  import type { FormState } from '../lib/form';
  import { isEnabled } from '../lib/form';
  import type { ParamSet, ParamSpec } from '../lib/types';
  import ParamField from './ParamField.svelte';

  interface Props {
    specs: ParamSpec[];
    form: FormState;
    controls: ParamSet;
    invalidKey: string | null;
    onText: (key: string, text: string) => void;
    onUnit: (key: string, unit: string) => void;
    onFlag: (key: string, value: boolean) => void;
    onChoice: (key: string, index: number) => void;
  }
  let { specs, form, controls, invalidKey, onText, onUnit, onFlag, onChoice }: Props = $props();

  // groups in the order of their first parameter
  const groups = $derived.by(() => {
    const out: { name: string; specs: ParamSpec[] }[] = [];
    for (const s of specs) {
      let g = out.find((x) => x.name === s.group);
      if (!g) out.push((g = { name: s.group, specs: [] }));
      g.specs.push(s);
    }
    return out;
  });
</script>

{#each groups as g (g.name)}
  <fieldset>
    <legend>{g.name}</legend>
    <div class="grid">
      {#each g.specs as spec (spec.key)}
        {@const st = form[spec.key]}
        {#if st}
          <ParamField
            {spec}
            state={st}
            enabled={isEnabled(spec, controls)}
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
  .grid {
    display: grid;
    grid-template-columns: minmax(8rem, 1fr) minmax(5rem, 8rem) 5.5rem;
    gap: 6px 8px;
    align-items: center;
  }
</style>
