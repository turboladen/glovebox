<script lang="ts">
  import { onMount } from 'svelte'
  import { link } from '@keenmate/svelte-spa-router'
  import { vehicles as vehiclesApi, reminders as remindersApi } from '../lib/api'
  import type { Vehicle, RemindersResponse } from '../lib/types'

  let vehicleList: Vehicle[] = $state([])
  let reminderMap: Map<number, RemindersResponse> = $state(new Map())
  let loading = $state(true)
  let error = $state('')

  onMount(async () => {
    try {
      vehicleList = await vehiclesApi.list()
      // Fetch reminders for each vehicle in parallel
      const results = await Promise.allSettled(
        vehicleList.map((v) => remindersApi.get(v.id))
      )
      for (let i = 0; i < results.length; i++) {
        const result = results[i]
        if (result.status === 'fulfilled') {
          reminderMap.set(vehicleList[i].id, result.value)
        }
      }
      reminderMap = reminderMap // trigger reactivity
    } catch (e: any) {
      error = e.message
    } finally {
      loading = false
    }
  })

  function statusSummary(r: RemindersResponse | undefined) {
    if (!r) return { overdue: 0, upcoming: 0 }
    return {
      overdue: r.reminders.filter((i) => i.status === 'overdue').length,
      upcoming: r.reminders.filter((i) => i.status === 'upcoming').length,
    }
  }

  function formatMileage(n: number): string {
    return n.toLocaleString()
  }
</script>

<div class="garage">
  <div class="garage-header">
    <div>
      <h1>Garage</h1>
      {#if !loading && vehicleList.length > 0}
        <p class="garage-count">{vehicleList.length} vehicle{vehicleList.length !== 1 ? 's' : ''}</p>
      {/if}
    </div>
    <a href="/vehicles/new" use:link class="btn btn-primary">+ Add Car</a>
  </div>

  {#if loading}
    <div class="vehicle-grid">
      {#each Array(6) as _, i}
        <div class="vehicle-card skeleton-card-wrap" style="--delay: {i * 80}ms">
          <div class="card-photo skeleton"></div>
          <div class="card-body">
            <div class="skeleton skeleton-heading"></div>
            <div class="skeleton skeleton-text"></div>
            <div style="margin-top: var(--sp-3); padding-top: var(--sp-3); border-top: 1px solid var(--border-subtle);">
              <div class="skeleton skeleton-text-short"></div>
            </div>
          </div>
        </div>
      {/each}
    </div>
  {:else if error}
    <p class="error">{error}</p>
  {:else if vehicleList.length === 0}
    <div class="empty">
      <div class="empty-icon">⬡</div>
      <h2>Your garage is empty</h2>
      <p>Add your first vehicle to start tracking maintenance.</p>
      <a href="/vehicles/new" use:link class="btn btn-primary">Add your first car</a>
    </div>
  {:else}
    <div class="vehicle-grid">
      {#each vehicleList as vehicle, i (vehicle.id)}
        {@const summary = statusSummary(reminderMap.get(vehicle.id))}
        {@const est = reminderMap.get(vehicle.id)?.estimated_mileage}
        <a
          href="/vehicles/{vehicle.id}"
          use:link
          class="vehicle-card"
          style="--delay: {i * 60}ms"
        >
          <div class="card-photo">
            {#if vehicle.photo_path}
              <img src="/files/{vehicle.photo_path}" alt={vehicle.name} />
            {:else}
              <div class="placeholder-photo">
                <svg width="48" height="48" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                  <path d="M7 17m-2 0a2 2 0 1 0 4 0a2 2 0 1 0 -4 0" />
                  <path d="M17 17m-2 0a2 2 0 1 0 4 0a2 2 0 1 0 -4 0" />
                  <path d="M5 17H3v-6l2-5h9l4 5h1a2 2 0 0 1 2 2v4h-2m-4 0H9" />
                  <path d="M10 6l-1 5h-4" />
                </svg>
              </div>
            {/if}
            <div class="card-photo-overlay"></div>
          </div>
          <div class="card-body">
            <div class="card-info">
              <h2>{vehicle.name}</h2>
              {#if vehicle.year || vehicle.make || vehicle.model}
                <p class="subtitle">
                  {vehicle.year ?? ''} {vehicle.make ?? ''} {vehicle.model ?? ''}
                </p>
              {/if}
            </div>
            <div class="card-footer">
              {#if est}
                <span class="mileage">{formatMileage(est)} mi</span>
              {/if}
              <div class="status-badges">
                {#if summary.overdue > 0}
                  <span class="badge badge-danger">{summary.overdue} overdue</span>
                {/if}
                {#if summary.upcoming > 0}
                  <span class="badge badge-warning">{summary.upcoming} upcoming</span>
                {/if}
                {#if summary.overdue === 0 && summary.upcoming === 0}
                  <span class="badge badge-success">All good</span>
                {/if}
              </div>
            </div>
          </div>
        </a>
      {/each}
    </div>
  {/if}
</div>

<style>
  .garage-header {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    margin-bottom: var(--sp-6);
  }

  .garage-header h1 {
    margin: 0;
  }

  .garage-count {
    font-size: 0.85rem;
    color: var(--text-muted);
    margin: var(--sp-1) 0 0;
  }

  /* --- Grid --- */
  .vehicle-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
    gap: var(--sp-4);
  }

  /* --- Card --- */
  .vehicle-card {
    background: var(--bg-raised);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-lg);
    overflow: hidden;
    text-decoration: none;
    color: var(--text);
    transition:
      border-color var(--duration-base) var(--ease-out),
      box-shadow var(--duration-base) var(--ease-out),
      transform var(--duration-base) var(--ease-out);
    animation: card-enter var(--duration-slow) var(--ease-out) both;
    animation-delay: var(--delay, 0ms);
  }

  .vehicle-card:hover {
    border-color: var(--primary);
    box-shadow: var(--shadow-md), 0 0 0 1px var(--primary-muted);
    transform: translateY(-2px);
  }

  /* --- Photo area --- */
  .card-photo {
    position: relative;
    height: 150px;
    background: var(--surface);
    display: flex;
    align-items: center;
    justify-content: center;
    overflow: hidden;
  }

  .card-photo img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    transition: transform var(--duration-slow) var(--ease-out);
  }

  .vehicle-card:hover .card-photo img {
    transform: scale(1.05);
  }

  .card-photo-overlay {
    position: absolute;
    inset: 0;
    background: linear-gradient(
      to top,
      var(--bg-raised) 0%,
      transparent 50%
    );
    pointer-events: none;
  }

  .placeholder-photo {
    color: var(--text-muted);
    opacity: 0.3;
    transition: opacity var(--duration-base) var(--ease-out);
  }

  .vehicle-card:hover .placeholder-photo {
    opacity: 0.5;
  }

  /* --- Card body --- */
  .card-body {
    padding: var(--sp-3) var(--sp-4) var(--sp-4);
  }

  .card-info h2 {
    margin: 0;
    font-family: var(--font-display);
    font-size: 1.05rem;
    font-weight: 600;
    letter-spacing: -0.01em;
    line-height: 1.3;
  }

  .subtitle {
    font-size: 0.8rem;
    color: var(--text-muted);
    margin: var(--sp-1) 0 0;
  }

  .card-footer {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-top: var(--sp-3);
    padding-top: var(--sp-3);
    border-top: 1px solid var(--border-subtle);
  }

  .mileage {
    font-family: var(--font-display);
    font-size: 0.85rem;
    font-weight: 600;
    color: var(--text-secondary);
    letter-spacing: -0.01em;
  }

  .status-badges {
    display: flex;
    gap: var(--sp-2);
    flex-wrap: wrap;
  }

  /* --- Empty state --- */
  .empty {
    text-align: center;
    padding: var(--sp-12) 0;
    color: var(--text-muted);
  }

  .empty-icon {
    font-size: 3rem;
    color: var(--primary);
    opacity: 0.3;
    margin-bottom: var(--sp-4);
  }

  .empty h2 {
    font-family: var(--font-display);
    color: var(--text-secondary);
    margin-bottom: var(--sp-2);
  }

  .empty p {
    margin-bottom: var(--sp-6);
    font-size: 0.9rem;
  }

  /* --- Skeleton loading --- */
  .skeleton-card-wrap {
    pointer-events: none;
    animation: fade-in var(--duration-slow) var(--ease-out) both;
    animation-delay: var(--delay, 0ms);
  }

  .skeleton-card-wrap .card-body {
    padding: var(--sp-4);
  }

  .error {
    color: var(--danger);
    padding: var(--sp-4);
    background: var(--danger-bg);
    border: 1px solid var(--danger-border);
    border-radius: var(--radius-md);
  }

  /* --- Animations --- */
  @keyframes card-enter {
    from {
      opacity: 0;
      transform: translateY(12px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .vehicle-card, .skeleton-card-wrap {
      animation: none;
    }
  }
</style>
