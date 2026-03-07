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
    <h1>Garage</h1>
    <a href="/vehicles/new" use:link class="btn btn-primary">+ Add Car</a>
  </div>

  {#if loading}
    <p class="loading">Loading...</p>
  {:else if error}
    <p class="error">{error}</p>
  {:else if vehicleList.length === 0}
    <div class="empty">
      <p>No vehicles yet.</p>
      <a href="/vehicles/new" use:link class="btn btn-primary">Add your first car</a>
    </div>
  {:else}
    <div class="vehicle-grid">
      {#each vehicleList as vehicle (vehicle.id)}
        {@const summary = statusSummary(reminderMap.get(vehicle.id))}
        {@const est = reminderMap.get(vehicle.id)?.estimated_mileage}
        <a href="/vehicles/{vehicle.id}" use:link class="vehicle-card">
          <div class="card-photo">
            {#if vehicle.photo_path}
              <img src="/files/{vehicle.photo_path}" alt={vehicle.name} />
            {:else}
              <div class="placeholder-photo">🚗</div>
            {/if}
          </div>
          <div class="card-info">
            <h2>{vehicle.name}</h2>
            <p class="subtitle">
              {vehicle.year ?? ''} {vehicle.make ?? ''} {vehicle.model ?? ''}
            </p>
            {#if est}
              <p class="mileage">~{formatMileage(est)} mi</p>
            {/if}
            <div class="status-badges">
              {#if summary.overdue > 0}
                <span class="badge badge-overdue">{summary.overdue} overdue</span>
              {/if}
              {#if summary.upcoming > 0}
                <span class="badge badge-upcoming">{summary.upcoming} upcoming</span>
              {/if}
              {#if summary.overdue === 0 && summary.upcoming === 0}
                <span class="badge badge-ok">All good</span>
              {/if}
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
    align-items: center;
    margin-bottom: 1.5rem;
  }

  .vehicle-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
    gap: 1rem;
  }

  .vehicle-card {
    border: 1px solid var(--border);
    border-radius: 8px;
    overflow: hidden;
    text-decoration: none;
    color: var(--text);
    transition: box-shadow 0.15s;
  }

  .vehicle-card:hover {
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.12);
  }

  .card-photo {
    height: 140px;
    background: var(--surface);
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .card-photo img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .placeholder-photo {
    font-size: 3rem;
    opacity: 0.3;
  }

  .card-info {
    padding: 0.75rem 1rem;
  }

  .card-info h2 {
    margin: 0;
    font-size: 1.1rem;
  }

  .subtitle {
    font-size: 0.85rem;
    color: var(--text-muted);
    margin: 0.25rem 0;
  }

  .mileage {
    font-size: 0.85rem;
    color: var(--text-muted);
    margin: 0.25rem 0 0.5rem;
  }

  .status-badges {
    display: flex;
    gap: 0.5rem;
    flex-wrap: wrap;
  }

  .badge {
    font-size: 0.75rem;
    padding: 0.15rem 0.5rem;
    border-radius: 4px;
    font-weight: 500;
  }

  .badge-overdue {
    background: var(--danger-bg);
    color: var(--danger);
  }

  .badge-upcoming {
    background: var(--warning-bg);
    color: var(--warning);
  }

  .badge-ok {
    background: var(--success-bg);
    color: var(--success);
  }

  .empty {
    text-align: center;
    padding: 3rem 0;
    color: var(--text-muted);
  }

  .loading {
    color: var(--text-muted);
  }

  .error {
    color: var(--danger);
  }
</style>
