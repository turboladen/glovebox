<script lang="ts">
  import { Router, link } from '@keenmate/svelte-spa-router'
  import Sidebar from './components/Sidebar.svelte'
  import Dashboard from './components/Dashboard.svelte'
  import VehicleDetail from './components/VehicleDetail.svelte'
  import VehicleNew from './components/VehicleNew.svelte'
  import Shops from './components/Shops.svelte'
  import NotFound from './components/NotFound.svelte'

  const routes = {
    '/': Dashboard,
    '/shops': Shops,
    '/vehicles/new': VehicleNew,
    '/vehicles/:id': VehicleDetail,
    '/vehicles/:id/:tab': VehicleDetail,
    '/vehicles/:id/:tab/:sub': VehicleDetail,
    '*': NotFound,
  }

  const SIDEBAR_KEY = 'glovebox.sidebar'

  let sidebarOpen = $state(localStorage.getItem(SIDEBAR_KEY) !== 'collapsed')
  // Bumped when the collapsed rail's ⌕ opens the sidebar: search must stay
  // reachable while collapsed, and reopening for it should land in the box.
  let searchSignal = $state(0)

  function toggleSidebar() {
    sidebarOpen = !sidebarOpen
    localStorage.setItem(SIDEBAR_KEY, sidebarOpen ? 'open' : 'collapsed')
  }

  function openSidebarWithSearch() {
    sidebarOpen = true
    localStorage.setItem(SIDEBAR_KEY, 'open')
    searchSignal += 1
  }
</script>

<!-- No top bar: the sidebar is the app chrome (logo, search, nav verbs).
     Every vertical pixel above the content belongs to the content. -->
<div class="app">
  {#if sidebarOpen}
    <Sidebar onToggle={toggleSidebar} {searchSignal} />
  {:else}
    <nav class="rail" aria-label="Collapsed sidebar">
      <a href="/" use:link class="rail-logo" title="Glovebox" aria-label="Glovebox home">⬡</a>
      <button
        class="rail-btn"
        onclick={toggleSidebar}
        aria-label="Open sidebar"
        title="Open sidebar"
      >
        <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round">
          <rect x="3" y="4" width="18" height="16" rx="2" />
          <path d="M9.5 4v16" />
          <path d="M13.5 10l2.5 2-2.5 2" />
        </svg>
      </button>
      <button
        class="rail-btn"
        onclick={openSidebarWithSearch}
        aria-label="Search"
        title="Search"
      >
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round">
          <circle cx="11" cy="11" r="7" />
          <path d="M21 21l-4.5-4.5" />
        </svg>
      </button>
    </nav>
  {/if}
  <main>
    <Router {routes} />
  </main>
</div>

<style>
  .app {
    display: flex;
    align-items: stretch;
    min-height: 100vh;
  }

  /* Signal-lime accent line along the top of the page */
  .app::before {
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
    pointer-events: none;
  }

  main {
    flex: 1;
    min-width: 0;
    max-width: 1100px;
    padding: var(--sp-5) var(--sp-6) var(--sp-12);
    margin: 0 auto;
  }

  /* Slim rail when the sidebar is collapsed: reopen + search stay reachable. */
  .rail {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--sp-2);
    flex-shrink: 0;
    width: 44px;
    padding: var(--sp-3) 0;
    background: var(--bg-raised);
    border-right: 1px solid var(--border-subtle);
  }

  .rail-logo {
    font-size: 1.35rem;
    line-height: 1;
    color: var(--primary);
    text-decoration: none;
    margin-bottom: var(--sp-2);
    transition: transform var(--duration-base) var(--ease-out);
  }

  .rail-logo:hover {
    transform: rotate(30deg);
  }

  .rail-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 30px;
    height: 30px;
    padding: 0;
    background: none;
    border: none;
    border-radius: var(--radius-sm);
    color: var(--text-muted);
    cursor: pointer;
    transition:
      color var(--duration-fast) var(--ease-out),
      background var(--duration-fast) var(--ease-out);
  }

  .rail-btn:hover {
    color: var(--primary);
    background: var(--surface);
  }

  @media (max-width: 640px) {
    main {
      padding: var(--sp-4) var(--sp-3) var(--sp-8);
    }
  }
</style>
