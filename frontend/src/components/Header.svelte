<script lang="ts">
  import { link, push } from '@keenmate/svelte-spa-router'
  import { search as searchApi } from '../lib/api'
  import type { SearchHit } from '../lib/types'

  let { onToggleSidebar }: { onToggleSidebar: () => void } = $props()

  let query = $state('')
  let hits: SearchHit[] = $state([])
  let searchOpen = $state(false)
  let searchBox: HTMLElement | undefined = $state(undefined)
  let debounceTimer: ReturnType<typeof setTimeout> | undefined

  const kindLabels: Record<string, string> = {
    vehicle: 'Vehicles',
    service: 'Services',
    incident: 'Incidents',
    incident_followup: 'Incident followups',
    build: 'Builds',
    document: 'Documents',
    research_finding: 'Research',
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
      default:
        return `/vehicles/${v}`
    }
  }

  function openHit(hit: SearchHit) {
    closeSearch()
    push(hitTarget(hit))
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

<header>
  <div class="header-left">
    <button
      class="sidebar-toggle"
      onclick={onToggleSidebar}
      aria-label="Toggle sidebar"
      title="Toggle sidebar"
    >
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round">
        <path d="M4 6h16" /><path d="M4 12h16" /><path d="M4 18h16" />
      </svg>
    </button>
    <a href="/" use:link class="logo">
      <span class="logo-icon" aria-hidden="true">⬡</span>
      Glovebox
    </a>
  </div>

  <div class="search" bind:this={searchBox}>
    <input
      type="search"
      placeholder="Search everything…"
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
              {#each group as hit (hit.kind + '-' + hit.id)}
                <button class="hit" onclick={() => openHit(hit)}>
                  <span class="hit-title">{hit.title}</span>
                  {#if hit.snippet}
                    <span class="hit-snippet">{hit.snippet.replace(/[\[\]]/g, '')}</span>
                  {/if}
                </button>
              {/each}
            </div>
          {/each}
        {/if}
      </div>
    {/if}
  </div>

  <nav class="header-nav">
    <a href="/shops" use:link class="nav-link" title="Shops">
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
        <path d="M3 21h18"/>
        <path d="M5 21V7l8-4v18"/>
        <path d="M19 21V11l-6-4"/>
        <path d="M9 9v.01"/>
        <path d="M9 12v.01"/>
        <path d="M9 15v.01"/>
        <path d="M9 18v.01"/>
      </svg>
    </a>
  </nav>
</header>

<style>
  header {
    display: flex;
    align-items: center;
    gap: var(--sp-4);
    padding: var(--sp-3) var(--sp-4);
    border-bottom: 1px solid var(--border-subtle);
    background: var(--bg-raised);
    box-shadow: inset 0 1px 0 var(--edge-highlight);
  }

  /* Signal-lime accent line along top of page */
  header::before {
    content: '';
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    height: 2px;
    background: linear-gradient(
      90deg,
      transparent 0%,
      var(--primary) 15%,
      var(--primary) 85%,
      transparent 100%
    );
    z-index: 100;
    opacity: 0.8;
  }

  .header-left {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
  }

  .sidebar-toggle {
    display: inline-flex;
    align-items: center;
    padding: var(--sp-1);
    background: none;
    border: none;
    border-radius: var(--radius-sm);
    color: var(--text-muted);
    cursor: pointer;
    transition: color var(--duration-fast) var(--ease-out);
  }

  .sidebar-toggle:hover {
    color: var(--primary);
  }

  .logo {
    display: inline-flex;
    align-items: center;
    gap: var(--sp-2);
    font-family: var(--font-display);
    font-size: 1.25rem;
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
    font-size: 1.45rem;
    line-height: 1;
    color: var(--primary);
    transition: transform var(--duration-base) var(--ease-out);
  }

  .logo:hover .logo-icon {
    transform: rotate(30deg);
  }

  /* --- Global search --- */
  .search {
    position: relative;
    flex: 1;
    max-width: 420px;
    margin: 0 auto;
  }

  .search input {
    font-size: 0.85rem;
    padding: var(--sp-2) var(--sp-4);
    background: var(--bg);
    border-radius: 999px;
  }

  .search-results {
    position: absolute;
    top: calc(100% + 4px);
    left: 0;
    right: 0;
    max-height: 60vh;
    overflow-y: auto;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    box-shadow: var(--shadow-lg);
    z-index: 50;
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

  .hit {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 1px;
    width: 100%;
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

  .header-nav {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
  }

  .nav-link {
    display: flex;
    align-items: center;
    color: var(--text-muted);
    transition: color var(--duration-fast) var(--ease-out);
  }

  .nav-link:hover {
    color: var(--primary);
  }

  @media (max-width: 640px) {
    header {
      gap: var(--sp-2);
      padding: var(--sp-2) var(--sp-3);
    }

    .logo {
      font-size: 1.05rem;
    }
  }
</style>
