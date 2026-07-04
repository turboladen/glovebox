<script lang="ts">
  // Records tab (unit F): reference material — sub-nav Parts / Documents.
  // (Research moved to Plan — it's future work, not a record of the past;
  // VehicleDetail redirects old records/research deep links.)
  import { push } from '@keenmate/svelte-spa-router'
  import PartsTab from './PartsTab.svelte'
  import DocumentsTab from './DocumentsTab.svelte'

  let { vehicleId, sub = 'parts' }: { vehicleId: number; sub?: string } = $props()

  const subTabs = [
    { id: 'parts', label: 'Parts' },
    { id: 'documents', label: 'Documents' },
  ]

  // Unknown :sub params fall back to Parts instead of a blank pane.
  let activeSub = $derived(subTabs.some((t) => t.id === sub) ? sub : 'parts')

  function openSub(id: string) {
    push(`/vehicles/${vehicleId}/records${id === 'parts' ? '' : `/${id}`}`)
  }
</script>

<div class="records-tab">
  <div class="sub-nav">
    {#each subTabs as t (t.id)}
      <button class="sub-btn" class:active={activeSub === t.id} onclick={() => openSub(t.id)}>
        {t.label}
      </button>
    {/each}
  </div>

  {#if activeSub === 'documents'}
    <DocumentsTab {vehicleId} />
  {:else}
    <PartsTab {vehicleId} />
  {/if}
</div>

<style>
  .sub-nav {
    display: flex;
    gap: 2px;
    padding: 2px;
    margin-bottom: var(--sp-4);
    background: var(--surface);
    border: 1px solid var(--border-subtle);
    border-radius: 999px;
    width: fit-content;
  }

  .sub-btn {
    padding: 0.2rem var(--sp-3);
    border: none;
    background: none;
    border-radius: 999px;
    font-family: var(--font-display);
    font-size: 0.88rem;
    font-weight: 600;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    cursor: pointer;
    color: var(--text-muted);
    transition:
      background var(--duration-fast) var(--ease-out),
      color var(--duration-fast) var(--ease-out);
  }

  .sub-btn:hover:not(.active) {
    color: var(--text);
  }

  .sub-btn.active {
    background: var(--primary);
    color: var(--primary-text);
  }
</style>
