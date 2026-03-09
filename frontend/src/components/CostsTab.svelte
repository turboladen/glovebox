<script lang="ts">
  import { onMount } from 'svelte'
  import { costs as costsApi } from '../lib/api'
  import type { CostSummary } from '../lib/types'
  import { formatMonth } from '../lib/dates'

  let { vehicleId }: { vehicleId: number } = $props()

  let data: CostSummary | null = $state(null)
  let loading = $state(true)

  onMount(async () => {
    try {
      data = await costsApi.get(vehicleId)
    } catch (e) {
      console.error(e)
    } finally {
      loading = false
    }
  })

  function fmt(cents: number): string {
    return `$${(cents / 100).toFixed(2)}`
  }

  function fmtLong(cents: number | null): string {
    if (cents === null) return 'N/A'
    return `$${(cents / 100).toFixed(2)}`
  }
</script>

<div class="costs-tab">
  <h3>Cost of Ownership</h3>

  {#if loading}
    <p>Loading cost data...</p>
  {:else if !data}
    <p class="empty">Could not load cost data.</p>
  {:else if data.total_cost_cents === 0 && data.part_count === 0}
    <p class="empty">No cost data yet. Log services or add parts to see ownership costs.</p>
  {:else}
    <div class="summary-grid">
      <div class="summary-card">
        <span class="card-label">Total Spent</span>
        <span class="card-value">{fmt(data.total_cost_cents)}</span>
      </div>
      <div class="summary-card">
        <span class="card-label">Services</span>
        <span class="card-value">{fmt(data.total_service_cost_cents)}</span>
        <span class="card-sub">{data.service_count} service{data.service_count !== 1 ? 's' : ''}</span>
      </div>
      <div class="summary-card">
        <span class="card-label">Parts</span>
        <span class="card-value">{fmt(data.total_parts_cost_cents)}</span>
        <span class="card-sub">{data.part_count} part{data.part_count !== 1 ? 's' : ''}</span>
      </div>
      <div class="summary-card">
        <span class="card-label">Labor</span>
        <span class="card-value">{fmt(data.total_labor_cost_cents)}</span>
      </div>
      {#if data.cost_per_mile_cents !== null}
        <div class="summary-card">
          <span class="card-label">Cost per Mile</span>
          <span class="card-value">{fmtLong(data.cost_per_mile_cents)}</span>
        </div>
      {/if}
    </div>

    {#if data.monthly_costs.length > 0}
      <h4>Monthly Breakdown</h4>
      <table class="cost-table">
        <thead>
          <tr>
            <th>Month</th>
            <th>Services</th>
            <th>Parts</th>
            <th>Total</th>
          </tr>
        </thead>
        <tbody>
          {#each data.monthly_costs as mc (mc.month)}
            <tr>
              <td>{formatMonth(mc.month)}</td>
              <td>{fmt(mc.service_cost_cents)}</td>
              <td>{fmt(mc.parts_cost_cents)}</td>
              <td class="total">{fmt(mc.total_cents)}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
  {/if}
</div>

<style>
  .costs-tab h3 { margin: 0 0 var(--sp-4); }
  .costs-tab h4 { margin: var(--sp-6) 0 var(--sp-2); font-family: var(--font-display); }

  .summary-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
    gap: var(--sp-3);
    margin-bottom: var(--sp-4);
  }

  .summary-card {
    padding: var(--sp-3) var(--sp-4);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    background: var(--bg-raised);
    display: flex;
    flex-direction: column;
    transition: border-color var(--duration-base) var(--ease-out);
  }

  .summary-card:hover {
    border-color: var(--border);
  }

  .card-label {
    font-family: var(--font-display);
    font-size: 0.8rem;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.03em;
  }

  .card-value {
    font-family: var(--font-display);
    font-size: 1.3rem;
    font-weight: 600;
    margin-top: var(--sp-1);
  }

  .card-sub {
    font-size: 0.8rem;
    color: var(--text-muted);
  }

  .cost-table {
    width: 100%;
    border-collapse: collapse;
    font-size: 0.9rem;
  }

  .cost-table th, .cost-table td {
    padding: var(--sp-2) var(--sp-3);
    text-align: left;
    border-bottom: 1px solid var(--border-subtle);
  }

  .cost-table th {
    font-family: var(--font-display);
    font-weight: 600;
    font-size: 0.8rem;
    text-transform: uppercase;
    color: var(--text-muted);
  }

  .cost-table .total { font-weight: 600; }

  .empty { color: var(--text-muted); text-align: center; padding: var(--sp-8) 0; }
</style>
