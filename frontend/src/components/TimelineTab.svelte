<script lang="ts">
  // Timeline (unit F): ONE chronological stream subsuming the retired
  // History and Incidents tabs — services + incidents + manual odometer
  // readings, newest first, with kind filters. Service rows expand into
  // the service card's detail/edit UI (ported from HistoryTab); incident
  // rows expand into IncidentDetail (ported from IncidentsTab). Creation
  // lives here too: Record service (existing ServiceForm) and Log
  // incident (IncidentForm).
  import { onMount } from 'svelte'
  import { link, querystring, replace } from '@keenmate/svelte-spa-router'
  import {
    services as servicesApi,
    incidents as incidentsApi,
    parts as partsApi,
    shops as shopsApi,
    mileage as mileageApi,
    schedules as schedulesApi,
    documents as documentsApi,
  } from '../lib/api'
  import { anchorId, flashHighlightFromQuery } from '../lib/highlight'
  import type {
    ServiceRecordWithLinks,
    IncidentWithDetails,
    MileageEntry,
    Part,
    Document,
    DocumentDisposition,
    ResolvedScheduleItem,
    ServicePrefill,
    Shop,
  } from '../lib/types'
  import { formatDate } from '../lib/dates'
  import { formatCents as formatCentsShared } from '../lib/money'
  import ConfirmDelete from './ConfirmDelete.svelte'
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
  // All vehicle documents, batch-loaded once (no N+1) and filtered per service
  // row by linked_entity — so a freshly-attached invoice is visible here.
  let allDocs: Document[] = $state([])
  let shopList: Shop[] = $state([])
  let scheduleItems: ResolvedScheduleItem[] = $state([])
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
  // Prefill carried by the routed record-service flow (?action=record with
  // optional &schedule_item=&desc=&retro= from Plan → Due): the Due
  // actions land on THIS one real form, prefilled, instead of stripped
  // mini-forms (round-2 feedback #9).
  let servicePrefill: ServicePrefill | null = $state(null)

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

  async function loadData() {
    try {
      const [svcList, incidentList, mileageList, partsList, shops, schedule, docList] = await Promise.all([
        servicesApi.list(vehicleId),
        incidentsApi.list(vehicleId),
        mileageApi.list(vehicleId),
        partsApi.list(vehicleId),
        shopsApi.list(),
        schedulesApi.resolve(vehicleId),
        documentsApi.list({ vehicle_id: vehicleId }),
      ])
      services = svcList
      incidents = incidentList
      scheduleItems = schedule
      // Service-created logs are excluded: the service row already shows
      // that odometer reading (same rule as the backend activity feed).
      mileageLogs = mileageList.filter((m) => m.service_record_id == null)
      allParts = partsList
      allDocs = docList
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

  // The context strip's "Record service" and Plan → Due's actions route
  // here with ?action=record (+ optional prefill params) so there is ONE
  // service form; reacting to the querystring also covers clicking it
  // while already on the Timeline.
  $effect(() => {
    const qs = new URLSearchParams(querystring() ?? '')
    if (qs.get('action') === 'record') {
      const scheduleItem = qs.get('schedule_item')
      servicePrefill = {
        description: qs.get('desc') ?? undefined,
        scheduleItemId: scheduleItem ? parseInt(scheduleItem, 10) : undefined,
        retro: qs.get('retro') === '1',
      }
      showServiceForm = true
      showIncidentForm = false
      editingIncident = null
      // Consume the params: pushing an IDENTICAL hash fires no hashchange,
      // so leaving them in place makes the strip's second click dead and
      // makes a refresh re-open the form.
      replace(`/vehicles/${vehicleId}/timeline`)
    }
  })

  function closeServiceForm() {
    showServiceForm = false
    servicePrefill = null
  }

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

  // Category chips show only categories PRESENT in the data, with counts
  // (round-2 feedback #8) — not the whole static whitelist. Canonical
  // order is kept so chips don't jump around as data changes.
  let categoryCounts = $derived.by(() => {
    const counts = new Map<string, number>()
    for (const i of incidents) counts.set(i.category, (counts.get(i.category) ?? 0) + 1)
    return incidentCategories
      .filter((c) => counts.has(c))
      .map((c) => ({ category: c, count: counts.get(c)! }))
  })

  // If the selected category disappears (deletion, recategorized), fall
  // back to All rather than filtering to an empty stream.
  $effect(() => {
    if (incidentCategory !== 'all' && !categoryCounts.some((x) => x.category === incidentCategory)) {
      incidentCategory = 'all'
    }
  })

  let visible = $derived(filtered.slice(0, limit))

  function partsForService(service: ServiceRecordWithLinks): Part[] {
    if (!service.part_ids.length) return []
    const ids = new Set(service.part_ids)
    return allParts.filter((p) => ids.has(p.id))
  }

  function incidentsForService(serviceId: number): IncidentWithDetails[] {
    return incidents.filter((i) => i.service_record_ids.includes(serviceId))
  }

  function documentsForService(serviceId: number): Document[] {
    return allDocs.filter(
      (d) => d.linked_entity_type === 'service' && d.linked_entity_id === serviceId,
    )
  }

  // --- Maintenance-item linking from the record side (mirror of the Due
  // tab's "Link existing service…" picker, pointed the other way). ---

  function scheduleItemName(itemId: number): string {
    return (
      scheduleItems.find((s) => s.effective_item.id === itemId)?.effective_item.name ??
      `Item #${itemId}`
    )
  }

  let showMaintenancePicker = $state(false)
  let linkingItemId: number | null = $state(null)
  let linkError = $state('')

  async function linkMaintenanceItem(record: ServiceRecordWithLinks, itemId: number) {
    linkError = ''
    linkingItemId = itemId
    try {
      if (!record.schedule_item_ids.includes(itemId)) {
        // Union write — same semantics as the Due-side picker (the PUT
        // replaces links wholesale, so send existing + the new one).
        await servicesApi.update(vehicleId, record.id, {
          schedule_item_ids: [...record.schedule_item_ids, itemId],
        })
      }
      showMaintenancePicker = false
      await refresh()
    } catch (e: any) {
      linkError = e.message
    } finally {
      linkingItemId = null
    }
  }

  function formatCents(cents: number | null): string {
    if (cents == null) return ''
    return formatCentsShared(cents)
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
    showMaintenancePicker = false
    linkError = ''
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

  // No catch: a failure must propagate to ConfirmDelete, which keeps the
  // confirm row open and shows the error. Collapse LAST — nulling expandedId
  // first would unmount ConfirmDelete mid-run and lose any late error.
  async function deleteService(id: number, documents: DocumentDisposition) {
    await servicesApi.delete(vehicleId, id, documents)
    await refresh()
    expandedId = null
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
    <!-- ONE record-service verb per screen (round-2 feedback #7): the
         context strip owns it. This toolbar keeps the kind filters and
         the incident verb only. -->
    <div class="create-actions">
      <button
        class="btn btn-secondary"
        onclick={() => { showIncidentForm = !showIncidentForm; editingIncident = null; closeServiceForm() }}
      >
        {showIncidentForm && !editingIncident ? 'Cancel' : 'Log incident'}
      </button>
    </div>
  </div>

  {#if filter === 'incidents' && categoryCounts.length > 0}
    <div class="filter-bar category-bar" data-testid="category-filter">
      <button class="filter-btn" class:active={incidentCategory === 'all'} onclick={() => (incidentCategory = 'all')}>
        All <span class="chip-count num">{incidents.length}</span>
      </button>
      {#each categoryCounts as { category, count } (category)}
        <button class="filter-btn" class:active={incidentCategory === category} onclick={() => (incidentCategory = category)}>
          {incidentLabel(category)} <span class="chip-count num">{count}</span>
        </button>
      {/each}
    </div>
  {/if}

  {#if showServiceForm}
    {#key servicePrefill}
      <ServiceForm
        {vehicleId}
        prefill={servicePrefill}
        onComplete={async () => { closeServiceForm(); await refresh() }}
        onCancel={closeServiceForm}
      />
    {/key}
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
    <div class="history-list ledger">
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
                  {#if record.schedule_item_ids.length > 0}
                    <div class="linked-items">
                      <span class="linked-label">Maintenance:</span>
                      {#each record.schedule_item_ids as itemId (itemId)}
                        <!-- Hypermedia: the linked item deep-links to its Due
                             row (?hl= flashes it). -->
                        <a
                          class="linked-chip sched-chip"
                          href="/vehicles/{vehicleId}/plan/due?hl=schedule_item:{itemId}"
                          use:link
                          title="View this maintenance item under Plan → Due"
                        >
                          {scheduleItemName(itemId)}
                        </a>
                      {/each}
                    </div>
                  {/if}
                  {#if documentsForService(record.id).length > 0}
                    <div class="linked-items">
                      <span class="linked-label">Documents:</span>
                      {#each documentsForService(record.id) as doc (doc.id)}
                        <!-- Hypermedia: the chip opens the file (no dead-end
                             facts). Attached invoices/receipts land here. -->
                        <a
                          class="linked-chip doc-chip"
                          href="/files/{doc.file_path}"
                          target="_blank"
                          rel="noopener"
                          title="Open {doc.title || doc.file_name}"
                        >
                          {doc.title || doc.file_name}
                        </a>
                      {/each}
                    </div>
                  {/if}
                  {#if linkError}
                    <p class="link-error">{linkError}</p>
                  {/if}
                  {#if showMaintenancePicker}
                    <!-- Compact picker mirroring the Due tab's link picker:
                         the vehicle's schedule items; choosing one links this
                         record to it (union write) and the reminder clears. -->
                    <div class="link-picker" data-testid="maintenance-picker">
                      <div class="picker-label">Link “{record.description || 'this service'}” to a maintenance item</div>
                      {#if scheduleItems.length === 0}
                        <p class="picker-empty">No maintenance schedule on this vehicle yet.</p>
                      {:else}
                        <div class="picker-rows">
                          {#each scheduleItems as item (item.effective_item.id)}
                            {@const alreadyLinked = record.schedule_item_ids.includes(item.effective_item.id)}
                            <button
                              class="picker-row"
                              disabled={alreadyLinked || linkingItemId != null}
                              onclick={() => linkMaintenanceItem(record, item.effective_item.id)}
                            >
                              <span class="picker-desc">{item.effective_item.name}</span>
                              <span class="picker-interval">
                                {#if item.effective_item.interval_miles}{item.effective_item.interval_miles.toLocaleString()} mi{/if}
                                {#if item.effective_item.interval_miles && item.effective_item.interval_months}&nbsp;/&nbsp;{/if}
                                {#if item.effective_item.interval_months}{item.effective_item.interval_months} mo{/if}
                              </span>
                              {#if alreadyLinked}
                                <span class="picker-linked">linked</span>
                              {:else if linkingItemId === item.effective_item.id}
                                <span class="picker-linking">linking…</span>
                              {/if}
                            </button>
                          {/each}
                        </div>
                      {/if}
                    </div>
                  {/if}
                </div>
                <div class="expand-actions">
                  <button class="btn btn-secondary btn-sm" onclick={() => startEdit(record)}>Edit</button>
                  <button class="btn btn-secondary btn-sm" onclick={() => { showMaintenancePicker = !showMaintenancePicker; linkError = '' }}>
                    {showMaintenancePicker ? 'Close picker' : 'Link to maintenance item…'}
                  </button>
                  <ConfirmDelete
                    getDocCount={() => documentsApi.countFor('service', record.id)}
                    onDelete={(docs) => deleteService(record.id, docs)}
                  />
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

  /* Segmented control — the house filter language. */
  .filter-bar {
    display: flex; gap: 2px;
    padding: 2px;
    background: var(--surface);
    border: 1px solid var(--border-subtle);
    border-radius: 999px;
    width: fit-content;
  }

  .filter-btn {
    padding: 0.2rem var(--sp-3); border: none; background: none;
    border-radius: 999px;
    font-family: var(--font-display); font-size: 0.88rem; font-weight: 600;
    letter-spacing: 0.05em; text-transform: uppercase;
    cursor: pointer; color: var(--text-muted);
    transition: background var(--duration-fast) var(--ease-out), color var(--duration-fast) var(--ease-out);
  }

  .filter-btn:hover:not(.active) {
    color: var(--text);
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

  .chip-count {
    font-size: 0.72em;
    opacity: 0.75;
  }

  /* The stream is ONE ledger (round-2 feedback #5): hairline-ruled rows
     inside a single card — the same grammar as attention and Due — with
     a status rail instead of per-row borders. */
  .history-list {
    display: flex;
    flex-direction: column;
  }

  .history-card,
  .service-entry {
    border-left: 3px solid transparent;
    transition: background var(--duration-fast) var(--ease-out);
  }

  .history-card {
    padding: var(--sp-3) var(--sp-4);
  }

  .service-card { cursor: pointer; }

  .history-card:hover,
  .service-entry:hover {
    background: var(--surface);
  }

  /* Expanded rows anchor themselves with the signal rail + a raised wash. */
  .service-entry.expanded {
    border-left-color: var(--primary);
    background: var(--surface);
  }

  .history-card.obs-card.expanded {
    border-left-color: var(--warning);
    background: var(--surface);
  }

  .service-entry .service-card {
    border: none;
    padding: var(--sp-3) var(--sp-4);
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
    font-size: 0.7rem; text-transform: uppercase; letter-spacing: 0.08em;
    padding: 0.1rem 0.5rem; border-radius: 999px; font-weight: 600;
  }

  .service-badge { background: var(--success-bg); color: var(--success); border: 1px solid var(--success-border); }
  .obs-badge { background: var(--warning-bg); color: var(--warning); border: 1px solid var(--warning-border); }
  .mileage-badge { background: var(--info-bg); color: var(--info); border: 1px solid var(--info-border); }
  .resolved-badge { font-size: 0.75rem; color: var(--success); }

  .date {
    font-variant-numeric: tabular-nums;
    font-size: 0.82rem;
    color: var(--text-secondary);
  }
  .cost {
    margin-left: auto;
    font-variant-numeric: tabular-nums;
    font-size: 0.9rem;
    font-weight: 700;
  }
  .description { margin: var(--sp-1) 0; font-weight: 500; font-size: 0.95rem; }
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
    padding: 0.1rem 0.55rem; border-radius: 999px; font-size: 0.78rem;
  }

  .part-chip { background: var(--success-bg); color: var(--success); border: 1px solid var(--success-border); }
  .obs-chip { background: var(--warning-bg); color: var(--warning); border: 1px solid var(--warning-border); }
  .sched-chip {
    background: var(--info-bg); color: var(--info); border: 1px solid var(--info-border);
    text-decoration: none;
  }
  .sched-chip:hover { text-decoration: underline; }
  .doc-chip {
    background: var(--surface); color: var(--text-secondary); border: 1px solid var(--border);
    text-decoration: none;
  }
  .doc-chip:hover { text-decoration: underline; color: var(--text); }

  /* Compact link-to-maintenance picker — mirrors the Due tab's link-existing
     picker (name | interval instead of date | description | miles). */
  .link-picker {
    margin-top: var(--sp-2);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    background: var(--bg);
    overflow: hidden;
    animation: fade-in-down var(--duration-base) var(--ease-out) both;
  }

  .picker-label {
    font-family: var(--font-display);
    font-size: 0.72rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.1em;
    color: var(--text-muted);
    padding: var(--sp-2) var(--sp-3) var(--sp-1);
  }

  .picker-rows {
    display: flex;
    flex-direction: column;
    max-height: 220px;
    overflow-y: auto;
  }

  .picker-row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) max-content max-content;
    column-gap: var(--sp-3);
    align-items: baseline;
    width: 100%;
    padding: var(--sp-1) var(--sp-3);
    border: none;
    border-top: 1px dotted var(--border-subtle);
    background: none;
    color: var(--text-secondary);
    font-size: 0.82rem;
    text-align: left;
    cursor: pointer;
    transition:
      background var(--duration-fast) var(--ease-out),
      color var(--duration-fast) var(--ease-out);
  }

  .picker-row:hover:not(:disabled) {
    background: var(--surface);
    color: var(--text);
  }

  .picker-row:disabled {
    cursor: default;
    opacity: 0.55;
  }

  .picker-desc {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .picker-interval {
    white-space: nowrap;
    text-align: right;
    font-variant-numeric: tabular-nums;
    color: var(--text-muted);
  }

  .picker-linked,
  .picker-linking {
    font-family: var(--font-display);
    font-size: 0.66rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.07em;
    padding: 0 var(--sp-2);
    border-radius: 999px;
    background: var(--success-bg);
    color: var(--success);
    border: 1px solid var(--success-border);
  }

  .picker-linking {
    background: var(--surface);
    color: var(--text-muted);
    border-color: var(--border-subtle);
  }

  .picker-empty {
    margin: 0;
    padding: var(--sp-2) var(--sp-3) var(--sp-3);
    font-size: 0.82rem;
    color: var(--text-muted);
  }

  .link-error {
    color: var(--danger);
    font-size: 0.82rem;
    margin: var(--sp-1) 0 0;
  }

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
    padding: var(--sp-2) var(--sp-5);
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 999px;
    color: var(--text-secondary);
    font-family: var(--font-display);
    font-size: 0.85rem;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    cursor: pointer;
    transition: border-color var(--duration-fast) var(--ease-out);
  }

  .load-more:hover {
    border-color: var(--text-muted);
    color: var(--text);
  }
</style>
