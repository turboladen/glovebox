<script lang="ts">
  import { onMount } from 'svelte'
  import { link } from '@keenmate/svelte-spa-router'
  import { vehicles as vehiclesApi, reminders as remindersApi, mileage as mileageApi, vehicleExport } from '../lib/api'
  import type { Vehicle, RemindersResponse } from '../lib/types'
  import { formatDate } from '../lib/dates'
  import ScheduleTab from './ScheduleTab.svelte'
  import HistoryTab from './HistoryTab.svelte'
  import MileageEntry from './MileageEntry.svelte'
  import ServiceForm from './ServiceForm.svelte'
  import ObservationsTab from './ObservationsTab.svelte'
  import DocumentsTab from './DocumentsTab.svelte'
  import PartsTab from './PartsTab.svelte'
  import CostsTab from './CostsTab.svelte'
  import ChatTab from './ChatTab.svelte'
  import ResearchTab from './ResearchTab.svelte'
  import AccidentsTab from './AccidentsTab.svelte'
  import VehicleEdit from './VehicleEdit.svelte'

  let { routeParams = {} }: { routeParams?: Record<string, string> } = $props()

  let vehicle: Vehicle | null = $state(null)
  let reminderData: RemindersResponse | null = $state(null)
  let loading = $state(true)
  let error = $state('')
  let activeTab = $state('schedule')
  let showMileageForm = $state(false)
  let showServiceForm = $state(false)
  let showEditForm = $state(false)

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

  function onVehicleUpdated(updated: Vehicle) {
    vehicle = updated
    showEditForm = false
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
  <br>Generated: ${esc(formatDate(new Date().toISOString()))}
</div>
<h2>Service Records (${data.record_count})</h2>
<table><thead><tr><th>Date</th><th>Mileage</th><th>Description</th><th>Cost</th><th>Shop</th></tr></thead><tbody>
${data.service_records.map(r => `<tr><td>${esc(formatDate(r.date))}</td><td>${r.mileage?.toLocaleString() ?? ''}</td><td>${esc(r.description)}</td><td>${esc(r.total_cost)}</td><td>${esc(r.shop)}</td></tr>`).join('')}
</tbody></table>
${data.installed_parts.length ? `<h2>Installed Parts</h2>
<table><thead><tr><th>Part</th><th>Manufacturer</th><th>Part #</th><th>Installed</th><th>Mileage</th><th>Cost</th></tr></thead><tbody>
${data.installed_parts.map(p => `<tr><td>${esc(p.name)}</td><td>${esc(p.manufacturer)}</td><td>${esc(p.part_number)}</td><td>${esc(formatDate(p.installed_date))}</td><td>${p.installed_odometer?.toLocaleString() ?? ''}</td><td>${esc(p.cost)}</td></tr>`).join('')}
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
      {#if vehicle.year || vehicle.make || vehicle.model}
        <p class="vehicle-subtitle">
          {[vehicle.year, vehicle.make, vehicle.model, vehicle.trim_level].filter(Boolean).join(' ')}
        </p>
      {/if}
      {#if vehicle.sold_date}
        <span class="sold-badge">Sold {formatDate(vehicle.sold_date)}</span>
      {/if}
    </div>

    <div class="status-bar">
      {#if reminderData}
        <div class="mileage-readout">
          <span class="est-mileage">{formatMileage(reminderData.estimated_mileage)}</span>
          <span class="mileage-unit">mi{#if reminderData.mileage_is_estimate} est.{/if}</span>
          <span class="mileage-date">as of {formatDate(reminderData.mileage_as_of)}</span>
        </div>
      {/if}
      <div class="actions">
        <button class="btn btn-secondary" onclick={() => (showEditForm = !showEditForm)}>
          Edit
        </button>
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

    {#if showEditForm}
      <VehicleEdit {vehicle} onComplete={onVehicleUpdated} onCancel={() => (showEditForm = false)} />
    {/if}

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
      <button class="tab" class:active={activeTab === 'accidents'} onclick={() => (activeTab = 'accidents')}>
        Accidents
      </button>
      <button class="tab" class:active={activeTab === 'costs'} onclick={() => (activeTab = 'costs')}>
        Costs
      </button>
      <button class="tab" class:active={activeTab === 'research'} onclick={() => (activeTab = 'research')}>
        Research
      </button>
      <button class="tab" class:active={activeTab === 'ai'} onclick={() => (activeTab = 'ai')}>
        AI
      </button>
    </div>

    {#key activeTab}
      <div class="tab-content tab-content-enter">
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
        {:else if activeTab === 'accidents'}
          <AccidentsTab vehicleId={vehicle.id} />
        {:else if activeTab === 'costs'}
          <CostsTab vehicleId={vehicle.id} />
        {:else if activeTab === 'research'}
          <ResearchTab vehicleId={vehicle.id} />
        {:else if activeTab === 'ai'}
          <ChatTab vehicleId={vehicle.id} />
        {/if}
      </div>
    {/key}
  </div>
{/if}

<style>
  .detail-header {
    margin-bottom: var(--sp-4);
  }

  .back-link {
    display: inline-flex;
    align-items: center;
    gap: var(--sp-1);
    font-size: 0.8rem;
    font-weight: 500;
    color: var(--text-muted);
    text-decoration: none;
    letter-spacing: 0.02em;
    transition: color var(--duration-fast) var(--ease-out);
  }

  .back-link:hover {
    color: var(--primary);
  }

  .detail-header h1 {
    margin: var(--sp-2) 0 0;
    font-family: var(--font-display);
    font-size: 1.6rem;
    font-weight: 700;
    letter-spacing: -0.02em;
  }

  .vehicle-subtitle {
    margin: var(--sp-1) 0 0;
    font-size: 0.9rem;
    color: var(--text-muted);
  }

  .sold-badge {
    display: inline-block;
    margin-top: var(--sp-2);
    padding: var(--sp-1) var(--sp-3);
    font-size: 0.75rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--warning);
    border: 1px solid var(--warning);
    border-radius: var(--radius-sm);
  }

  /* --- Instrument cluster status bar --- */
  .status-bar {
    display: flex;
    align-items: center;
    gap: var(--sp-4);
    flex-wrap: wrap;
    margin-bottom: var(--sp-5);
    padding: var(--sp-4) var(--sp-5);
    background: var(--bg-raised);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-lg);
  }

  .mileage-readout {
    display: flex;
    align-items: baseline;
    gap: var(--sp-1);
  }

  .est-mileage {
    font-family: var(--font-display);
    font-weight: 700;
    font-size: 1.25rem;
    letter-spacing: -0.02em;
    color: var(--text);
  }

  .mileage-unit {
    font-family: var(--font-display);
    font-size: 0.85rem;
    font-weight: 600;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.03em;
  }

  .mileage-date {
    font-size: 0.85rem;
    color: var(--text-muted);
    margin-left: var(--sp-1);
  }

  .actions {
    margin-left: auto;
    display: flex;
    gap: var(--sp-2);
  }

  /* --- Tab navigation --- */
  .tabs {
    display: flex;
    gap: 0;
    border-bottom: 1px solid var(--border-subtle);
    margin-bottom: var(--sp-5);
    overflow-x: auto;
    scrollbar-width: none;
  }

  .tabs::-webkit-scrollbar {
    display: none;
  }

  .tab {
    position: relative;
    padding: var(--sp-3) var(--sp-4);
    background: none;
    border: none;
    cursor: pointer;
    font-family: var(--font-display);
    font-size: 0.85rem;
    font-weight: 500;
    color: var(--text-muted);
    white-space: nowrap;
    transition:
      color var(--duration-fast) var(--ease-out);
  }

  .tab::after {
    content: '';
    position: absolute;
    bottom: -1px;
    left: var(--sp-4);
    right: var(--sp-4);
    height: 2px;
    background: var(--primary);
    border-radius: 1px;
    transform: scaleX(0);
    transition: transform var(--duration-base) var(--ease-out);
  }

  .tab.active {
    color: var(--primary);
    font-weight: 600;
  }

  .tab.active::after {
    transform: scaleX(1);
  }

  .tab:hover:not(.active) {
    color: var(--text);
  }

  .tab:hover:not(.active)::after {
    transform: scaleX(0.5);
    background: var(--text-muted);
  }

  /* --- States --- */
  .loading {
    text-align: center;
    padding: var(--sp-12) 0;
    color: var(--text-muted);
  }

  .error {
    color: var(--danger);
    padding: var(--sp-4);
    background: var(--danger-bg);
    border: 1px solid var(--danger-border);
    border-radius: var(--radius-md);
  }

  /* --- Mobile --- */
  @media (max-width: 640px) {
    .detail-header h1 {
      font-size: 1.3rem;
    }

    .status-bar {
      padding: var(--sp-3) var(--sp-4);
      gap: var(--sp-2);
    }

    .est-mileage {
      font-size: 1.15rem;
    }

    .actions {
      width: 100%;
      margin-left: 0;
    }

    .actions .btn {
      flex: 1;
      justify-content: center;
      font-size: 0.8rem;
      padding: var(--sp-2) var(--sp-2);
    }

    .tabs {
      margin-left: calc(-1 * var(--sp-4));
      margin-right: calc(-1 * var(--sp-4));
      padding-left: var(--sp-4);
      padding-right: var(--sp-4);
      /* Fade hint for scroll */
      mask-image: linear-gradient(to right, black 90%, transparent 100%);
      -webkit-mask-image: linear-gradient(to right, black 90%, transparent 100%);
    }

    .tab {
      padding: var(--sp-2) var(--sp-3);
      font-size: 0.8rem;
    }
  }
</style>
