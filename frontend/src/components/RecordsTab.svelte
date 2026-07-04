<script lang="ts">
  // Records tab (unit F): reference material — sub-nav Parts / Documents /
  // Research, re-homing the existing tabs unchanged.
  import { push } from '@keenmate/svelte-spa-router'
  import PartsTab from './PartsTab.svelte'
  import DocumentsTab from './DocumentsTab.svelte'
  import ResearchTab from './ResearchTab.svelte'

  let { vehicleId, sub = 'parts' }: { vehicleId: number; sub?: string } = $props()

  const subTabs = [
    { id: 'parts', label: 'Parts' },
    { id: 'documents', label: 'Documents' },
    { id: 'research', label: 'Research' },
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
  {:else if activeSub === 'research'}
    <ResearchTab {vehicleId} />
  {:else}
    <PartsTab {vehicleId} />
  {/if}
</div>

<style>
  .sub-nav {
    display: flex;
    gap: var(--sp-1);
    margin-bottom: var(--sp-4);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    overflow: hidden;
    width: fit-content;
  }

  .sub-btn {
    padding: var(--sp-1) var(--sp-3);
    border: none;
    background: none;
    font-family: var(--font-display);
    font-size: 0.85rem;
    cursor: pointer;
    color: var(--text-muted);
    transition:
      background var(--duration-fast) var(--ease-out),
      color var(--duration-fast) var(--ease-out);
  }

  .sub-btn.active {
    background: var(--primary);
    color: var(--primary-text);
  }
</style>
