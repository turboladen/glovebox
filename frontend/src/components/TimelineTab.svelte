<script lang="ts">
  // Timeline (unit F): ONE chronological stream subsuming the retired
  // History and Incidents tabs — services + incidents + manual odometer
  // readings, newest first, with kind filters. Service rows expand into
  // the service card's detail/edit UI (ported from HistoryTab); incident
  // rows expand into IncidentDetail (ported from IncidentsTab). Creation
  // lives here too: Record service (existing ServiceForm) and Log
  // incident (IncidentForm).
  import { onMount } from 'svelte'
  import { querystring, replace } from '@keenmate/svelte-spa-router'
  import {
    services as servicesApi,
    incidents as incidentsApi,
    parts as partsApi,
    shops as shopsApi,
    mileage as mileageApi,
  } from '../lib/api'
  import { anchorId, flashHighlightFromQuery } from '../lib/highlight'
  import type {
    ServiceRecordWithLinks,
    IncidentWithDetails,
    MileageEntry,
    Part,
    Shop,
  } from '../lib/types'
  import { formatDate } from '../lib/dates'
  import ServiceForm from './ServiceForm.svelte'
  import IncidentForm from './IncidentForm.svelte'
  import IncidentDetail from './IncidentDetail.svelte'

  let { vehicleId, estimatedMileage, onChanged }: {
    vehicleId: number
    estimatedMileage?: number
    onChanged?: () => void
  } = $props()

  type TimelineEntry =
    | { type: 'service'; date: string; data: ServiceRecordWithLinks }
    | { type: 'incident'; date: string; data: IncidentWithDetails }
    | { type: 'mileage'; date: string; data: MileageEntry }

  const PAGE = 25

  let services: ServiceRecordWithLinks[] = $state([])
  let incidents: IncidentWithDetails[] = $state([])
  let mileageLogs: MileageEntry[] = $state([])
  let allParts: Part[] = $state([])
  let shopList: Shop[] = $state([])
  let loading = $state(true)
  let filter = $state<'all' | 'services' | 'incidents' | 'mileage'>('all')
  // Second-level filter, shown only when the kind filter is Incidents
  // (same category whitelist as IncidentForm).
  const incidentCategories = [
    'general', 'noise', 'leak', 'warning_light', 'cosmetic',
    'performance', 'obd_code', 'damage', 'accident', 'note',
  ]
  let incidentCategory = $state('all')
  let limit = $state(PAGE)

  // Creation forms
  let showServiceForm = $state(false)
  let showIncidentForm = $state(false)
  let editingIncident: IncidentWithDetails | null = $state(null)

  // Expanded/editing state (service rows)
  let expandedId: string | null = $state(null)
  let editing = $state(false)
  let editDate = $state('')
  let editMileage = $state('')
  let editDescription = $state('')
  let editCostDollars = $state('')
  let editShopName = $state('')
  let editShopId: number | null = $state(null)
  let editNotes = $state('')
  let editPaidBy = $state('self')
  let editPayerNote = $state('')
  let saving = $state(false)
  let deleting = $state(false)
  let confirmDelete = $state(false)

  async function loadData() {
    try {
      const [svcList, incidentList, mileageList, partsList, shops] = await Promise.all([
        servicesApi.list(vehicleId),
        incidentsApi.list(vehicleId),
        mileageApi.list(vehicleId),
        partsApi.list(vehicleId),
        shopsApi.list(),
      ])
      services = svcList
      incidents = incidentList
      // Service-created logs are excluded: the service row already shows
      // that odometer reading (same rule as the backend activity feed).
      mileageLogs = mileageList.filter((m) => m.service_record_id == null)
      allParts = partsList
      shopList = shops
    } catch (e) {
      console.error(e)
    } finally {
      loading = false
    }
  }

  onMount(async () => {
    await loadData()
    // Deep-link highlight (?hl=service:N / incident:N — see lib/highlight).
    for (const kind of ['service', 'incident', 'mileage']) {
      flashHighlightFromQuery(kind)
    }
  })

  // The vehicle header's "Record service" routes here with ?action=record
  // so there is ONE service form; reacting to the querystring also covers
  // clicking it while already on the Timeline.
  $effect(() => {
    if (new URLSearchParams(querystring() ?? '').get('action') === 'record') {
      showServiceForm = true
      showIncidentForm = false
      editingIncident = null
      // Consume the param: pushing an IDENTICAL hash fires no hashchange, so
      // leaving it in place makes the header's second click dead and makes a
      // refresh re-open the form.
      replace(`/vehicles/${vehicleId}/timeline`)
    }
  })

  async function refresh() {
    await loadData()
    onChanged?.()
  }

  let entries: TimelineEntry[] = $derived.by(() => {
    const timeline: TimelineEntry[] = [
      ...services.map((s): TimelineEntry => ({ type: 'service', date: s.service_date, data: s })),
      ...incidents.map((i): TimelineEntry => ({ type: 'incident', date: i.occurred_at, data: i })),
      ...mileageLogs.map((m): TimelineEntry => ({ type: 'mileage', date: m.recorded_at, data: m })),
    ]
    // Date-only service dates sort at end-of-day, matching the backend
    // feed ("noticed it in the morning, fixed it that afternoon").
    const key = (d: string) => (d.length === 10 ? `${d} 23:59:59` : d)
    timeline.sort((a, b) => key(b.date).localeCompare(key(a.date)))
    return timeline
  })

  let filtered = $derived(
    filter === 'all'
      ? entries
      : entries.filter((e) =>
          filter === 'services' ? e.type === 'service'
          : filter === 'incidents'
            ? e.type === 'incident' &&
              (incidentCategory === 'all' || e.data.category === incidentCategory)
          : e.type === 'mileage'),
  )

  let visible = $derived(filtered.slice(0, limit))

  function partsForService(service: ServiceRecordWithLinks): Part[] {
    if (!service.part_ids.length) return []
    const ids = new Set(service.part_ids)
    return allParts.filter((p) => ids.has(p.id))
  }

  function incidentsForService(serviceId: number): IncidentWithDetails[] {
    return incidents.filter((i) => i.service_record_ids.includes(serviceId))
  }

  function formatCents(cents: number | null): string {
    if (cents == null) return ''
    return `$${(cents / 100).toFixed(2)}`
  }

  function formatMileage(n: number | null): string {
    return n != null ? n.toLocaleString() + ' mi' : ''
  }

  function entryKey(entry: TimelineEntry): string {
    return entry.type + '-' + entry.data.id
  }

  function incidentLabel(c: string): string {
    return c.replace(/_/g, ' ')
  }

  function toggleExpand(key: string) {
    expandedId = expandedId === key ? null : key
    editing = false
    confirmDelete = false
  }

  function startEditIncident(inc: IncidentWithDetails) {
    editingIncident = inc
    showIncidentForm = true
    showServiceForm = false
  }

  // --- Service row editing (ported from HistoryTab) ---

  function startEdit(record: ServiceRecordWithLinks) {
    editing = true
    editDate = record.service_date
    editMileage = record.mileage != null ? String(record.mileage) : ''
    editDescription = record.description ?? ''
    editCostDollars = record.total_cost_cents != null ? (record.total_cost_cents / 100).toFixed(2) : ''
    editShopName = record.shop_name ?? ''
    editShopId = record.shop_id ?? null
    editNotes = record.notes ?? ''
    editPaidBy = record.paid_by
    editPayerNote = record.payer_note ?? ''
  }

  function cancelEdit() {
    editing = false
    confirmDelete = false
  }

  function shopForName(name: string): Shop | undefined {
    return shopList.find((s) => s.name.toLowerCase() === name.toLowerCase())
  }

  async function saveEdit(record: ServiceRecordWithLinks) {
    saving = true
    try {
      const costCents = editCostDollars ? Math.round(parseFloat(editCostDollars) * 100) : null
      const matchedShop = shopForName(editShopName)
      await servicesApi.update(vehicleId, record.id, {
        service_date: editDate,
        mileage: editMileage ? parseInt(editMileage) : null,
        description: editDescription || null,
        total_cost_cents: costCents,
        shop_name: editShopName || null,
        shop_id: matchedShop?.id ?? editShopId ?? null,
        notes: editNotes || null,
        paid_by: editPaidBy,
        // Explicit null clears a previously set note (double-option update).
        payer_note: editPaidBy !== 'self' && editPayerNote ? editPayerNote : null,
      })
      editing = false
      await refresh()
    } catch (e) {
      console.error('Failed to update service record:', e)
    } finally {
      saving = false
    }
  }

  async function deleteService(id: number) {
    deleting = true
    try {
      await servicesApi.delete(vehicleId, id)
      expandedId = null
      confirmDelete = false
      await refresh()
    } catch (e) {
      console.error('Failed to delete service record:', e)
    } finally {
      deleting = false
    }
  }
</script>

<div class="timeline">
  <div class="tab-header">
    <div class="filter-bar">
      <button class="filter-btn" class:active={filter === 'all'} onclick={() => (filter = 'all')}>All</button>
      <button class="filter-btn" class:active={filter === 'services'} onclick={() => (filter = 'services')}>Services</button>
      <button class="filter-btn" class:active={filter === 'incidents'} onclick={() => { filter = 'incidents'; incidentCategory = 'all' }}>Incidents</button>
      <button class="filter-btn" class:active={filter === 'mileage'} onclick={() => (filter = 'mileage')}>Mileage</button>
    </div>
    <div class="create-actions">
      <button
        class="btn btn-secondary"
        onclick={() => { showIncidentForm = !showIncidentForm; editingIncident = null; showServiceForm = false }}
      >
        {showIncidentForm && !editingIncident ? 'Cancel' : 'Log incident'}
      </button>
      <button
        class="btn btn-primary"
        onclick={() => { showServiceForm = !showServiceForm; showIncidentForm = false; editingIncident = null }}
      >
        {showServiceForm ? 'Cancel' : 'Record service'}
      </button>
    </div>
  </div>

  {#if filter === 'incidents'}
    <div class="filter-bar category-bar" data-testid="category-filter">
      <button class="filter-btn" class:active={incidentCategory === 'all'} onclick={() => (incidentCategory = 'all')}>All</button>
      {#each incidentCategories as c (c)}
        <button class="filter-btn" class:active={incidentCategory === c} onclick={() => (incidentCategory = c)}>
          {incidentLabel(c)}
        </button>
      {/each}
    </div>
  {/if}

  {#if showServiceForm}
    <ServiceForm
      {vehicleId}
      onComplete={async () => { showServiceForm = false; await refresh() }}
      onCancel={() => (showServiceForm = false)}
    />
  {/if}

  {#if showIncidentForm}
    <IncidentForm
      {vehicleId}
      incident={editingIncident}
      serviceRecords={services}
      {estimatedMileage}
      onComplete={async () => { showIncidentForm = false; editingIncident = null; await refresh() }}
      onCancel={() => { showIncidentForm = false; editingIncident = null }}
    />
  {/if}

  {#if loading}
    <p>Loading timeline...</p>
  {:else if entries.length === 0}
    <p class="empty">No history yet.</p>
  {:else}
    <div class="history-list">
      {#each visible as entry (entryKey(entry))}
        {@const key = entryKey(entry)}
        {@const isExpanded = expandedId === key}
        {#if entry.type === 'service'}
          {@const record = entry.data}
          <!-- One contiguous card: the wrapper owns the border; the row and
               its expanded panel are borderless halves inside it. -->
          <div class="service-entry" class:expanded={isExpanded} id={anchorId('service', record.id)}>
          <div
            class="history-card service-card"
            class:expanded={isExpanded}
            onclick={() => toggleExpand(key)}
            role="button"
            tabindex="0"
            onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); toggleExpand(key) } }}
          >
            <div class="history-header">
              <span class="type-badge service-badge">Service</span>
              <span class="date">{formatDate(record.service_date)}</span>
              {#if record.total_cost_cents}
                <span class="cost">{formatCents(record.total_cost_cents)}</span>
              {/if}
            </div>
            {#if record.description}
              <p class="description">{record.description}</p>
            {/if}
            <div class="meta">
              {#if record.mileage}
                <span>{formatMileage(record.mileage)}</span>
              {/if}
              {#if record.shop_name}
                <span>at {record.shop_name}</span>
              {/if}
            </div>
            {#if !isExpanded}
              {#if record.notes}
                <p class="notes">{record.notes}</p>
              {/if}
              {#if partsForService(record).length > 0}
                <div class="linked-items">
                  <span class="linked-label">Parts:</span>
                  {#each partsForService(record) as part (part.id)}
                    <span class="linked-chip part-chip">{part.name}</span>
                  {/each}
                </div>
              {/if}
              {#if incidentsForService(record.id).length > 0}
                <div class="linked-items">
                  <span class="linked-label">Incidents:</span>
                  {#each incidentsForService(record.id) as inc (inc.id)}
                    <span class="linked-chip obs-chip">{inc.title}</span>
                  {/each}
                </div>
              {/if}
            {/if}
          </div>
          {#if isExpanded}
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div class="expanded-panel" onclick={(e) => e.stopPropagation()}>
              {#if editing}
                <div class="edit-form">
                  <div class="form-row">
                    <div class="field">
                      <label for="edit-date-{record.id}">Date</label>
                      <input id="edit-date-{record.id}" type="date" bind:value={editDate} />
                    </div>
                    <div class="field">
                      <label for="edit-mileage-{record.id}">Mileage</label>
                      <input id="edit-mileage-{record.id}" type="number" bind:value={editMileage} min="0" />
                    </div>
                  </div>
                  <div class="field">
                    <label for="edit-desc-{record.id}">Description</label>
                    <input id="edit-desc-{record.id}" type="text" bind:value={editDescription} />
                  </div>
                  <div class="form-row">
                    <div class="field">
                      <label for="edit-cost-{record.id}">Total Cost ($)</label>
                      <input id="edit-cost-{record.id}" type="number" step="0.01" min="0" bind:value={editCostDollars} />
                    </div>
                    <div class="field">
                      <label for="edit-shop-{record.id}">Shop</label>
                      <input id="edit-shop-{record.id}" type="text" bind:value={editShopName} />
                    </div>
                  </div>
                  <div class="form-row">
                    <div class="field">
                      <label for="edit-paid-by-{record.id}">Paid By</label>
                      <select id="edit-paid-by-{record.id}" bind:value={editPaidBy}>
                        <option value="self">Me</option>
                        <option value="insurance">Insurance</option>
                        <option value="third_party">Third party</option>
                      </select>
                    </div>
                    {#if editPaidBy !== 'self'}
                      <div class="field">
                        <label for="edit-payer-note-{record.id}">Payer Note</label>
                        <input id="edit-payer-note-{record.id}" type="text" bind:value={editPayerNote} placeholder="e.g., Progressive claim #12345" />
                      </div>
                    {/if}
                  </div>
                  <div class="field">
                    <label for="edit-notes-{record.id}">Notes</label>
                    <textarea id="edit-notes-{record.id}" bind:value={editNotes} rows="2"></textarea>
                  </div>
                  <div class="edit-actions">
                    <button class="btn btn-secondary btn-sm" onclick={cancelEdit} disabled={saving}>Cancel</button>
                    <button class="btn btn-primary btn-sm" onclick={() => saveEdit(record)} disabled={saving}>
                      {saving ? 'Saving...' : 'Save'}
                    </button>
                  </div>
                </div>
              {:else}
                <div class="detail-section">
                  {#if record.parts_cost_cents != null}
                    <span><strong>Parts cost:</strong> {formatCents(record.parts_cost_cents)}</span>
                  {/if}
                  {#if record.labor_cost_cents != null}
                    <span><strong>Labor cost:</strong> {formatCents(record.labor_cost_cents)}</span>
                  {/if}
                  {#if record.paid_by !== 'self'}
                    <span>
                      <strong>Paid by:</strong>
                      {record.paid_by === 'insurance' ? 'Insurance' : 'Third party'}{record.payer_note ? ` — ${record.payer_note}` : ''}
                    </span>
                  {/if}
                  {#if record.notes}
                    <p class="notes">{record.notes}</p>
                  {/if}
                  {#if partsForService(record).length > 0}
                    <div class="linked-items">
                      <span class="linked-label">Parts:</span>
                      {#each partsForService(record) as part (part.id)}
                        <span class="linked-chip part-chip">{part.name}</span>
                      {/each}
                    </div>
                  {/if}
                  {#if incidentsForService(record.id).length > 0}
                    <div class="linked-items">
                      <span class="linked-label">Incidents:</span>
                      {#each incidentsForService(record.id) as inc (inc.id)}
                        <span class="linked-chip obs-chip">{inc.title}</span>
                      {/each}
                    </div>
                  {/if}
                </div>
                <div class="expand-actions">
                  <button class="btn btn-secondary btn-sm" onclick={() => startEdit(record)}>Edit</button>
                  {#if confirmDelete}
                    <span class="confirm-text">Delete this record?</span>
                    <button class="btn btn-danger btn-sm" onclick={() => deleteService(record.id)} disabled={deleting}>
                      {deleting ? 'Deleting...' : 'Yes, Delete'}
                    </button>
                    <button class="btn btn-secondary btn-sm" onclick={() => (confirmDelete = false)}>Cancel</button>
                  {:else}
                    <button class="btn btn-danger-outline btn-sm" onclick={() => (confirmDelete = true)}>Delete</button>
                  {/if}
                </div>
              {/if}
            </div>
          {/if}
          </div>
        {:else if entry.type === 'incident'}
          {@const inc = entry.data}
          <div class="history-card obs-card" class:resolved={inc.resolved} class:expanded={isExpanded} id={anchorId('incident', inc.id)}>
            <div
              class="inc-header"
              role="button"
              tabindex="0"
              onclick={() => toggleExpand(key)}
              onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); toggleExpand(key) } }}
            >
              <span class="type-badge obs-badge">Incident</span>
              <span class="date">{formatDate(inc.occurred_at)}</span>
              <span class="inc-title">{inc.title}</span>
              <span class="category">{incidentLabel(inc.category)}</span>
              {#if inc.resolved}
                <span class="resolved-badge">Resolved</span>
              {/if}
              <span class="expand-icon">{isExpanded ? '▲' : '▼'}</span>
            </div>
            {#if !isExpanded && inc.description}
              <p class="notes">{inc.description}</p>
            {/if}
            {#if isExpanded}
              <IncidentDetail
                {vehicleId}
                incident={inc}
                serviceRecords={services}
                onEdit={startEditIncident}
                onChanged={refresh}
              />
            {/if}
          </div>
        {:else}
          {@const log = entry.data}
          <div class="history-card mileage-card" id={anchorId('mileage', log.id)}>
            <div class="history-header">
              <span class="type-badge mileage-badge">Mileage</span>
              <span class="date">{formatDate(log.recorded_at)}</span>
              <span class="cost">{formatMileage(log.mileage)}</span>
            </div>
            {#if log.notes}
              <p class="notes">{log.notes}</p>
            {/if}
          </div>
        {/if}
      {/each}
    </div>

    {#if filtered.length > limit}
      <button class="load-more" onclick={() => (limit += PAGE)}>
        Load more ({filtered.length - limit} older)
      </button>
    {/if}
  {/if}
</div>

<style>
  .tab-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: var(--sp-3);
    flex-wrap: wrap;
    margin-bottom: var(--sp-4);
  }

  .create-actions {
    display: flex;
    gap: var(--sp-2);
  }

  .filter-bar {
    display: flex; gap: var(--sp-1);
    border: 1px solid var(--border-subtle); border-radius: var(--radius-md); overflow: hidden; width: fit-content;
  }

  .filter-btn {
    padding: var(--sp-1) var(--sp-3); border: none; background: none;
    font-family: var(--font-display); font-size: 0.85rem; cursor: pointer; color: var(--text-muted);
    transition: background var(--duration-fast) var(--ease-out), color var(--duration-fast) var(--ease-out);
  }

  .filter-btn.active {
    background: var(--primary); color: var(--primary-text);
  }

  .category-bar {
    margin-bottom: var(--sp-4);
    flex-wrap: wrap;
  }

  .category-bar .filter-btn {
    font-size: 0.78rem;
    text-transform: capitalize;
  }

  .history-list { display: flex; flex-direction: column; gap: var(--sp-2); }

  .history-card {
    padding: var(--sp-3) var(--sp-4); border: 1px solid var(--border-subtle); border-radius: var(--radius-md);
    background: var(--bg-raised);
    transition:
      border-color var(--duration-base) var(--ease-out),
      box-shadow var(--duration-base) var(--ease-out);
  }

  .service-card { cursor: pointer; }

  .history-card:hover {
    border-color: var(--border);
    box-shadow: var(--shadow-sm);
  }

  .history-card.expanded {
    border-color: var(--primary);
  }

  /* Service entries wrap the row + its expanded panel so the expanded
     state reads as ONE contiguous card: the border lives on the wrapper,
     never on both halves. */
  .service-entry {
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    background: var(--bg-raised);
    transition:
      border-color var(--duration-base) var(--ease-out),
      box-shadow var(--duration-base) var(--ease-out);
  }

  .service-entry:hover {
    border-color: var(--border);
    box-shadow: var(--shadow-sm);
  }

  .service-entry.expanded {
    border-color: var(--primary);
  }

  .service-entry .service-card,
  .service-entry .service-card:hover,
  .service-entry .service-card.expanded {
    border: none;
    box-shadow: none;
    background: none;
  }

  .history-card.resolved { opacity: 0.6; }

  .history-header {
    display: flex; align-items: center; gap: var(--sp-2);
  }

  .inc-header {
    display: flex; align-items: center; gap: var(--sp-2);
    cursor: pointer; flex-wrap: wrap;
    margin: calc(-1 * var(--sp-3)) calc(-1 * var(--sp-4)) 0;
    padding: var(--sp-3) var(--sp-4);
  }

  .obs-card.expanded .inc-header {
    margin-bottom: 0;
  }

  .type-badge {
    font-family: var(--font-display);
    font-size: 0.7rem; text-transform: uppercase; letter-spacing: 0.05em;
    padding: 0.1rem 0.4rem; border-radius: var(--radius-sm); font-weight: 600;
  }

  .service-badge { background: var(--success-bg); color: var(--success); }
  .obs-badge { background: var(--warning-bg); color: var(--warning); }
  .mileage-badge { background: var(--info-bg); color: var(--info); }
  .resolved-badge { font-size: 0.75rem; color: var(--success); }

  .date { font-weight: 600; }
  .cost { margin-left: auto; font-weight: 600; }
  .description { margin: var(--sp-1) 0; }
  .inc-title { font-weight: 600; flex: 1; }
  .meta { font-size: 0.85rem; color: var(--text-muted); display: flex; gap: var(--sp-3); }
  .category { text-transform: capitalize; font-size: 0.8rem; color: var(--text-muted); }
  .notes { font-size: 0.85rem; color: var(--text-muted); margin: var(--sp-1) 0 0; font-style: italic; }
  .empty { color: var(--text-muted); text-align: center; padding: var(--sp-8) 0; }
  .expand-icon { font-size: 0.7rem; color: var(--text-muted); }

  .linked-items {
    display: flex; flex-wrap: wrap; align-items: center; gap: var(--sp-1);
    margin-top: var(--sp-2); font-size: 0.8rem;
  }

  .linked-label {
    font-weight: 600; color: var(--text-muted); font-size: 0.75rem;
    text-transform: uppercase; letter-spacing: 0.03em;
  }

  .linked-chip {
    padding: 0.1rem 0.5rem; border-radius: var(--radius-sm); font-size: 0.8rem;
  }

  .part-chip { background: var(--success-bg); color: var(--success); }
  .obs-chip { background: var(--warning-bg); color: var(--warning); }

  /* Expanded panel (service rows): a borderless bottom half of the
     wrapping .service-entry, separated by a hairline. */
  .expanded-panel {
    border-top: 1px solid var(--border-subtle);
    background: none;
    padding: var(--sp-3) var(--sp-4);
  }

  .detail-section {
    display: flex; flex-direction: column; gap: var(--sp-2);
    margin-bottom: var(--sp-3);
    font-size: 0.85rem;
  }

  .expand-actions {
    display: flex; align-items: center; gap: var(--sp-2);
    padding-top: var(--sp-2);
    border-top: 1px solid var(--border-subtle);
  }

  .confirm-text {
    font-size: 0.8rem;
    color: var(--danger);
    font-weight: 500;
  }

  .btn-danger {
    background: var(--danger);
    color: white;
    border: 1px solid var(--danger);
  }

  .btn-danger:hover {
    opacity: 0.9;
  }

  .btn-danger-outline {
    background: none;
    color: var(--danger);
    border: 1px solid var(--danger);
  }

  .btn-danger-outline:hover {
    background: var(--danger-bg);
  }

  .btn-sm {
    font-size: 0.75rem;
    padding: var(--sp-1) var(--sp-2);
  }

  /* Edit form */
  .edit-form {
    display: flex; flex-direction: column; gap: var(--sp-2);
  }

  .form-row {
    display: flex; gap: var(--sp-3);
  }

  .form-row .field {
    flex: 1;
  }

  .field {
    display: flex; flex-direction: column; gap: var(--sp-1);
  }

  .field label {
    font-size: 0.75rem; font-weight: 600; color: var(--text-muted);
    text-transform: uppercase; letter-spacing: 0.03em;
  }

  .field input, .field select, .field textarea {
    padding: var(--sp-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--bg);
    font-size: 0.85rem;
  }

  .edit-actions {
    display: flex; justify-content: flex-end; gap: var(--sp-2);
    margin-top: var(--sp-2);
  }

  .load-more {
    display: block;
    margin: var(--sp-4) auto 0;
    padding: var(--sp-2) var(--sp-4);
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    color: var(--text-secondary);
    font-size: 0.85rem;
    cursor: pointer;
    transition: border-color var(--duration-fast) var(--ease-out);
  }

  .load-more:hover {
    border-color: var(--text-muted);
    color: var(--text);
  }
</style>
