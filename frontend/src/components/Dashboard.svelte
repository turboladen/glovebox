<script lang="ts">
  import { onMount } from 'svelte'
  import { link } from '@keenmate/svelte-spa-router'
  import { dashboard as dashboardApi, workItems as workItemsApi } from '../lib/api'
  import { garageDashboard, refreshDashboard } from '../lib/stores'
  import type { ActivityItem, AttentionItem } from '../lib/types'
  import { formatDate } from '../lib/dates'

  // ONE component, two scopes (decision ⑥: "the same dashboard scoped").
  // No vehicleId → garage-wide landing; with vehicleId → the vehicle's
  // Overview tab, filtering the same /api/dashboard snapshot to one car.
  let { vehicleId }: { vehicleId?: number } = $props()

  const ACTIVITY_LIMIT = 8

  let activity: ActivityItem[] = $state([])
  let loading = $state(true)
  let error = $state('')

  let dash = $derived($garageDashboard)
  let garageScope = $derived(vehicleId == null)

  let vehicles = $derived(
    (dash?.vehicles ?? []).filter((s) =>
      garageScope ? !s.vehicle.archived_at : s.vehicle.id === vehicleId,
    ),
  )
  let attention = $derived(
    (dash?.attention ?? []).filter((a) => garageScope || a.vehicle_id === vehicleId),
  )
  let upcomingVisits = $derived(
    (dash?.upcoming_visits ?? []).filter((v) => garageScope || v.vehicle_id === vehicleId),
  )
  let activeBuilds = $derived(
    (dash?.active_builds ?? []).filter((b) => garageScope || b.vehicle_id === vehicleId),
  )
  let unscheduledCount = $derived(
    vehicles.reduce((n, s) => n + s.unscheduled_work_count, 0),
  )
  let forecastTotal = $derived(
    garageScope
      ? (dash?.budget_total_cents ?? 0)
      : (vehicles[0]?.forecast_total_cents ?? 0),
  )
  let emptyGarage = $derived(garageScope && dash != null && dash.vehicles.length === 0)

  onMount(async () => {
    try {
      const [, feed] = await Promise.all([
        refreshDashboard(),
        vehicleId == null
          ? dashboardApi.activity(ACTIVITY_LIMIT)
          : dashboardApi.vehicleActivity(vehicleId, ACTIVITY_LIMIT),
      ])
      activity = feed
    } catch (e: any) {
      error = e.message
    } finally {
      loading = false
    }
  })

  function attentionTarget(a: AttentionItem): string {
    return `/vehicles/${a.vehicle_id}/${a.deep_link_hint}`
  }

  // "Plan it" quick action: put the attention row on the to-do list with
  // its source linked, so completing the work later closes the loop.
  let planningKey: string | null = $state(null)

  function rowKey(a: AttentionItem): string {
    return `${a.kind}-${a.vehicle_id}-${a.entity_id}`
  }

  function canPlan(a: AttentionItem): boolean {
    return a.kind === 'overdue' || a.kind === 'due_soon' || a.kind === 'recall'
  }

  async function planIt(a: AttentionItem) {
    planningKey = rowKey(a)
    try {
      // Labels are built as "Name — status detail"; the name is the title.
      const title = a.label.split(' — ')[0]
      await workItemsApi.create(a.vehicle_id, {
        title,
        schedule_item_id: a.kind === 'recall' ? undefined : a.entity_id,
        research_finding_id: a.kind === 'recall' ? a.entity_id : undefined,
      })
      await refreshDashboard()
    } catch (e: any) {
      error = e.message
    } finally {
      planningKey = null
    }
  }

  function formatDollars(cents: number): string {
    return `$${(cents / 100).toLocaleString(undefined, { maximumFractionDigits: 0 })}`
  }

  function formatCents(cents: number | null): string {
    if (cents == null) return ''
    return `$${(cents / 100).toFixed(2)}`
  }
</script>

{#if loading}
  <div class="skeleton skeleton-card"></div>
  <div class="skeleton skeleton-card"></div>
{:else if error}
  <p class="error">{error}</p>
{:else if emptyGarage}
  <!-- Welcome / setup state, ported from the retired garage-cards view -->
  <div class="welcome">
    <div class="welcome-hero">
      <h2>Welcome to Glovebox</h2>
      <p class="welcome-subtitle">Your precision maintenance tracker</p>
      <div class="welcome-accent"></div>
      <a href="/vehicles/new" use:link class="btn btn-primary welcome-cta">Add Your First Vehicle</a>
    </div>
    <div class="setup-card">
      <h3 class="setup-heading">Get Started</h3>
      <div class="setup-checklist">
        <a href="/vehicles/new" use:link class="setup-step">
          <span class="step-indicator"></span>
          <span class="step-label">Add your first vehicle</span>
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
  <div class="dashboard" data-testid="dashboard">
    {#if garageScope}
      <h1 class="dash-title">Garage</h1>
    {/if}

    {#if attention.length > 0}
      <section class="block attention-block" data-testid="attention-block">
        <h3 class="block-title attention-title">⚠ Needs attention ({attention.length})</h3>
        <div class="block-rows">
          {#each attention as a (rowKey(a))}
            <div class="row attention-row">
              <a href={attentionTarget(a)} use:link class="row-link">
                {#if garageScope}<span class="row-vehicle">{a.vehicle_name}</span> ·{/if}
                {a.label}
              </a>
              {#if canPlan(a)}
                {#if a.planned}
                  <span class="planned-chip">planned</span>
                {:else}
                  <button
                    class="plan-it"
                    onclick={() => planIt(a)}
                    disabled={planningKey === rowKey(a)}
                  >
                    {planningKey === rowKey(a) ? 'planning…' : 'plan it'}
                  </button>
                {/if}
              {/if}
            </div>
          {/each}
        </div>
      </section>
    {/if}

    <div class="block-pair">
      <section class="block plan-block" data-testid="plan-budget-block">
        <h3 class="block-title plan-title">Plan &amp; budget</h3>
        <div class="block-rows">
          {#each upcomingVisits as v (v.id)}
            <div class="row">
              <a href="/vehicles/{v.vehicle_id}/plan/visits" use:link class="row-link">
                {#if garageScope}<span class="row-vehicle">{v.vehicle_name}</span> ·{/if}
                Visit {v.planned_date ? formatDate(v.planned_date) : 'unscheduled'}
                {#if v.shop_name}· {v.shop_name}{/if}
                {#if v.est_total_cents > 0}· ~{formatDollars(v.est_total_cents)}{/if}
              </a>
            </div>
          {/each}
          {#if upcomingVisits.length === 0}
            <p class="row muted">No visits planned.</p>
          {/if}
          {#if unscheduledCount > 0}
            <p class="row muted">
              {unscheduledCount} unscheduled to-do item{unscheduledCount === 1 ? '' : 's'}
            </p>
          {/if}
          <p class="row forecast-line">
            12-mo forecast <strong>{formatDollars(forecastTotal)}</strong>
          </p>
        </div>
      </section>

      {#if activeBuilds.length > 0}
        <section class="block builds-block" data-testid="builds-block">
          <h3 class="block-title builds-title">Builds</h3>
          <div class="block-rows">
            {#each activeBuilds as b (b.id)}
              <div class="row">
                <a href="/vehicles/{b.vehicle_id}/builds" use:link class="row-link">
                  {#if garageScope}<span class="row-vehicle">{b.vehicle_name}</span> ·{/if}
                  {b.name} · {b.parts_installed}/{b.parts_total} parts
                  {#if b.total_cost_cents > 0}· {formatDollars(b.total_cost_cents)}{/if}
                </a>
              </div>
            {/each}
          </div>
        </section>
      {/if}
    </div>

    <section class="block activity-block" data-testid="activity-block">
      <h3 class="block-title">Recent activity</h3>
      <div class="block-rows">
        {#each activity as item (item.kind + '-' + item.id)}
          <div class="row">
            <a href="/vehicles/{item.vehicle_id}/timeline" use:link class="row-link">
              <span class="activity-date">{formatDate(item.date)}</span>
              {#if garageScope}· <span class="row-vehicle">{item.vehicle_name}</span>{/if}
              · {item.summary}
              {#if item.total_cost_cents}· {formatCents(item.total_cost_cents)}{/if}
            </a>
          </div>
        {/each}
        {#if activity.length === 0}
          <p class="row muted">No activity yet.</p>
        {/if}
      </div>
    </section>
  </div>
{/if}

<style>
  .dash-title {
    margin-bottom: var(--sp-4);
  }

  .dashboard {
    display: flex;
    flex-direction: column;
    gap: var(--sp-4);
  }

  .block {
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-lg);
    padding: var(--sp-3) var(--sp-4);
    background: var(--bg-raised);
  }

  .block-title {
    font-family: var(--font-display);
    font-size: 0.85rem;
    font-weight: 600;
    margin: 0 0 var(--sp-2);
    text-transform: none;
    letter-spacing: 0;
  }

  .block-rows {
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
  }

  .row {
    font-size: 0.85rem;
    margin: 0;
  }

  .row-link {
    color: var(--text-secondary);
    text-decoration: none;
  }

  .row-link:hover {
    color: var(--text);
    text-decoration: underline;
  }

  .row-vehicle {
    font-weight: 600;
    color: var(--text);
  }

  .muted {
    color: var(--text-muted);
  }

  /* Mockup block tints, translated into the house semantic tokens:
     attention = danger, plan & budget = info, builds = success. */
  .attention-block {
    background: var(--danger-bg);
    border-color: var(--danger-border);
  }

  .attention-title {
    color: var(--danger);
  }

  .attention-row {
    display: flex;
    align-items: baseline;
    gap: var(--sp-2);
  }

  .plan-it {
    padding: 0;
    border: none;
    background: none;
    font-size: 0.78rem;
    font-weight: 500;
    color: var(--primary);
    cursor: pointer;
    text-decoration: underline;
    white-space: nowrap;
  }

  .plan-it:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .planned-chip {
    font-size: 0.65rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    padding: 0 var(--sp-1);
    border-radius: var(--radius-sm);
    background: var(--success-bg);
    color: var(--success);
    border: 1px solid var(--success-border);
  }

  .plan-block {
    background: var(--info-bg);
    border-color: var(--border-subtle);
  }

  .plan-title {
    color: var(--info);
  }

  .builds-block {
    background: var(--success-bg);
    border-color: var(--success-border);
  }

  .builds-title {
    color: var(--success);
  }

  .block-pair {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--sp-4);
  }

  .block-pair > .block:only-child {
    grid-column: 1 / -1;
  }

  .forecast-line {
    color: var(--text-secondary);
  }

  .activity-date {
    font-weight: 600;
    color: var(--text);
  }

  .error {
    color: var(--danger);
    padding: var(--sp-4);
    background: var(--danger-bg);
    border: 1px solid var(--danger-border);
    border-radius: var(--radius-md);
  }

  /* --- Welcome / empty state (ported from Garage.svelte) --- */
  .welcome {
    display: flex;
    flex-direction: column;
    gap: var(--sp-8);
  }

  .welcome-hero {
    text-align: center;
    padding: var(--sp-10) 0 var(--sp-6);
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

  .setup-card {
    background: var(--bg-raised);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-lg);
    padding: var(--sp-5) var(--sp-6);
  }

  .setup-heading {
    font-family: var(--font-display);
    font-size: 0.9rem;
    font-weight: 600;
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

  @media (max-width: 720px) {
    .block-pair {
      grid-template-columns: 1fr;
    }
  }
</style>
