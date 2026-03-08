<script lang="ts">
  import { onMount } from 'svelte'
  import { services as servicesApi, observations as obsApi } from '../lib/api'
  import type { ServiceRecordWithLinks, Observation } from '../lib/types'

  let { vehicleId }: { vehicleId: number } = $props()

  type TimelineEntry =
    | { type: 'service'; date: string; data: ServiceRecordWithLinks }
    | { type: 'observation'; date: string; data: Observation }

  let entries: TimelineEntry[] = $state([])
  let loading = $state(true)
  let filter = $state<'all' | 'services' | 'observations'>('all')

  onMount(async () => {
    try {
      const [svcList, obsList] = await Promise.all([
        servicesApi.list(vehicleId),
        obsApi.list(vehicleId),
      ])

      const timeline: TimelineEntry[] = [
        ...svcList.map((s): TimelineEntry => ({ type: 'service', date: s.service_date, data: s })),
        ...obsList.map((o): TimelineEntry => ({ type: 'observation', date: o.observed_at, data: o })),
      ]
      timeline.sort((a, b) => b.date.localeCompare(a.date))
      entries = timeline
    } catch (e) {
      console.error(e)
    } finally {
      loading = false
    }
  })

  let filtered = $derived(
    filter === 'all' ? entries :
    filter === 'services' ? entries.filter(e => e.type === 'service') :
    entries.filter(e => e.type === 'observation')
  )

  function formatCents(cents: number | null): string {
    if (cents == null) return ''
    return `$${(cents / 100).toFixed(2)}`
  }

  function formatMileage(n: number | null): string {
    return n != null ? n.toLocaleString() + ' mi' : ''
  }
</script>

{#if loading}
  <p>Loading history...</p>
{:else if entries.length === 0}
  <p class="empty">No history yet.</p>
{:else}
  <div class="filter-bar">
    <button class="filter-btn" class:active={filter === 'all'} onclick={() => (filter = 'all')}>All</button>
    <button class="filter-btn" class:active={filter === 'services'} onclick={() => (filter = 'services')}>Services</button>
    <button class="filter-btn" class:active={filter === 'observations'} onclick={() => (filter = 'observations')}>Observations</button>
  </div>

  <div class="history-list">
    {#each filtered as entry (entry.type + '-' + (entry.type === 'service' ? entry.data.id : entry.data.id))}
      {#if entry.type === 'service'}
        {@const record = entry.data}
        <div class="history-card service-card">
          <div class="history-header">
            <span class="type-badge service-badge">Service</span>
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
      {:else}
        {@const obs = entry.data}
        <div class="history-card obs-card" class:resolved={obs.resolved}>
          <div class="history-header">
            <span class="type-badge obs-badge">Observation</span>
            <span class="date">{obs.observed_at.split(' ')[0]}</span>
            {#if obs.resolved}
              <span class="resolved-badge">Resolved</span>
            {/if}
          </div>
          <p class="description">{obs.title}</p>
          {#if obs.description}
            <p class="notes">{obs.description}</p>
          {/if}
          <div class="meta">
            {#if obs.odometer}
              <span>{formatMileage(obs.odometer)}</span>
            {/if}
            <span class="category">{obs.category.replace(/_/g, ' ')}</span>
          </div>
        </div>
      {/if}
    {/each}
  </div>
{/if}

<style>
  .filter-bar {
    display: flex; gap: 0.25rem; margin-bottom: 1rem;
    border: 1px solid var(--border); border-radius: 4px; overflow: hidden; width: fit-content;
  }

  .filter-btn {
    padding: 0.3rem 0.75rem; border: none; background: none;
    font-size: 0.85rem; cursor: pointer; color: var(--text-muted);
  }

  .filter-btn.active {
    background: var(--primary); color: white;
  }

  .history-list { display: flex; flex-direction: column; gap: 0.5rem; }

  .history-card {
    padding: 0.75rem 1rem; border: 1px solid var(--border); border-radius: 4px;
  }

  .history-card.resolved { opacity: 0.6; }

  .history-header {
    display: flex; align-items: center; gap: 0.5rem;
  }

  .type-badge {
    font-size: 0.7rem; text-transform: uppercase; letter-spacing: 0.05em;
    padding: 0.1rem 0.4rem; border-radius: 3px; font-weight: 600;
  }

  .service-badge { background: var(--success-bg); color: var(--success); }
  .obs-badge { background: var(--warning-bg); color: var(--warning); }
  .resolved-badge { font-size: 0.75rem; color: var(--success); }

  .date { font-weight: 600; }
  .cost { margin-left: auto; font-weight: 600; }
  .description { margin: 0.25rem 0; }
  .meta { font-size: 0.85rem; color: var(--text-muted); display: flex; gap: 0.75rem; }
  .category { text-transform: capitalize; }
  .notes { font-size: 0.85rem; color: var(--text-muted); margin: 0.25rem 0 0; font-style: italic; }
  .empty { color: var(--text-muted); text-align: center; padding: 2rem 0; }
</style>
