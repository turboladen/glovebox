<script lang="ts">
  import { link, push, replace } from '@keenmate/svelte-spa-router'
  import { vehicles as vehiclesApi, reminders as remindersApi, mileage as mileageApi, vehicleExport, research } from '../lib/api'
  import type { Vehicle, RemindersResponse } from '../lib/types'
  import { formatDate } from '../lib/dates'
  import { refreshDashboard } from '../lib/stores'
  import Dashboard from './Dashboard.svelte'
  import PlanTab from './PlanTab.svelte'
  import TimelineTab from './TimelineTab.svelte'
  import BuildsTab from './BuildsTab.svelte'
  import RecordsTab from './RecordsTab.svelte'
  import MileageEntry from './MileageEntry.svelte'
  import CostsTab from './CostsTab.svelte'
  import VehicleEdit from './VehicleEdit.svelte'

  let { routeParams = {} }: { routeParams?: Record<string, string> } = $props()

  let vehicle: Vehicle | null = $state(null)
  let reminderData: RemindersResponse | null = $state(null)
  let plannedCount = $state(0)
  let loading = $state(true)
  let error = $state('')
  let showMileageForm = $state(false)
  let showEditForm = $state(false)
  let menuOpen = $state(false)
  let menuWrap: HTMLElement | undefined = $state(undefined)

  // Tabs are URL-driven (/vehicles/:id/:tab[/:sub]) so dashboard rows,
  // sidebar entries, and search hits can deep-link into a view.
  // Unknown :tab params fall back to Overview instead of a blank pane.
  const knownTabs = ['overview', 'timeline', 'plan', 'builds', 'records', 'costs']
  let vehicleId = $derived(parseInt(routeParams.id))
  let activeTab = $derived(
    routeParams.tab && knownTabs.includes(routeParams.tab) ? routeParams.tab : 'overview',
  )

  // Research moved from Records to Plan (UX quick wins) — keep old deep
  // links working.
  $effect(() => {
    if (routeParams.tab === 'records' && routeParams.sub === 'research') {
      replace(`/vehicles/${routeParams.id}/plan/research`)
    }
  })

  function openTab(tab: string) {
    push(`/vehicles/${vehicleId}${tab === 'overview' ? '' : `/${tab}`}`)
  }

  // ONE record-service verb, ONE form: route to the Timeline with its
  // service form open (the ?action=record param is read by TimelineTab).
  function openRecordService() {
    push(`/vehicles/${vehicleId}/timeline?action=record`)
  }

  function onDocClick(e: MouseEvent) {
    if (menuOpen && menuWrap && !menuWrap.contains(e.target as Node)) {
      menuOpen = false
    }
  }

  function onDocKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') menuOpen = false
  }

  async function loadData() {
    try {
      const id = parseInt(routeParams.id)
      const [v, r, planned] = await Promise.all([
        vehiclesApi.get(id),
        remindersApi.get(id),
        research.listFindings(id, 'planned'),
      ])
      vehicle = v
      reminderData = r
      plannedCount = planned.length
    } catch (e: any) {
      error = e.message
    } finally {
      loading = false
    }
  }

  // Reload when the sidebar switches vehicles in place (same component,
  // new :id param).
  let loadedId: number | null = $state(null)
  $effect(() => {
    if (!Number.isNaN(vehicleId) && vehicleId !== loadedId) {
      loadedId = vehicleId
      loading = true
      loadData()
    }
  })

  async function onMileageAdded() {
    showMileageForm = false
    await refreshReminders()
  }

  async function refreshReminders() {
    if (vehicle) {
      reminderData = await remindersApi.get(vehicle.id)
    }
    // Mileage/service/timeline mutations also feed the shared dashboard
    // snapshot (sidebar hints + Overview blocks) — keep it fresh.
    refreshDashboard().catch(() => {})
  }

  function onVehicleUpdated(updated: Vehicle) {
    vehicle = updated
    showEditForm = false
    refreshDashboard().catch(() => {})
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

  async function archiveVehicle() {
    if (!vehicle) return
    if (!confirm(`Archive "${vehicle.name}"? It moves to the sidebar's Archived group (reversible).`)) return
    try {
      vehicle = await vehiclesApi.archive(vehicle.id)
      refreshDashboard().catch(() => {})
    } catch (e: any) {
      error = e.message
    }
  }

  async function unarchiveVehicle() {
    if (!vehicle) return
    try {
      vehicle = await vehiclesApi.unarchive(vehicle.id)
      refreshDashboard().catch(() => {})
    } catch (e: any) {
      error = e.message
    }
  }

  async function deleteVehicle() {
    if (!vehicle) return
    if (!confirm(`Are you sure? This will permanently delete all of "${vehicle.name}"'s data.`)) return
    try {
      await vehiclesApi.delete(vehicle.id)
      refreshDashboard().catch(() => {})
      push('/')
    } catch (e: any) {
      error = e.message
    }
  }
</script>

<svelte:document onclick={onDocClick} onkeydown={onDocKeydown} />

{#if loading}
  <p class="loading">Loading...</p>
{:else if error}
  <p class="error">{error}</p>
{:else if vehicle}
  <div class="vehicle-detail">
    <div class="detail-header">
      <a href="/" use:link class="back-link">← All vehicles</a>
      <h1>{vehicle.name}</h1>
      {#if vehicle.year || vehicle.make || vehicle.model}
        <p class="vehicle-subtitle">
          {[vehicle.year, vehicle.make, vehicle.model, vehicle.trim_level].filter(Boolean).join(' ')}
        </p>
      {/if}
      {#if vehicle.archived_at}
        <span class="archived-badge">Archived</span>
      {/if}
      {#if vehicle.sold_date}
        <span class="sold-badge">Sold {formatDate(vehicle.sold_date)}</span>
      {/if}
    </div>

    <div class="status-bar">
      {#if reminderData}
        <div class="mileage-readout">
          <span class="est-mileage">{formatMileage(reminderData.estimated_mileage)}</span>
          <span class="mileage-unit">mi</span>
          {#if reminderData.mileage_is_estimate}<span class="est-flag">est.</span>{/if}
          <span class="mileage-date">as of {formatDate(reminderData.mileage_as_of)}</span>
        </div>
      {/if}
      <!-- The two everyday verbs stay visible at equal weight; everything
           occasional lives behind the ⋯ overflow menu. -->
      <div class="actions">
        <button class="btn btn-secondary" onclick={() => (showMileageForm = !showMileageForm)}>
          Update mileage
        </button>
        <button class="btn btn-secondary" onclick={openRecordService}>
          Record service
        </button>
        <div class="overflow-wrap" bind:this={menuWrap}>
          <button
            class="btn btn-secondary btn-overflow"
            aria-label="More actions"
            title="More actions"
            aria-haspopup="menu"
            aria-expanded={menuOpen}
            onclick={() => (menuOpen = !menuOpen)}
          >
            ⋯
          </button>
          {#if menuOpen}
            <div class="overflow-menu" role="menu">
              <button role="menuitem" class="menu-item" onclick={() => { menuOpen = false; showEditForm = !showEditForm }}>
                Edit vehicle…
              </button>
              <button role="menuitem" class="menu-item" onclick={() => { menuOpen = false; exportHistory() }}>
                Export history
              </button>
              {#if vehicle.archived_at}
                <button role="menuitem" class="menu-item" onclick={() => { menuOpen = false; unarchiveVehicle() }}>
                  Unarchive vehicle
                </button>
                <button role="menuitem" class="menu-item danger" onclick={() => { menuOpen = false; deleteVehicle() }}>
                  Delete vehicle…
                </button>
              {:else}
                <button role="menuitem" class="menu-item" onclick={() => { menuOpen = false; archiveVehicle() }}>
                  Archive vehicle…
                </button>
              {/if}
            </div>
          {/if}
        </div>
      </div>
    </div>

    {#if showEditForm}
      <VehicleEdit {vehicle} onComplete={onVehicleUpdated} onCancel={() => (showEditForm = false)} />
    {/if}

    {#if showMileageForm}
      <MileageEntry vehicleId={vehicle.id} onComplete={onMileageAdded} onCancel={() => (showMileageForm = false)} />
    {/if}

    <div class="tabs">
      <button class="tab" class:active={activeTab === 'overview'} onclick={() => openTab('overview')}>
        Overview
      </button>
      <button class="tab" class:active={activeTab === 'timeline'} onclick={() => openTab('timeline')}>
        Timeline
      </button>
      <button class="tab" class:active={activeTab === 'plan'} onclick={() => openTab('plan')}>
        Plan{#if plannedCount > 0} <span class="badge badge-planned">{plannedCount}</span>{/if}
      </button>
      <button class="tab" class:active={activeTab === 'builds'} onclick={() => openTab('builds')}>
        Builds
      </button>
      <button class="tab" class:active={activeTab === 'records'} onclick={() => openTab('records')}>
        Records
      </button>
      <button class="tab" class:active={activeTab === 'costs'} onclick={() => openTab('costs')}>
        Costs
      </button>
    </div>

    {#key activeTab}
      <div class="tab-content tab-content-enter">
        {#if activeTab === 'overview'}
          <Dashboard vehicleId={vehicle.id} />
        {:else if activeTab === 'timeline'}
          <TimelineTab vehicleId={vehicle.id} estimatedMileage={reminderData?.estimated_mileage} onChanged={refreshReminders} />
        {:else if activeTab === 'plan'}
          <PlanTab vehicleId={vehicle.id} {reminderData} sub={routeParams.sub ?? 'due'} onScheduleChanged={refreshReminders} />
        {:else if activeTab === 'builds'}
          <BuildsTab vehicleId={vehicle.id} />
        {:else if activeTab === 'records'}
          <RecordsTab vehicleId={vehicle.id} sub={routeParams.sub ?? 'parts'} />
        {:else if activeTab === 'costs'}
          <CostsTab vehicleId={vehicle.id} />
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
    font-size: 1.8rem;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .vehicle-subtitle {
    margin: var(--sp-1) 0 0;
    font-size: 0.9rem;
    color: var(--text-muted);
  }

  .archived-badge,
  .sold-badge {
    display: inline-block;
    margin-top: var(--sp-2);
    padding: var(--sp-1) var(--sp-3);
    font-size: 0.75rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    border-radius: var(--radius-sm);
  }

  .archived-badge {
    color: var(--text-muted);
    border: 1px solid var(--border);
    margin-right: var(--sp-2);
  }

  .sold-badge {
    color: var(--warning);
    border: 1px solid var(--warning);
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
    box-shadow: inset 0 1px 0 var(--edge-highlight), var(--shadow-sm);
  }

  .mileage-readout {
    display: flex;
    align-items: baseline;
    gap: var(--sp-2);
    padding: var(--sp-1) var(--sp-3);
    margin-left: calc(-1 * var(--sp-3));
    border-left: 3px solid var(--primary);
  }

  /* The odometer: the number a car person actually reads. */
  .est-mileage {
    font-family: var(--font-mono);
    font-variant-numeric: tabular-nums;
    font-weight: 700;
    font-size: 1.65rem;
    letter-spacing: 0.01em;
    line-height: 1.1;
    color: var(--text);
  }

  .mileage-unit {
    font-family: var(--font-display);
    font-size: 0.85rem;
    font-weight: 600;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.09em;
  }

  .est-flag {
    font-family: var(--font-display);
    font-size: 0.7rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--text-muted);
    border: 1px solid var(--border);
    border-radius: 999px;
    padding: 0 var(--sp-2);
    align-self: center;
  }

  .mileage-date {
    font-size: 0.82rem;
    color: var(--text-muted);
    margin-left: var(--sp-1);
  }

  .actions {
    margin-left: auto;
    display: flex;
    gap: var(--sp-2);
  }

  /* --- ⋯ overflow menu --- */
  .overflow-wrap {
    position: relative;
  }

  .btn-overflow {
    padding-left: var(--sp-3);
    padding-right: var(--sp-3);
    font-weight: 700;
    letter-spacing: 0.05em;
  }

  .overflow-menu {
    position: absolute;
    top: calc(100% + 4px);
    right: 0;
    min-width: 170px;
    padding: var(--sp-1);
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    box-shadow: var(--shadow-lg);
    z-index: 40;
    display: flex;
    flex-direction: column;
    animation: fade-in-down var(--duration-fast) var(--ease-out) both;
  }

  .menu-item {
    padding: var(--sp-2) var(--sp-3);
    border: none;
    border-radius: var(--radius-sm);
    background: none;
    text-align: left;
    font-family: var(--font-display);
    font-size: 0.85rem;
    color: var(--text-secondary);
    cursor: pointer;
    white-space: nowrap;
    transition:
      background var(--duration-fast) var(--ease-out),
      color var(--duration-fast) var(--ease-out);
  }

  .menu-item:hover {
    background: var(--surface-hover);
    color: var(--text);
  }

  .menu-item.danger {
    color: var(--danger);
  }

  .menu-item.danger:hover {
    background: var(--danger-bg);
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
    font-size: 0.95rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.09em;
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
