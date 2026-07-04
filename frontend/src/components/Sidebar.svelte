<script lang="ts">
  // The sidebar IS the app chrome (2hea round 2): identity (logo), global
  // search, the garage list, and the nav verbs (Shops, + Add vehicle) all
  // live here — there is no top bar. The panel-left glyph collapses it;
  // App.svelte's slim rail carries reopen + search while collapsed.
  import { onMount } from 'svelte'
  import { link, location, push } from '@keenmate/svelte-spa-router'
  import { garageDashboard, refreshDashboard } from '../lib/stores'
  import type { VehicleSummary } from '../lib/types'
  import GlobalSearch from './GlobalSearch.svelte'

  let { onToggle, searchSignal = 0 }: {
    onToggle: () => void
    searchSignal?: number
  } = $props()

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
  <div class="sidebar-head">
    <a href="/" use:link class="logo">
      <span class="logo-icon" aria-hidden="true">⬡</span>
      Glovebox
    </a>
    <button
      class="panel-toggle"
      onclick={onToggle}
      aria-label="Toggle sidebar"
      title="Collapse sidebar"
    >
      <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round">
        <rect x="3" y="4" width="18" height="16" rx="2" />
        <path d="M9.5 4v16" />
        <path d="M16 10l-2.5 2 2.5 2" />
      </svg>
    </button>
  </div>

  <GlobalSearch {searchSignal} />

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

  <!-- Nav verbs, grouped at the bottom (no more top-right island). -->
  <div class="sidebar-foot">
    <a href="/shops" use:link class="foot-link" class:active={path === '/shops'}>
      <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <path d="M3 21h18"/>
        <path d="M5 21V7l8-4v18"/>
        <path d="M19 21V11l-6-4"/>
        <path d="M9 9v.01"/>
        <path d="M9 12v.01"/>
        <path d="M9 15v.01"/>
        <path d="M9 18v.01"/>
      </svg>
      Shops
    </a>
    <a href="/vehicles/new" use:link class="add-vehicle">+ Add vehicle</a>
  </div>
</aside>

<style>
  .sidebar {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    width: 236px;
    flex-shrink: 0;
    padding: var(--sp-3);
    border-right: 1px solid var(--border-subtle);
    background: var(--bg-raised);
    min-height: 100%;
  }

  /* --- Head: identity + the panel toggle, adjacent to what it controls --- */
  .sidebar-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--sp-2);
    padding: 0 var(--sp-1) var(--sp-1);
  }

  .logo {
    display: inline-flex;
    align-items: center;
    gap: var(--sp-2);
    font-family: var(--font-display);
    font-size: 1.15rem;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.14em;
    color: var(--text);
    text-decoration: none;
    transition: color var(--duration-fast) var(--ease-out);
    white-space: nowrap;
  }

  .logo:hover {
    color: var(--primary);
  }

  .logo-icon {
    font-size: 1.35rem;
    line-height: 1;
    color: var(--primary);
    transition: transform var(--duration-base) var(--ease-out);
  }

  .logo:hover .logo-icon {
    transform: rotate(30deg);
  }

  .panel-toggle {
    display: inline-flex;
    align-items: center;
    padding: var(--sp-1);
    background: none;
    border: none;
    border-radius: var(--radius-sm);
    color: var(--text-muted);
    cursor: pointer;
    transition:
      color var(--duration-fast) var(--ease-out),
      background var(--duration-fast) var(--ease-out);
  }

  .panel-toggle:hover {
    color: var(--primary);
    background: var(--surface);
  }

  .sidebar-label {
    font-family: var(--font-display);
    font-size: 0.72rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.16em;
    color: var(--text-muted);
    padding: var(--sp-2) var(--sp-2) var(--sp-1);
    display: flex;
    align-items: center;
    gap: var(--sp-2);
  }

  /* gauge-label tick */
  .sidebar-label::before {
    content: '';
    width: 3px;
    height: 11px;
    background: var(--primary);
    border-radius: 1px;
  }

  /* Garage bays: flat rows with a signal rail when parked on one. */
  .entry {
    position: relative;
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: var(--sp-2) var(--sp-3);
    border: 1px solid transparent;
    border-radius: var(--radius-md);
    background: none;
    color: var(--text);
    text-decoration: none;
    transition:
      border-color var(--duration-fast) var(--ease-out),
      background var(--duration-fast) var(--ease-out);
  }

  .entry::before {
    content: '';
    position: absolute;
    left: 0;
    top: 6px;
    bottom: 6px;
    width: 3px;
    border-radius: 2px;
    background: transparent;
    transition: background var(--duration-fast) var(--ease-out);
  }

  .entry:hover {
    background: var(--surface);
  }

  .entry.active {
    background: var(--surface);
    border-color: var(--border-subtle);
    box-shadow: inset 0 1px 0 var(--edge-highlight);
  }

  .entry.active::before {
    background: var(--primary);
  }

  .entry-name {
    font-family: var(--font-display);
    font-size: 1rem;
    font-weight: 600;
    letter-spacing: 0.02em;
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
    font-size: 0.7rem;
    color: var(--text-secondary);
    font-family: var(--font-numeral);
    font-variant-numeric: tabular-nums;
  }

  .hint {
    font-family: var(--font-display);
    font-size: 0.68rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.07em;
    padding: 0 var(--sp-2);
    border-radius: 999px;
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

  /* --- Foot: the occasional nav verbs --- */
  .sidebar-foot {
    margin-top: auto;
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    padding-top: var(--sp-3);
    border-top: 1px solid var(--border-subtle);
  }

  .foot-link {
    display: inline-flex;
    align-items: center;
    gap: var(--sp-2);
    padding: var(--sp-1) var(--sp-2);
    border-radius: var(--radius-md);
    font-family: var(--font-display);
    font-size: 0.85rem;
    font-weight: 600;
    letter-spacing: 0.05em;
    color: var(--text-secondary);
    text-decoration: none;
    transition:
      color var(--duration-fast) var(--ease-out),
      background var(--duration-fast) var(--ease-out);
  }

  .foot-link:hover,
  .foot-link.active {
    color: var(--primary);
    background: var(--surface);
  }

  .add-vehicle {
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
