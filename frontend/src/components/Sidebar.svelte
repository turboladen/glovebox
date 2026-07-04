<script lang="ts">
  // The sidebar IS the app chrome (2hea round 2): identity (logo), global
  // search, the garage list, and the occasional nav verb (Shops) all live
  // here — there is no top bar. The panel-left glyph collapses it;
  // App.svelte's slim rail carries reopen + search while collapsed.
  // Round 3: the panel is viewport-height — head (logo/search) and foot
  // (Shops) stay pinned while the garage list scrolls internally; adding
  // a vehicle is a page-level action on the garage dashboard, not a nav
  // verb parked next to Shops.
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

  <div class="sidebar-scroll">
  <!-- The section header is a real control, not a dead label — it goes to
       the garage overview, same destination as the logo. -->
  <a href="/" use:link class="sidebar-label" title="Garage overview">Garage</a>

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
  </div>

  <!-- Foot: pinned nav verb (adding a vehicle lives on the garage page). -->
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
  </div>
</aside>

<style>
  /* Viewport-height panel (round-3 feedback #4): the page scroll must
     never carry the foot away — head and foot pin, the list scrolls. */
  .sidebar {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    width: 236px;
    flex-shrink: 0;
    padding: var(--sp-3);
    border-right: 1px solid var(--border-subtle);
    background: var(--bg-raised);
    position: sticky;
    top: 0;
    align-self: flex-start;
    height: 100vh;
    /* NO `overflow: hidden` here: it would clip the search-results overlay
       at the sidebar's right edge (round-3 regression). Internal scrolling
       is .sidebar-scroll's job; nothing else overflows the fixed height.
       And because position: sticky makes the sidebar a stacking context,
       the overlay's own z-index can never escape it — without a z-index
       HERE the whole sidebar (overlay included) paints under the main
       area's positioned elements (dropdowns z:10, overflow menus z:40).
       50 floats the overlay over all page content while staying under
       modal backdrops (z:100). */
    z-index: 50;
  }

  /* The garage list scrolls internally between the pinned head and foot. */
  .sidebar-scroll {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    /* keep the entries' focus rings/rails from clipping at the edge */
    margin: 0 calc(-1 * var(--sp-1));
    padding: 0 var(--sp-1);
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
    text-decoration: none;
    width: fit-content;
    transition: color var(--duration-fast) var(--ease-out);
  }

  /* A live control gets a hover voice (round-3 feedback #2). */
  .sidebar-label:hover {
    color: var(--primary);
    text-decoration: underline;
    text-underline-offset: 3px;
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

  /* --- Foot: the occasional nav verb, pinned below the scroll region --- */
  .sidebar-foot {
    flex-shrink: 0;
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
</style>
