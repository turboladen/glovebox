<script lang="ts">
  import { onMount } from 'svelte'
  import { services as servicesApi, observations as obsApi, parts as partsApi } from '../lib/api'
  import type { ServiceRecordWithLinks, Observation, Part } from '../lib/types'
  import { formatDate } from '../lib/dates'

  let { vehicleId }: { vehicleId: number } = $props()

  type TimelineEntry =
    | { type: 'service'; date: string; data: ServiceRecordWithLinks }
    | { type: 'observation'; date: string; data: Observation }

  let entries: TimelineEntry[] = $state([])
  let allParts: Part[] = $state([])
  let allObservations: Observation[] = $state([])
  let loading = $state(true)
  let filter = $state<'all' | 'services' | 'observations'>('all')

  onMount(async () => {
    try {
      const [svcList, obsList, partsList] = await Promise.all([
        servicesApi.list(vehicleId),
        obsApi.list(vehicleId),
        partsApi.list(vehicleId),
      ])

      allParts = partsList
      allObservations = obsList

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

  function partsForService(service: ServiceRecordWithLinks): Part[] {
    if (!service.part_ids.length) return []
    const ids = new Set(service.part_ids)
    return allParts.filter(p => ids.has(p.id))
  }

  function resolvedObsForService(serviceId: number): Observation[] {
    return allObservations.filter(o => o.resolved_service_id === serviceId)
  }

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
            <span class="date">{formatDate(record.service_date)}</span>
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
          {#if partsForService(record).length > 0}
            <div class="linked-items">
              <span class="linked-label">Parts:</span>
              {#each partsForService(record) as part (part.id)}
                <span class="linked-chip part-chip">{part.name}</span>
              {/each}
            </div>
          {/if}
          {#if resolvedObsForService(record.id).length > 0}
            <div class="linked-items">
              <span class="linked-label">Resolved:</span>
              {#each resolvedObsForService(record.id) as obs (obs.id)}
                <span class="linked-chip obs-chip">{obs.title}</span>
              {/each}
            </div>
          {/if}
        </div>
      {:else}
        {@const obs = entry.data}
        <div class="history-card obs-card" class:resolved={obs.resolved}>
          <div class="history-header">
            <span class="type-badge obs-badge">Observation</span>
            <span class="date">{formatDate(obs.observed_at)}</span>
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
    display: flex; gap: var(--sp-1); margin-bottom: var(--sp-4);
    border: 1px solid var(--border-subtle); border-radius: var(--radius-md); overflow: hidden; width: fit-content;
  }

  .filter-btn {
    padding: var(--sp-1) var(--sp-3); border: none; background: none;
    font-family: var(--font-display); font-size: 0.85rem; cursor: pointer; color: var(--text-muted);
    transition: background var(--duration-fast) var(--ease-out), color var(--duration-fast) var(--ease-out);
  }

  .filter-btn.active {
    background: var(--primary); color: var(--primary-text);
  }

  .history-list { display: flex; flex-direction: column; gap: var(--sp-2); }

  .history-card {
    padding: var(--sp-3) var(--sp-4); border: 1px solid var(--border-subtle); border-radius: var(--radius-md);
    background: var(--bg-raised);
    transition:
      border-color var(--duration-base) var(--ease-out),
      box-shadow var(--duration-base) var(--ease-out),
      transform var(--duration-base) var(--ease-out);
  }

  .history-card:hover {
    border-color: var(--border);
    box-shadow: var(--shadow-sm);
    transform: translateY(-1px);
  }

  .history-card.resolved { opacity: 0.6; }

  .history-header {
    display: flex; align-items: center; gap: var(--sp-2);
  }

  .type-badge {
    font-family: var(--font-display);
    font-size: 0.7rem; text-transform: uppercase; letter-spacing: 0.05em;
    padding: 0.1rem 0.4rem; border-radius: var(--radius-sm); font-weight: 600;
  }

  .service-badge { background: var(--success-bg); color: var(--success); }
  .obs-badge { background: var(--warning-bg); color: var(--warning); }
  .resolved-badge { font-size: 0.75rem; color: var(--success); }

  .date { font-weight: 600; }
  .cost { margin-left: auto; font-weight: 600; }
  .description { margin: var(--sp-1) 0; }
  .meta { font-size: 0.85rem; color: var(--text-muted); display: flex; gap: var(--sp-3); }
  .category { text-transform: capitalize; }
  .notes { font-size: 0.85rem; color: var(--text-muted); margin: var(--sp-1) 0 0; font-style: italic; }
  .empty { color: var(--text-muted); text-align: center; padding: var(--sp-8) 0; }

  .linked-items {
    display: flex; flex-wrap: wrap; align-items: center; gap: var(--sp-1);
    margin-top: var(--sp-2); font-size: 0.8rem;
  }

  .linked-label {
    font-weight: 600; color: var(--text-muted); font-size: 0.75rem;
    text-transform: uppercase; letter-spacing: 0.03em;
  }

  .linked-chip {
    padding: 0.1rem 0.5rem; border-radius: var(--radius-sm); font-size: 0.8rem;
  }

  .part-chip { background: var(--success-bg); color: var(--success); }
  .obs-chip { background: var(--warning-bg); color: var(--warning); }
</style>
