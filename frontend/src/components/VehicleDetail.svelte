<script lang="ts">
  import { onMount } from 'svelte'
  import { link } from '@keenmate/svelte-spa-router'
  import { vehicles as vehiclesApi, reminders as remindersApi, mileage as mileageApi, vehicleExport } from '../lib/api'
  import type { Vehicle, RemindersResponse } from '../lib/types'
  import ScheduleTab from './ScheduleTab.svelte'
  import HistoryTab from './HistoryTab.svelte'
  import MileageEntry from './MileageEntry.svelte'
  import ServiceForm from './ServiceForm.svelte'
  import ObservationsTab from './ObservationsTab.svelte'
  import DocumentsTab from './DocumentsTab.svelte'
  import PartsTab from './PartsTab.svelte'
  import CostsTab from './CostsTab.svelte'

  let { routeParams = {} }: { routeParams?: Record<string, string> } = $props()

  let vehicle: Vehicle | null = $state(null)
  let reminderData: RemindersResponse | null = $state(null)
  let loading = $state(true)
  let error = $state('')
  let activeTab = $state('schedule')
  let showMileageForm = $state(false)
  let showServiceForm = $state(false)

  async function loadData() {
    try {
      const id = parseInt(routeParams.id)
      vehicle = await vehiclesApi.get(id)
      reminderData = await remindersApi.get(id)
    } catch (e: any) {
      error = e.message
    } finally {
      loading = false
    }
  }

  onMount(loadData)

  async function onMileageAdded() {
    showMileageForm = false
    if (vehicle) {
      reminderData = await remindersApi.get(vehicle.id)
    }
  }

  async function onServiceAdded() {
    showServiceForm = false
    if (vehicle) {
      reminderData = await remindersApi.get(vehicle.id)
    }
  }

  function formatMileage(n: number): string {
    return n.toLocaleString()
  }

  function esc(s: string | number | null | undefined): string {
    if (s === null || s === undefined) return ''
    return String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;')
  }

  async function exportHistory() {
    if (!vehicle) return
    try {
      const data = await vehicleExport.get(vehicle.id)
      const w = window.open('', '_blank')
      if (!w) return
      w.document.write(`<!DOCTYPE html>
<html><head><title>Service History - ${esc(data.vehicle_name)}</title>
<style>
  body { font-family: -apple-system, BlinkMacSystemFont, sans-serif; max-width: 800px; margin: 2rem auto; padding: 0 1rem; color: #333; }
  h1 { font-size: 1.5rem; border-bottom: 2px solid #333; padding-bottom: 0.5rem; }
  h2 { font-size: 1.1rem; margin-top: 1.5rem; }
  .meta { color: #666; font-size: 0.9rem; margin-bottom: 1rem; }
  table { width: 100%; border-collapse: collapse; font-size: 0.85rem; margin-bottom: 1rem; }
  th, td { padding: 0.4rem 0.6rem; text-align: left; border-bottom: 1px solid #ddd; }
  th { background: #f5f5f5; font-weight: 600; }
  .totals { margin-top: 1rem; font-size: 0.9rem; }
  .totals strong { display: inline-block; width: 150px; }
  @media print { body { margin: 0; } button { display: none; } }
</style></head><body>
<h1>Service History: ${esc(data.vehicle_name)}</h1>
<div class="meta">
  ${[data.year, data.make, data.model].filter(Boolean).map(v => esc(v)).join(' ')}
  ${data.vin ? `<br>VIN: ${esc(data.vin)}` : ''}
  <br>Generated: ${esc(new Date().toLocaleDateString())}
</div>
<h2>Service Records (${data.record_count})</h2>
<table><thead><tr><th>Date</th><th>Mileage</th><th>Description</th><th>Cost</th><th>Shop</th></tr></thead><tbody>
${data.service_records.map(r => `<tr><td>${esc(r.date)}</td><td>${r.mileage?.toLocaleString() ?? ''}</td><td>${esc(r.description)}</td><td>${esc(r.total_cost)}</td><td>${esc(r.shop)}</td></tr>`).join('')}
</tbody></table>
${data.installed_parts.length ? `<h2>Installed Parts</h2>
<table><thead><tr><th>Part</th><th>Manufacturer</th><th>Part #</th><th>Installed</th><th>Mileage</th><th>Cost</th></tr></thead><tbody>
${data.installed_parts.map(p => `<tr><td>${esc(p.name)}</td><td>${esc(p.manufacturer)}</td><td>${esc(p.part_number)}</td><td>${esc(p.installed_date)}</td><td>${p.installed_odometer?.toLocaleString() ?? ''}</td><td>${esc(p.cost)}</td></tr>`).join('')}
</tbody></table>` : ''}
<div class="totals">
  <strong>Services:</strong> ${esc(data.total_service_cost)}<br>
  <strong>Parts:</strong> ${esc(data.total_parts_cost)}<br>
  <strong>Total:</strong> ${esc(data.total_cost)}
</div>
<br><button onclick="window.print()">Print</button>
</body></html>`)
      w.document.close()
    } catch (e) {
      console.error('Export failed:', e)
    }
  }
</script>

{#if loading}
  <p class="loading">Loading...</p>
{:else if error}
  <p class="error">{error}</p>
{:else if vehicle}
  <div class="vehicle-detail">
    <div class="detail-header">
      <a href="/" use:link class="back-link">← Garage</a>
      <h1>{vehicle.name}</h1>
    </div>

    <div class="status-bar">
      {#if reminderData}
        <span class="est-mileage">{formatMileage(reminderData.estimated_mileage)} mi (est.)</span>
        <span class="mileage-date">as of {reminderData.mileage_as_of}</span>
      {/if}
      <div class="actions">
        <button class="btn btn-secondary" onclick={() => (showMileageForm = !showMileageForm)}>
          Update Mileage
        </button>
        <button class="btn btn-primary" onclick={() => (showServiceForm = !showServiceForm)}>
          Log Service
        </button>
        <button class="btn btn-secondary" onclick={exportHistory}>
          Export History
        </button>
      </div>
    </div>

    {#if showMileageForm}
      <MileageEntry vehicleId={vehicle.id} onComplete={onMileageAdded} onCancel={() => (showMileageForm = false)} />
    {/if}

    {#if showServiceForm}
      <ServiceForm vehicleId={vehicle.id} onComplete={onServiceAdded} onCancel={() => (showServiceForm = false)} />
    {/if}

    <div class="tabs">
      <button class="tab" class:active={activeTab === 'schedule'} onclick={() => (activeTab = 'schedule')}>
        Schedule
      </button>
      <button class="tab" class:active={activeTab === 'history'} onclick={() => (activeTab = 'history')}>
        History
      </button>
      <button class="tab" class:active={activeTab === 'parts'} onclick={() => (activeTab = 'parts')}>
        Parts
      </button>
      <button class="tab" class:active={activeTab === 'observations'} onclick={() => (activeTab = 'observations')}>
        Obs.
      </button>
      <button class="tab" class:active={activeTab === 'documents'} onclick={() => (activeTab = 'documents')}>
        Docs
      </button>
      <button class="tab" class:active={activeTab === 'costs'} onclick={() => (activeTab = 'costs')}>
        Costs
      </button>
    </div>

    <div class="tab-content">
      {#if activeTab === 'schedule'}
        <ScheduleTab {reminderData} vehicleId={vehicle.id} />
      {:else if activeTab === 'history'}
        <HistoryTab vehicleId={vehicle.id} />
      {:else if activeTab === 'parts'}
        <PartsTab vehicleId={vehicle.id} />
      {:else if activeTab === 'observations'}
        <ObservationsTab vehicleId={vehicle.id} />
      {:else if activeTab === 'documents'}
        <DocumentsTab vehicleId={vehicle.id} />
      {:else if activeTab === 'costs'}
        <CostsTab vehicleId={vehicle.id} />
      {/if}
    </div>
  </div>
{/if}

<style>
  .detail-header {
    margin-bottom: 1rem;
  }

  .back-link {
    font-size: 0.85rem;
    color: var(--text-muted);
    text-decoration: none;
  }

  .back-link:hover {
    color: var(--text);
  }

  .detail-header h1 {
    margin: 0.25rem 0 0;
  }

  .status-bar {
    display: flex;
    align-items: center;
    gap: 1rem;
    flex-wrap: wrap;
    margin-bottom: 1rem;
    padding: 0.75rem 1rem;
    background: var(--surface);
    border-radius: 8px;
  }

  .est-mileage {
    font-weight: 600;
    font-size: 1.1rem;
  }

  .mileage-date {
    font-size: 0.85rem;
    color: var(--text-muted);
  }

  .actions {
    margin-left: auto;
    display: flex;
    gap: 0.5rem;
  }

  .tabs {
    display: flex;
    gap: 0;
    border-bottom: 2px solid var(--border);
    margin-bottom: 1rem;
  }

  .tab {
    padding: 0.5rem 1rem;
    background: none;
    border: none;
    border-bottom: 2px solid transparent;
    margin-bottom: -2px;
    cursor: pointer;
    font-size: 0.9rem;
    color: var(--text-muted);
  }

  .tab.active {
    color: var(--text);
    border-bottom-color: var(--primary);
    font-weight: 600;
  }

  .tab:hover:not(.active) {
    color: var(--text);
  }

  .loading, .error {
    padding: 2rem 0;
  }

  .error {
    color: var(--danger);
  }
</style>
