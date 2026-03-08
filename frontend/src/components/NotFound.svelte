<script lang="ts">
  import { link } from '@keenmate/svelte-spa-router'
</script>

<div class="not-found">
  <h1>404</h1>
  <p>Page not found.</p>
  <a href="/" use:link>Back to Garage</a>
</div>

<style>
  .not-found {
    text-align: center;
    padding: var(--sp-12) 0;
    color: var(--text-muted);
  }

  .not-found h1 {
    font-family: var(--font-display);
    font-size: 3rem;
    margin-bottom: var(--sp-2);
  }
</style>
