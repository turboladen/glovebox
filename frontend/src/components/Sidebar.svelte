<script lang="ts">
  import { onMount } from 'svelte'
  import { link, location, push } from '@keenmate/svelte-spa-router'
  import { garageDashboard, refreshDashboard } from '../lib/stores'
  import type { VehicleSummary } from '../lib/types'

  let showArchived = $state(false)

  let path = $derived(location() ?? '/')
  let dash = $derived($garageDashboard)
  let active = $derived((dash?.vehicles ?? []).filter((s) => !s.vehicle.archived_at))
  let archived = $derived((dash?.vehicles ?? []).filter((s) => s.vehicle.archived_at))

  onMount(() => {
    refreshDashboard().catch((e) => console.error('Dashboard load failed:', e))
  })

  function isVehicleActive(id: number): boolean {
    return path === `/vehicles/${id}` || path.startsWith(`/vehicles/${id}/`)
  }

  function subtitle(s: VehicleSummary): string {
    return [s.vehicle.year, s.vehicle.make, s.vehicle.model].filter(Boolean).join(' ')
  }

  function formatMileage(n: number): string {
    return n.toLocaleString()
  }
</script>

<aside class="sidebar" data-testid="sidebar">
  <div class="sidebar-label">Garage</div>

  <a href="/" use:link class="entry all-vehicles" class:active={path === '/'}>
    <span class="entry-name">All vehicles</span>
  </a>

  {#each active as s (s.vehicle.id)}
    <a
      href="/vehicles/{s.vehicle.id}"
      use:link
      class="entry vehicle"
      class:active={isVehicleActive(s.vehicle.id)}
    >
      <span class="entry-name">{s.vehicle.name}</span>
      {#if subtitle(s)}
        <span class="entry-sub">{subtitle(s)}</span>
      {/if}
      <span class="entry-hints">
        {#if s.estimated_mileage != null}
          <span class="hint-mileage">{formatMileage(s.estimated_mileage)} mi</span>
        {/if}
        {#if s.overdue_count > 0}
          <!-- Hypermedia affordance: the count links to what's due, it
               doesn't just report it. stopPropagation keeps the card's
               own navigation for the rest of the entry. -->
          <button
            class="hint hint-due hint-link"
            title="View due maintenance"
            onclick={(e) => {
              e.preventDefault()
              e.stopPropagation()
              push(`/vehicles/${s.vehicle.id}/plan/due`)
            }}
          >
            {s.overdue_count} due
          </button>
        {:else if s.due_soon_count > 0}
          <span class="hint hint-soon">{s.due_soon_count} soon</span>
        {/if}
        {#if s.open_recall_count > 0}
          <span class="hint hint-due">recall</span>
        {/if}
        {#if s.active_build}
          <span class="hint hint-build">build active</span>
        {/if}
      </span>
    </a>
  {/each}

  {#if archived.length > 0}
    <button class="archived-toggle" onclick={() => (showArchived = !showArchived)}>
      <span class="chevron" class:open={showArchived}>&#9654;</span>
      Archived ({archived.length})
    </button>
    {#if showArchived}
      {#each archived as s (s.vehicle.id)}
        <a
          href="/vehicles/{s.vehicle.id}"
          use:link
          class="entry vehicle archived"
          class:active={isVehicleActive(s.vehicle.id)}
        >
          <span class="entry-name">{s.vehicle.name}</span>
        </a>
      {/each}
    {/if}
  {/if}

  <a href="/vehicles/new" use:link class="add-vehicle">+ Add vehicle</a>
</aside>

<style>
  .sidebar {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    width: 230px;
    flex-shrink: 0;
    padding: var(--sp-4) var(--sp-3);
    border-right: 1px solid var(--border-subtle);
    background: var(--bg-raised);
    min-height: 100%;
  }

  .sidebar-label {
    font-family: var(--font-display);
    font-size: 0.7rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--text-muted);
    padding: 0 var(--sp-2);
  }

  .entry {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: var(--sp-2) var(--sp-3);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    background: var(--surface);
    color: var(--text);
    text-decoration: none;
    transition:
      border-color var(--duration-fast) var(--ease-out),
      background var(--duration-fast) var(--ease-out);
  }

  .entry:hover {
    border-color: var(--border);
    background: var(--surface-hover);
  }

  .entry.active {
    border-color: var(--primary);
    background: var(--primary-muted);
  }

  .entry-name {
    font-family: var(--font-display);
    font-size: 0.9rem;
    font-weight: 600;
  }

  .entry-sub {
    font-size: 0.72rem;
    color: var(--text-muted);
  }

  .entry-hints {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    flex-wrap: wrap;
    margin-top: 2px;
  }

  .entry-hints:empty {
    display: none;
  }

  .hint-mileage {
    font-size: 0.72rem;
    color: var(--text-secondary);
    font-family: var(--font-display);
    font-weight: 600;
  }

  .hint {
    font-size: 0.65rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    padding: 0 var(--sp-1);
    border-radius: var(--radius-sm);
  }

  .hint-due {
    background: var(--danger-bg);
    color: var(--danger);
    border: 1px solid var(--danger-border);
  }

  .hint-link {
    font-family: inherit;
    cursor: pointer;
    transition:
      border-color var(--duration-fast) var(--ease-out),
      text-decoration-color var(--duration-fast) var(--ease-out);
  }

  .hint-link:hover {
    text-decoration: underline;
    border-color: var(--danger);
  }

  .hint-soon {
    background: var(--warning-bg);
    color: var(--warning);
    border: 1px solid var(--warning-border);
  }

  .hint-build {
    background: var(--success-bg);
    color: var(--success);
    border: 1px solid var(--success-border);
  }

  .archived-toggle {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    background: none;
    border: none;
    color: var(--text-muted);
    font-size: 0.78rem;
    font-weight: 600;
    cursor: pointer;
    padding: var(--sp-1) var(--sp-2);
    text-align: left;
  }

  .archived-toggle:hover {
    color: var(--text);
  }

  .chevron {
    font-size: 0.6rem;
    transition: transform var(--duration-fast) var(--ease-out);
  }

  .chevron.open {
    transform: rotate(90deg);
  }

  .entry.archived {
    opacity: 0.6;
  }

  .add-vehicle {
    margin-top: auto;
    padding: var(--sp-2) var(--sp-3);
    font-size: 0.82rem;
    font-weight: 500;
    font-family: var(--font-display);
    color: var(--text-secondary);
    text-decoration: none;
    border: 1px dashed var(--border);
    border-radius: var(--radius-md);
    text-align: center;
    transition:
      color var(--duration-fast) var(--ease-out),
      border-color var(--duration-fast) var(--ease-out);
  }

  .add-vehicle:hover {
    color: var(--primary);
    border-color: var(--primary);
  }
</style>
