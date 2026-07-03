<script lang="ts">
  import { onMount } from 'svelte'
  import { services as servicesApi, incidents as incidentsApi, parts as partsApi, shops as shopsApi } from '../lib/api'
  import type { ServiceRecordWithLinks, IncidentWithDetails, Part, Shop } from '../lib/types'
  import { formatDate } from '../lib/dates'

  let { vehicleId }: { vehicleId: number } = $props()

  type TimelineEntry =
    | { type: 'service'; date: string; data: ServiceRecordWithLinks }
    | { type: 'incident'; date: string; data: IncidentWithDetails }

  let entries: TimelineEntry[] = $state([])
  let allParts: Part[] = $state([])
  let allIncidents: IncidentWithDetails[] = $state([])
  let shopList: Shop[] = $state([])
  let loading = $state(true)
  let filter = $state<'all' | 'services' | 'incidents'>('all')

  // Expanded/editing state
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

  onMount(async () => {
    try {
      const [svcList, incidentList, partsList, shops] = await Promise.all([
        servicesApi.list(vehicleId),
        incidentsApi.list(vehicleId),
        partsApi.list(vehicleId),
        shopsApi.list(),
      ])

      allParts = partsList
      allIncidents = incidentList
      shopList = shops

      const timeline: TimelineEntry[] = [
        ...svcList.map((s): TimelineEntry => ({ type: 'service', date: s.service_date, data: s })),
        ...incidentList.map((i): TimelineEntry => ({ type: 'incident', date: i.occurred_at, data: i })),
      ]
      timeline.sort((a, b) => b.date.localeCompare(a.date))
      entries = timeline
    } catch (e) {
      console.error(e)
    } finally {
      loading = false
    }
  })

  function partsForService(service: ServiceRecordWithLinks): Part[] {
    if (!service.part_ids.length) return []
    const ids = new Set(service.part_ids)
    return allParts.filter(p => ids.has(p.id))
  }

  function incidentsForService(serviceId: number): IncidentWithDetails[] {
    return allIncidents.filter(i => i.service_record_ids.includes(serviceId))
  }

  let filtered = $derived(
    filter === 'all' ? entries :
    filter === 'services' ? entries.filter(e => e.type === 'service') :
    entries.filter(e => e.type === 'incident')
  )

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

  function toggleExpand(key: string) {
    if (expandedId === key) {
      expandedId = null
      editing = false
      confirmDelete = false
    } else {
      expandedId = key
      editing = false
      confirmDelete = false
    }
  }

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
    return shopList.find(s => s.name.toLowerCase() === name.toLowerCase())
  }

  async function saveEdit(record: ServiceRecordWithLinks) {
    saving = true
    try {
      const costCents = editCostDollars ? Math.round(parseFloat(editCostDollars) * 100) : null
      const matchedShop = shopForName(editShopName)
      const updated = await servicesApi.update(vehicleId, record.id, {
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
      // Update in entries
      entries = entries.map(e =>
        e.type === 'service' && e.data.id === record.id
          ? { type: 'service', date: updated.service_date, data: updated }
          : e
      )
      editing = false
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
      entries = entries.filter(e => !(e.type === 'service' && e.data.id === id))
      expandedId = null
      confirmDelete = false
    } catch (e) {
      console.error('Failed to delete service record:', e)
    } finally {
      deleting = false
    }
  }
</script>

{#if loading}
  <p>Loading history...</p>
{:else if entries.length === 0}
  <p class="empty">No history yet.</p>
{:else}
  <div class="filter-bar">
    <button class="filter-btn" class:active={filter === 'all'} onclick={() => (filter = 'all')}>All</button>
    <button class="filter-btn" class:active={filter === 'services'} onclick={() => (filter = 'services')}>Services</button>
    <button class="filter-btn" class:active={filter === 'incidents'} onclick={() => (filter = 'incidents')}>Incidents</button>
  </div>

  <div class="history-list">
    {#each filtered as entry (entryKey(entry))}
      {@const key = entryKey(entry)}
      {@const isExpanded = expandedId === key}
      {#if entry.type === 'service'}
        {@const record = entry.data}
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
      {:else}
        {@const inc = entry.data}
        <div class="history-card obs-card" class:resolved={inc.resolved}>
          <div class="history-header">
            <span class="type-badge obs-badge">Incident</span>
            <span class="date">{formatDate(inc.occurred_at)}</span>
            {#if inc.resolved}
              <span class="resolved-badge">Resolved</span>
            {/if}
          </div>
          <p class="description">{inc.title}</p>
          {#if inc.description}
            <p class="notes">{inc.description}</p>
          {/if}
          <div class="meta">
            {#if inc.odometer}
              <span>{formatMileage(inc.odometer)}</span>
            {/if}
            <span class="category">{inc.category.replace(/_/g, ' ')}</span>
          </div>
        </div>
      {/if}
    {/each}
  </div>
{/if}

<style>
  .filter-bar {
    display: flex; gap: var(--sp-1); margin-bottom: var(--sp-4);
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

  .history-list { display: flex; flex-direction: column; gap: var(--sp-2); }

  .history-card {
    padding: var(--sp-3) var(--sp-4); border: 1px solid var(--border-subtle); border-radius: var(--radius-md);
    background: var(--bg-raised); cursor: pointer;
    transition:
      border-color var(--duration-base) var(--ease-out),
      box-shadow var(--duration-base) var(--ease-out),
      transform var(--duration-base) var(--ease-out);
  }

  .history-card:hover {
    border-color: var(--border);
    box-shadow: var(--shadow-sm);
    transform: translateY(-1px);
  }

  .history-card.expanded {
    border-color: var(--primary);
    border-bottom-left-radius: 0;
    border-bottom-right-radius: 0;
  }

  .history-card.resolved { opacity: 0.6; }

  .history-header {
    display: flex; align-items: center; gap: var(--sp-2);
  }

  .type-badge {
    font-family: var(--font-display);
    font-size: 0.7rem; text-transform: uppercase; letter-spacing: 0.05em;
    padding: 0.1rem 0.4rem; border-radius: var(--radius-sm); font-weight: 600;
  }

  .service-badge { background: var(--success-bg); color: var(--success); }
  .obs-badge { background: var(--warning-bg); color: var(--warning); }
  .resolved-badge { font-size: 0.75rem; color: var(--success); }

  .date { font-weight: 600; }
  .cost { margin-left: auto; font-weight: 600; }
  .description { margin: var(--sp-1) 0; }
  .meta { font-size: 0.85rem; color: var(--text-muted); display: flex; gap: var(--sp-3); }
  .category { text-transform: capitalize; }
  .notes { font-size: 0.85rem; color: var(--text-muted); margin: var(--sp-1) 0 0; font-style: italic; }
  .empty { color: var(--text-muted); text-align: center; padding: var(--sp-8) 0; }

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

  /* Expanded panel */
  .expanded-panel {
    border: 1px solid var(--primary);
    border-top: none;
    border-bottom-left-radius: var(--radius-md);
    border-bottom-right-radius: var(--radius-md);
    background: var(--bg-raised);
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
    background: var(--bg-base);
    font-size: 0.85rem;
  }

  .edit-actions {
    display: flex; justify-content: flex-end; gap: var(--sp-2);
    margin-top: var(--sp-2);
  }
</style>
