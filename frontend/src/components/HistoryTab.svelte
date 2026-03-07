<script lang="ts">
  import { onMount } from 'svelte'
  import { services as servicesApi } from '../lib/api'
  import type { ServiceRecordWithLinks } from '../lib/types'

  let { vehicleId }: { vehicleId: number } = $props()

  let records: ServiceRecordWithLinks[] = $state([])
  let loading = $state(true)

  onMount(async () => {
    try {
      records = await servicesApi.list(vehicleId)
    } catch (e) {
      console.error(e)
    } finally {
      loading = false
    }
  })

  function formatCents(cents: number | null): string {
    if (cents == null) return ''
    return `$${(cents / 100).toFixed(2)}`
  }

  function formatMileage(n: number | null): string {
    return n != null ? n.toLocaleString() + ' mi' : ''
  }
</script>

{#if loading}
  <p>Loading service history...</p>
{:else if records.length === 0}
  <p class="empty">No service records yet.</p>
{:else}
  <div class="history-list">
    {#each records as record (record.id)}
      <div class="history-card">
        <div class="history-header">
          <span class="date">{record.service_date}</span>
          {#if record.total_cost_cents}
            <span class="cost">{formatCents(record.total_cost_cents)}</span>
          {/if}
        </div>
        {#if record.description}
          <p class="description">{record.description}</p>
        {/if}
        <div class="meta">
          {#if record.mileage}
            <span>{formatMileage(record.mileage)}</span>
          {/if}
          {#if record.shop_name}
            <span>at {record.shop_name}</span>
          {/if}
        </div>
        {#if record.notes}
          <p class="notes">{record.notes}</p>
        {/if}
      </div>
    {/each}
  </div>
{/if}

<style>
  .history-list {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }

  .history-card {
    padding: 0.75rem 1rem;
    border: 1px solid var(--border);
    border-radius: 4px;
  }

  .history-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }

  .date {
    font-weight: 600;
  }

  .cost {
    font-weight: 600;
    color: var(--text);
  }

  .description {
    margin: 0.25rem 0;
  }

  .meta {
    font-size: 0.85rem;
    color: var(--text-muted);
    display: flex;
    gap: 0.75rem;
  }

  .notes {
    font-size: 0.85rem;
    color: var(--text-muted);
    margin: 0.25rem 0 0;
    font-style: italic;
  }

  .empty {
    color: var(--text-muted);
    text-align: center;
    padding: 2rem 0;
  }
</style>
