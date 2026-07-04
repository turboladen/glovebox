<script lang="ts">
  import { Router } from '@keenmate/svelte-spa-router'
  import Header from './components/Header.svelte'
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

  function toggleSidebar() {
    sidebarOpen = !sidebarOpen
    localStorage.setItem(SIDEBAR_KEY, sidebarOpen ? 'open' : 'collapsed')
  }
</script>

<div class="app">
  <Header onToggleSidebar={toggleSidebar} />
  <div class="body">
    {#if sidebarOpen}
      <Sidebar />
    {:else}
      <button
        class="sidebar-handle"
        onclick={toggleSidebar}
        aria-label="Open sidebar"
        title="Open sidebar"
      >
        <span class="handle-glyph">›</span>
      </button>
    {/if}
    <main>
      <Router {routes} />
    </main>
  </div>
</div>

<style>
  .app {
    display: flex;
    flex-direction: column;
    min-height: 100vh;
  }

  .body {
    display: flex;
    flex: 1;
    align-items: stretch;
  }

  main {
    flex: 1;
    min-width: 0;
    max-width: 1080px;
    padding: var(--sp-6) var(--sp-5) var(--sp-12);
    margin: 0 auto;
  }

  /* Slim reopen handle when the sidebar is fully hidden */
  .sidebar-handle {
    flex-shrink: 0;
    width: 14px;
    background: var(--bg-raised);
    border: none;
    border-right: 1px solid var(--border-subtle);
    color: var(--text-muted);
    cursor: pointer;
    padding: 0;
    transition:
      color var(--duration-fast) var(--ease-out),
      background var(--duration-fast) var(--ease-out);
  }

  .sidebar-handle:hover {
    color: var(--primary);
    background: var(--surface);
  }

  .handle-glyph {
    font-size: 0.9rem;
  }

  @media (max-width: 640px) {
    main {
      padding: var(--sp-4) var(--sp-3) var(--sp-8);
    }
  }
</style>
