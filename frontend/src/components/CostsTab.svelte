<script lang="ts">
  import { onMount } from 'svelte'
  import { costs as costsApi, budget as budgetApi } from '../lib/api'
  import { formatCents } from '../lib/money'
  import type { BudgetForecast, CostSummary } from '../lib/types'
  import { formatMonth } from '../lib/dates'

  let { vehicleId }: { vehicleId: number } = $props()

  let data: CostSummary | null = $state(null)
  let forecast: BudgetForecast | null = $state(null)
  let loading = $state(true)

  onMount(async () => {
    try {
      const [c, f] = await Promise.all([costsApi.get(vehicleId), budgetApi.get(vehicleId)])
      data = c
      forecast = f
    } catch (e) {
      console.error(e)
    } finally {
      loading = false
    }
  })

  const fmt = formatCents

  function fmtLong(cents: number | null): string {
    if (cents === null) return 'N/A'
    return formatCents(cents)
  }
</script>

<div class="costs-tab">
  <h3>Cost of Ownership</h3>

  {#if loading}
    <p>Loading cost data...</p>
  {:else if !data}
    <p class="empty">Could not load cost data.</p>
  {:else if data.total_cost_cents === 0 && data.part_count === 0}
    <p class="empty">No cost data yet. Record services or add parts to see ownership costs.</p>
  {:else}
    <div class="summary-grid">
      <div class="summary-card">
        <span class="card-label">Total Spent</span>
        <span class="card-value">{fmt(data.total_cost_cents)}</span>
      </div>
      <div class="summary-card">
        <span class="card-label">Out of Pocket</span>
        <span class="card-value">{fmt(data.out_of_pocket_cents)}</span>
      </div>
      {#if data.covered_cents > 0}
        <div class="summary-card">
          <span class="card-label">Covered by Others</span>
          <span class="card-value">{fmt(data.covered_cents)}</span>
          <span class="card-sub">insurance / third party</span>
        </div>
      {/if}
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
            <th>Out of Pocket</th>
            <th>Covered</th>
            <th>Total</th>
          </tr>
        </thead>
        <tbody>
          {#each data.monthly_costs as mc (mc.month)}
            <tr>
              <td>{formatMonth(mc.month)}</td>
              <td>{fmt(mc.service_cost_cents)}</td>
              <td>{fmt(mc.parts_cost_cents)}</td>
              <td>{fmt(mc.out_of_pocket_cents)}</td>
              <td>{mc.covered_cents > 0 ? fmt(mc.covered_cents) : '—'}</td>
              <td class="total">{fmt(mc.total_cents)}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
  {/if}

  <!-- The 12-month forecast renders independently of spend history: a
       fresh car with a schedule has a forecast before its first invoice. -->
  {#if !loading && forecast && forecast.total_cents > 0}
    <h4>Next {forecast.horizon_months} Months (forecast)</h4>
    <div class="summary-grid" data-testid="forecast-buckets">
      <div class="summary-card">
        <span class="card-label">Projected Maintenance</span>
        <span class="card-value">{fmt(forecast.projected_maintenance_cents)}</span>
        <span class="card-sub">from the schedule</span>
      </div>
      <div class="summary-card">
        <span class="card-label">Planned Visits</span>
        <span class="card-value">{fmt(forecast.planned_visits_cents)}</span>
      </div>
      <div class="summary-card">
        <span class="card-label">To-do Backlog</span>
        <span class="card-value">{fmt(forecast.planned_work_cents)}</span>
      </div>
      <div class="summary-card">
        <span class="card-label">Forecast Total</span>
        <span class="card-value">{fmt(forecast.total_cents)}</span>
      </div>
    </div>
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
    border-radius: var(--radius-lg);
    background: var(--bg-raised);
    box-shadow: inset 0 1px 0 var(--edge-highlight);
    display: flex;
    flex-direction: column;
    transition: border-color var(--duration-base) var(--ease-out);
  }

  .summary-card:hover {
    border-color: var(--border);
  }

  .card-label {
    font-family: var(--font-display);
    font-size: 0.74rem;
    font-weight: 600;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.11em;
  }

  .card-value {
    font-family: var(--font-numeral);
    font-variant-numeric: tabular-nums;
    font-size: 1.35rem;
    font-weight: 700;
    letter-spacing: -0.01em;
    margin-top: var(--sp-1);
  }

  .card-sub {
    font-size: 0.78rem;
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

  /* Numeric columns read like a ledger. */
  .cost-table th:not(:first-child),
  .cost-table td:not(:first-child) {
    text-align: right;
  }

  .cost-table td:not(:first-child) {
    font-family: var(--font-numeral);
    font-variant-numeric: tabular-nums;
    font-size: 0.82rem;
  }

  .cost-table th {
    font-family: var(--font-display);
    font-weight: 600;
    font-size: 0.76rem;
    text-transform: uppercase;
    letter-spacing: 0.09em;
    color: var(--text-muted);
  }

  .cost-table tbody tr:hover td {
    background: var(--surface);
  }

  .cost-table .total { font-weight: 700; }

  .empty { color: var(--text-muted); text-align: center; padding: var(--sp-8) 0; }
</style>
