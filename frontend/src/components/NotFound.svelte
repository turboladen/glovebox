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
    padding: 4rem 0;
    color: var(--text-muted);
  }

  .not-found h1 {
    font-size: 3rem;
    margin-bottom: 0.5rem;
  }
</style>
