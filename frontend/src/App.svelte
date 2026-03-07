<script lang="ts">
  import Router from '@keenmate/svelte-spa-router'
  import Garage from './components/Garage.svelte'
  import VehicleDetail from './components/VehicleDetail.svelte'
  import VehicleNew from './components/VehicleNew.svelte'
  import NotFound from './components/NotFound.svelte'

  const routes = {
    '/': Garage,
    '/vehicles/new': VehicleNew,
    '/vehicles/:id': VehicleDetail,
    '/vehicles/:id/*': VehicleDetail,
    '*': NotFound,
  }
</script>

<div class="app">
  <header>
    <a href="/" class="logo">Glovebox</a>
  </header>
  <main>
    <Router {routes} />
  </main>
</div>

<style>
  .app {
    max-width: 960px;
    margin: 0 auto;
    padding: 0 1rem;
  }

  header {
    padding: 1rem 0;
    border-bottom: 1px solid var(--border);
    margin-bottom: 1.5rem;
  }

  .logo {
    font-size: 1.25rem;
    font-weight: 700;
    color: var(--text);
    text-decoration: none;
  }
</style>
