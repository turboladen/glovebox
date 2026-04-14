<script lang="ts">
  import { onMount } from 'svelte'
  import { link } from '@keenmate/svelte-spa-router'
  import { vehicles as vehiclesApi, reminders as remindersApi, research } from '../lib/api'
  import type { Vehicle, RemindersResponse } from '../lib/types'

  let vehicleList: Vehicle[] = $state([])
  let reminderMap: Map<number, RemindersResponse> = $state(new Map())
  let plannedCountMap: Map<number, number> = $state(new Map())
  let loading = $state(true)
  let error = $state('')
  let showArchived = $state(false)

  let activeVehicles = $derived(vehicleList.filter(v => !v.archived_at))
  let archivedVehicles = $derived(vehicleList.filter(v => v.archived_at))
  let isArchivedOpen = $derived(showArchived || activeVehicles.length === 0)

  onMount(async () => {
    try {
      vehicleList = await vehiclesApi.list()
      // Fetch reminders and planned findings counts for active vehicles only
      const active = vehicleList.filter(v => !v.archived_at)
      const [reminderResults, plannedResults] = await Promise.all([
        Promise.allSettled(active.map((v) => remindersApi.get(v.id))),
        Promise.allSettled(active.map((v) => research.listFindings(v.id, 'planned'))),
      ])
      for (let i = 0; i < reminderResults.length; i++) {
        const result = reminderResults[i]
        if (result.status === 'fulfilled') {
          reminderMap.set(active[i].id, result.value)
        }
      }
      for (let i = 0; i < plannedResults.length; i++) {
        const result = plannedResults[i]
        if (result.status === 'fulfilled') {
          plannedCountMap.set(active[i].id, result.value.length)
        }
      }
      reminderMap = reminderMap // trigger reactivity
      plannedCountMap = plannedCountMap
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
  {#if loading || vehicleList.length > 0}
    <div class="garage-header">
      <div>
        <h1>Garage</h1>
        {#if !loading && activeVehicles.length > 0}
          <p class="garage-count">{activeVehicles.length} vehicle{activeVehicles.length !== 1 ? 's' : ''}</p>
        {/if}
      </div>
      <a href="/vehicles/new" use:link class="btn btn-primary">+ Add Car</a>
    </div>
  {/if}

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
    <div class="welcome">
      <!-- Hero Banner -->
      <div class="welcome-hero" style="--delay: 0ms">
        <h2>Welcome to Glovebox</h2>
        <p class="welcome-subtitle">Your precision maintenance tracker</p>
        <div class="welcome-accent"></div>
        <a href="/vehicles/new" use:link class="btn btn-primary welcome-cta">Add Your First Vehicle</a>
      </div>

      <!-- Feature Showcase -->
      <div class="feature-grid">
        <div class="feature-card" style="--delay: 80ms">
          <div class="feature-icon">
            <svg width="32" height="32" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
              <path d="M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94l-3.76 3.76z" />
            </svg>
          </div>
          <h3>Track Maintenance</h3>
          <p>Log services, track costs, and never miss scheduled maintenance.</p>
          <div class="feature-tags">
            <span class="tag">Services</span>
            <span class="tag">Schedules</span>
            <span class="tag">Costs</span>
          </div>
        </div>

        <div class="feature-card" style="--delay: 160ms">
          <div class="feature-icon">
            <svg width="32" height="32" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
              <path d="M12 3l1.912 5.813a2 2 0 0 0 1.275 1.275L21 12l-5.813 1.912a2 2 0 0 0-1.275 1.275L12 21l-1.912-5.813a2 2 0 0 0-1.275-1.275L3 12l5.813-1.912a2 2 0 0 0 1.275-1.275L12 3z" />
            </svg>
          </div>
          <h3>AI-Powered Insights</h3>
          <p>Chat about your vehicle, get suggestions, and auto-parse invoices.</p>
          <div class="feature-tags">
            <span class="tag">Chat</span>
            <span class="tag">Suggestions</span>
            <span class="tag">Research</span>
          </div>
        </div>

        <div class="feature-card" style="--delay: 240ms">
          <div class="feature-icon">
            <svg width="32" height="32" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
              <path d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7z" />
              <path d="M14 2v4a2 2 0 0 0 2 2h4" />
              <path d="M10 13H8" />
              <path d="M16 17H8" />
              <path d="M16 13h-2" />
            </svg>
          </div>
          <h3>Complete History</h3>
          <p>Documents, observations, accidents, and parts &mdash; all in one place.</p>
          <div class="feature-tags">
            <span class="tag">Documents</span>
            <span class="tag">Parts</span>
            <span class="tag">Observations</span>
          </div>
        </div>
      </div>

      <!-- Quick Start Checklist -->
      <div class="setup-card" style="--delay: 320ms">
        <h3 class="setup-heading">Get Started</h3>
        <div class="setup-checklist">
          <a href="/vehicles/new" use:link class="setup-step">
            <span class="step-indicator"></span>
            <span class="step-label">Add your first vehicle</span>
          </a>
          <a href="/settings" use:link class="setup-step">
            <span class="step-indicator"></span>
            <span class="step-label">Configure an AI provider</span>
            <span class="tag tag-muted">Optional</span>
          </a>
          <div class="setup-step disabled">
            <span class="step-indicator"></span>
            <span class="step-label">Add a trusted shop</span>
          </div>
          <div class="setup-step disabled">
            <span class="step-indicator"></span>
            <span class="step-label">Log your first service</span>
          </div>
        </div>
      </div>
    </div>
  {:else}
    {#if activeVehicles.length === 0 && archivedVehicles.length > 0}
      <p class="all-archived-msg">All vehicles are archived.</p>
    {/if}
    <div class="vehicle-grid">
      {#each activeVehicles as vehicle, i (vehicle.id)}
        {@const summary = statusSummary(reminderMap.get(vehicle.id))}
        {@const est = reminderMap.get(vehicle.id)?.estimated_mileage}
        {@const planned = plannedCountMap.get(vehicle.id) ?? 0}
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
                {#if planned > 0}
                  <span class="badge badge-planned">{planned} planned</span>
                {/if}
                {#if summary.overdue === 0 && summary.upcoming === 0 && planned === 0}
                  <span class="badge badge-success">All good</span>
                {/if}
              </div>
            </div>
          </div>
        </a>
      {/each}
    </div>

    {#if archivedVehicles.length > 0}
      <div class="archived-section">
        <button class="archived-header" onclick={() => (showArchived = !showArchived)}>
          <span class="archived-chevron" class:open={isArchivedOpen}>&#9654;</span>
          Archived ({archivedVehicles.length})
        </button>
        {#if isArchivedOpen}
          <div class="vehicle-grid">
            {#each archivedVehicles as vehicle (vehicle.id)}
              <a
                href="/vehicles/{vehicle.id}"
                use:link
                class="vehicle-card archived-card"
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
                </div>
              </a>
            {/each}
          </div>
        {/if}
      </div>
    {/if}
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

  /* --- Welcome Dashboard --- */
  .welcome {
    display: flex;
    flex-direction: column;
    gap: var(--sp-8);
  }

  .welcome-hero {
    text-align: center;
    padding: var(--sp-10) 0 var(--sp-6);
    animation: card-enter var(--duration-slow) var(--ease-out) both;
    animation-delay: var(--delay, 0ms);
  }

  .welcome-hero h2 {
    font-family: var(--font-display);
    font-size: 1.8rem;
    font-weight: 700;
    letter-spacing: -0.02em;
    color: var(--text);
    margin: 0 0 var(--sp-2);
  }

  .welcome-subtitle {
    font-size: 1rem;
    color: var(--text-muted);
    margin: 0 0 var(--sp-6);
  }

  .welcome-accent {
    width: 60px;
    height: 2px;
    background: var(--primary);
    margin: 0 auto var(--sp-6);
    border-radius: 1px;
  }

  .welcome-cta {
    font-size: 0.95rem;
    padding: var(--sp-3) var(--sp-6);
  }

  /* --- Feature Grid --- */
  .feature-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(240px, 1fr));
    gap: var(--sp-4);
  }

  .feature-card {
    background: var(--bg-raised);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-lg);
    padding: var(--sp-6) var(--sp-5);
    transition:
      border-color var(--duration-base) var(--ease-out),
      box-shadow var(--duration-base) var(--ease-out),
      transform var(--duration-base) var(--ease-out);
    animation: card-enter var(--duration-slow) var(--ease-out) both;
    animation-delay: var(--delay, 0ms);
  }

  .feature-card:hover {
    border-color: var(--primary);
    box-shadow: var(--shadow-md), 0 0 0 1px var(--primary-muted);
    transform: translateY(-2px);
  }

  .feature-icon {
    color: var(--primary);
    opacity: 0.7;
    margin-bottom: var(--sp-3);
  }

  .feature-card h3 {
    font-family: var(--font-display);
    font-size: 1rem;
    font-weight: 600;
    letter-spacing: -0.01em;
    margin: 0 0 var(--sp-2);
    color: var(--text);
  }

  .feature-card p {
    font-size: 0.85rem;
    color: var(--text-secondary);
    line-height: 1.5;
    margin: 0 0 var(--sp-4);
  }

  .feature-tags {
    display: flex;
    gap: var(--sp-2);
    flex-wrap: wrap;
  }

  .tag {
    font-size: 0.7rem;
    font-weight: 500;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    padding: 2px var(--sp-2);
    border-radius: var(--radius-sm);
    background: var(--surface);
    color: var(--text-muted);
  }

  .tag-muted {
    background: transparent;
    border: 1px solid var(--border);
    color: var(--text-muted);
  }

  /* --- Setup Checklist --- */
  .setup-card {
    background: var(--bg-raised);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-lg);
    padding: var(--sp-5) var(--sp-6);
    animation: card-enter var(--duration-slow) var(--ease-out) both;
    animation-delay: var(--delay, 0ms);
  }

  .setup-heading {
    font-family: var(--font-display);
    font-size: 0.9rem;
    font-weight: 600;
    letter-spacing: -0.01em;
    color: var(--text-secondary);
    margin: 0 0 var(--sp-4);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .setup-checklist {
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
  }

  .setup-step {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    padding: var(--sp-2) var(--sp-3);
    border-radius: var(--radius-md);
    text-decoration: none;
    color: var(--text);
    font-size: 0.9rem;
    transition:
      background var(--duration-fast) var(--ease-out),
      color var(--duration-fast) var(--ease-out);
  }

  a.setup-step:hover {
    background: var(--surface);
    color: var(--primary);
  }

  .setup-step.disabled {
    opacity: 0.35;
    pointer-events: none;
  }

  .step-indicator {
    width: 16px;
    height: 16px;
    border-radius: 50%;
    border: 2px solid var(--border);
    flex-shrink: 0;
    transition: border-color var(--duration-fast) var(--ease-out);
  }

  a.setup-step:hover .step-indicator {
    border-color: var(--primary);
  }

  .step-label {
    flex: 1;
  }

  .all-archived-msg {
    color: var(--text-muted);
    font-size: 0.9rem;
    margin-bottom: var(--sp-2);
  }

  /* --- Archived section --- */
  .archived-section {
    margin-top: var(--sp-8);
  }

  .archived-header {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    background: none;
    border: none;
    color: var(--text-muted);
    font-size: 0.9rem;
    font-weight: 600;
    cursor: pointer;
    padding: var(--sp-2) 0;
    margin-bottom: var(--sp-4);
    transition: color var(--duration-fast) var(--ease-out);
  }

  .archived-header:hover {
    color: var(--text);
  }

  .archived-chevron {
    font-size: 0.65rem;
    transition: transform var(--duration-fast) var(--ease-out);
  }

  .archived-chevron.open {
    transform: rotate(90deg);
  }

  .archived-card {
    opacity: 0.6;
  }

  .archived-card:hover {
    opacity: 1;
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
    .vehicle-card, .skeleton-card-wrap, .welcome-hero, .feature-card, .setup-card {
      animation: none;
    }
  }

  @media (max-width: 640px) {
    .welcome-hero h2 {
      font-size: 1.4rem;
    }

    .feature-grid {
      grid-template-columns: 1fr;
    }
  }
</style>
