<script lang="ts">
  // Global search, living at the top of the sidebar (2hea round 2): it no
  // longer scrolls away with a top bar, and the collapsed rail's ⌕ reopens
  // the sidebar straight into it (searchSignal). Results overlay the main
  // area — wider than the sidebar so snippets stay readable.
  import { push } from '@keenmate/svelte-spa-router'
  import { search as searchApi } from '../lib/api'
  import { garageDashboard } from '../lib/stores'
  import type { SearchHit } from '../lib/types'

  let { searchSignal = 0 }: { searchSignal?: number } = $props()

  let query = $state('')
  let hits: SearchHit[] = $state([])
  let searchOpen = $state(false)
  let searchBox: HTMLElement | undefined = $state(undefined)
  let input: HTMLInputElement | undefined = $state(undefined)
  let debounceTimer: ReturnType<typeof setTimeout> | undefined

  // The collapsed rail's ⌕ bumps searchSignal as it reopens the sidebar.
  $effect(() => {
    if (searchSignal > 0) input?.focus()
  })

  const kindLabels: Record<string, string> = {
    vehicle: 'Vehicles',
    service: 'Services',
    incident: 'Incidents',
    incident_followup: 'Incident followups',
    build: 'Builds',
    document: 'Documents',
    research_finding: 'Research',
    schedule_item: 'Maintenance',
    work_item: 'To-do',
  }

  // Planning hits fan out per applicable vehicle (an inherited "Air filter"
  // item is a destination on each car it applies to), so those rows carry the
  // vehicle name for disambiguation — sourced from the shared dashboard store.
  let vehicleNames = $derived(
    new Map(($garageDashboard?.vehicles ?? []).map((s) => [s.vehicle.id, s.vehicle.name])),
  )

  function vehicleName(hit: SearchHit): string | null {
    if (hit.kind !== 'schedule_item' && hit.kind !== 'work_item') return null
    return (hit.vehicle_id != null && vehicleNames.get(hit.vehicle_id)) || null
  }

  function onInput() {
    clearTimeout(debounceTimer)
    const q = query.trim()
    if (!q) {
      hits = []
      searchOpen = false
      return
    }
    debounceTimer = setTimeout(async () => {
      try {
        hits = await searchApi.query(q)
        searchOpen = true
      } catch (e) {
        console.error('Search failed:', e)
      }
    }, 200)
  }

  /** Every hit deep-links into the vehicle view that owns it. */
  function hitTarget(hit: SearchHit): string {
    const v = hit.vehicle_id
    if (v == null) return '/'
    switch (hit.kind) {
      case 'vehicle':
        return `/vehicles/${hit.id}`
      case 'service':
        return `/vehicles/${v}/timeline?hl=service:${hit.id}`
      case 'incident':
        return `/vehicles/${v}/timeline?hl=incident:${hit.id}`
      case 'incident_followup':
        return `/vehicles/${v}/timeline`
      case 'build':
        return `/vehicles/${v}/builds`
      case 'document':
        return `/vehicles/${v}/records/documents`
      case 'research_finding':
        return `/vehicles/${v}/plan/research?hl=finding:${hit.id}`
      case 'schedule_item':
        // Primary destination: the item's Due/overdue context. Items without
        // an active reminder card (dismissed/overridden) let the ?hl silently
        // no-op — the secondary ⚙ link reaches their Schedule-config entry.
        return `/vehicles/${v}/plan/due?hl=schedule_item:${hit.id}`
      case 'work_item':
        return `/vehicles/${v}/plan/todo?hl=work_item:${hit.id}`
      default:
        return `/vehicles/${v}`
    }
  }

  /** Secondary destination for schedule-item hits: the Schedule ⚙ entry. */
  function scheduleConfigTarget(hit: SearchHit): string {
    return `/vehicles/${hit.vehicle_id}/plan/schedule?hl=schedule_item:${hit.id}`
  }

  function openHit(hit: SearchHit) {
    closeSearch()
    push(hitTarget(hit))
  }

  function openScheduleConfig(hit: SearchHit) {
    closeSearch()
    push(scheduleConfigTarget(hit))
  }

  function closeSearch() {
    searchOpen = false
    query = ''
    hits = []
  }

  let grouped = $derived(
    Object.entries(
      hits.reduce<Record<string, SearchHit[]>>((acc, h) => {
        ;(acc[h.kind] ??= []).push(h)
        return acc
      }, {}),
    ),
  )

  function onDocClick(e: MouseEvent) {
    if (searchOpen && searchBox && !searchBox.contains(e.target as Node)) {
      searchOpen = false
    }
  }
</script>

<svelte:document onclick={onDocClick} />

<div class="search" bind:this={searchBox}>
  <span class="search-glyph" aria-hidden="true">
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round">
      <circle cx="11" cy="11" r="7" />
      <path d="M21 21l-4.5-4.5" />
    </svg>
  </span>
  <input
    type="search"
    placeholder="Search everything…"
    bind:this={input}
    bind:value={query}
    oninput={onInput}
    onfocus={() => { if (hits.length > 0) searchOpen = true }}
    onkeydown={(e) => { if (e.key === 'Escape') closeSearch() }}
    aria-label="Search"
  />
  {#if searchOpen}
    <div class="search-results">
      {#if hits.length === 0}
        <p class="no-hits">No matches.</p>
      {:else}
        {#each grouped as [kind, group] (kind)}
          <div class="hit-group">
            <div class="hit-group-label">{kindLabels[kind] ?? kind}</div>
            <!-- Key includes vehicle_id: planning hits fan out per vehicle. -->
            {#each group as hit (`${hit.kind}-${hit.id}-${hit.vehicle_id}`)}
              <div class="hit-row">
                <button class="hit" onclick={() => openHit(hit)}>
                  <span class="hit-title">
                    {hit.title}
                    {#if vehicleName(hit)}
                      <span class="hit-vehicle">{vehicleName(hit)}</span>
                    {/if}
                  </span>
                  {#if hit.snippet}
                    <span class="hit-snippet">{hit.snippet.replace(/[\[\]]/g, '')}</span>
                  {/if}
                </button>
                {#if hit.kind === 'schedule_item'}
                  <!-- Both destinations of a schedule item: the primary row
                       goes to its Due context, ⚙ to its Schedule-config entry. -->
                  <button
                    class="hit-alt"
                    title="Open in Schedule ⚙"
                    aria-label="Open in Schedule ⚙"
                    onclick={() => openScheduleConfig(hit)}
                  >
                    ⚙ schedule
                  </button>
                {/if}
              </div>
            {/each}
          </div>
        {/each}
      {/if}
    </div>
  {/if}
</div>

<style>
  .search {
    position: relative;
  }

  .search-glyph {
    position: absolute;
    left: 0.6rem;
    top: 50%;
    transform: translateY(-50%);
    display: inline-flex;
    color: var(--text-muted);
    pointer-events: none;
  }

  .search input {
    font-size: 0.83rem;
    padding: 0.35rem 0.6rem 0.35rem 1.8rem;
    background: var(--bg);
    border-radius: var(--radius-md);
  }

  /* Results fly out OVER the main area — the sidebar is too narrow to
     contain snippets, and clipping them would defeat the search. This
     z-index only orders the overlay WITHIN the sidebar's own stacking
     context (position: sticky creates one); floating above the main
     area's positioned elements is the sidebar's z-index in Sidebar.svelte. */
  .search-results {
    position: absolute;
    top: calc(100% + 4px);
    left: 0;
    width: min(340px, 82vw);
    max-height: 65vh;
    overflow-y: auto;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    box-shadow: var(--shadow-lg);
    z-index: 60;
    padding: var(--sp-2);
  }

  .no-hits {
    margin: 0;
    padding: var(--sp-2) var(--sp-3);
    font-size: 0.85rem;
    color: var(--text-muted);
  }

  .hit-group + .hit-group {
    margin-top: var(--sp-2);
    border-top: 1px solid var(--border-subtle);
    padding-top: var(--sp-2);
  }

  .hit-group-label {
    font-family: var(--font-display);
    font-size: 0.7rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--text-muted);
    padding: 0 var(--sp-2) var(--sp-1);
  }

  .hit-row {
    display: flex;
    align-items: stretch;
    gap: 2px;
  }

  .hit {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 1px;
    flex: 1;
    min-width: 0;
    padding: var(--sp-2);
    border: none;
    border-radius: var(--radius-sm);
    background: none;
    text-align: left;
    cursor: pointer;
    color: var(--text);
    transition: background var(--duration-fast) var(--ease-out);
  }

  .hit:hover {
    background: var(--surface-hover);
  }

  .hit-title {
    font-size: 0.85rem;
    font-weight: 500;
  }

  .hit-snippet {
    font-size: 0.75rem;
    color: var(--text-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 100%;
  }

  /* Vehicle chip on planning hits (they fan out per applicable vehicle). */
  .hit-vehicle {
    font-family: var(--font-display);
    font-size: 0.66rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--text-muted);
    border: 1px solid var(--border);
    border-radius: 999px;
    padding: 0 var(--sp-2);
    margin-left: var(--sp-1);
    vertical-align: 1px;
  }

  /* Secondary destination for a schedule-item hit: its Schedule ⚙ entry. */
  .hit-alt {
    flex-shrink: 0;
    align-self: center;
    padding: var(--sp-1) var(--sp-2);
    border: none;
    border-radius: var(--radius-sm);
    background: none;
    font-size: 0.72rem;
    color: var(--text-muted);
    white-space: nowrap;
    cursor: pointer;
    transition:
      color var(--duration-fast) var(--ease-out),
      background var(--duration-fast) var(--ease-out);
  }

  .hit-alt:hover {
    color: var(--primary);
    background: var(--surface-hover);
  }
</style>
