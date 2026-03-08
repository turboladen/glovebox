<script lang="ts">
  import { onMount } from 'svelte'
  import { ai } from '../lib/api'
  import type { ProviderSummary } from '../lib/types'

  let {
    selectedProviderId = $bindable<number | undefined>(undefined),
  }: {
    selectedProviderId?: number | undefined
  } = $props()

  let providers: ProviderSummary[] = $state([])
  let loaded = $state(false)

  onMount(async () => {
    try {
      const status = await ai.status()
      providers = status.providers.filter(p => p.enabled)
      if (!selectedProviderId && status.default_provider_id) {
        selectedProviderId = status.default_provider_id
      }
    } catch (e) {
      console.error('Failed to load AI providers:', e)
    } finally {
      loaded = true
    }
  })

  function handleChange(e: Event) {
    const value = (e.target as HTMLSelectElement).value
    selectedProviderId = value ? Number(value) : undefined
  }
</script>

{#if loaded && providers.length > 1}
  <select class="provider-select" aria-label="AI Provider" value={selectedProviderId ?? ''} onchange={handleChange}>
    {#each providers as p (p.id)}
      <option value={p.id}>
        {p.name}{p.is_default ? ' (default)' : ''}
      </option>
    {/each}
  </select>
{/if}

<style>
  .provider-select {
    padding: 2px 8px;
    font-size: 0.75rem;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--surface);
    color: var(--text-secondary);
    min-width: 0;
    max-width: 180px;
  }
</style>
