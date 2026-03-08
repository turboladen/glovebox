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
    <a href="/" class="logo">
      <span class="logo-icon" aria-hidden="true">⬡</span>
      Glovebox
    </a>
  </header>
  <main>
    <Router {routes} />
  </main>
</div>

<style>
  .app {
    max-width: 960px;
    margin: 0 auto;
    padding: 0 var(--sp-4);
  }

  header {
    display: flex;
    align-items: center;
    padding: var(--sp-4) 0;
    border-bottom: 1px solid var(--border-subtle);
    margin-bottom: var(--sp-6);
  }

  /* Guards Red accent line along top of page */
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
      var(--primary) 20%,
      var(--primary) 80%,
      transparent 100%
    );
    z-index: 100;
    opacity: 0.7;
  }

  .logo {
    display: inline-flex;
    align-items: center;
    gap: var(--sp-2);
    font-family: var(--font-display);
    font-size: 1.2rem;
    font-weight: 700;
    letter-spacing: -0.02em;
    color: var(--text);
    text-decoration: none;
    transition: color var(--duration-fast) var(--ease-out);
  }

  .logo:hover {
    color: var(--primary);
  }

  .logo-icon {
    font-size: 1.1rem;
    color: var(--primary);
    transition: transform var(--duration-base) var(--ease-out);
  }

  .logo:hover .logo-icon {
    transform: rotate(30deg);
  }

  main {
    padding-bottom: var(--sp-12);
  }

  @media (max-width: 640px) {
    .app {
      padding: 0 var(--sp-3);
    }

    header {
      padding: var(--sp-3) 0;
      margin-bottom: var(--sp-4);
    }

    .logo {
      font-size: 1.05rem;
    }
  }
</style>
