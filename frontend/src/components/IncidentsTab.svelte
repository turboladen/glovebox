<script lang="ts">
  // Interim Incidents tab (2hea unit B): deliberately minimal — combined
  // list + category filter + create/edit form + followups + resolve toggle.
  // Unit F's Timeline is the real navigation home for incidents.
  import { onMount } from 'svelte'
  import { incidents as incidentsApi, services as servicesApi } from '../lib/api'
  import type { IncidentWithDetails, ServiceRecordWithLinks, UpdateIncident } from '../lib/types'
  import { formatDate } from '../lib/dates'

  let { vehicleId, estimatedMileage }: { vehicleId: number; estimatedMileage?: number } = $props()

  let items: IncidentWithDetails[] = $state([])
  let serviceRecords: ServiceRecordWithLinks[] = $state([])
  let loading = $state(true)
  let expandedId: number | null = $state(null)
  let filter = $state('all')

  const categories = [
    'general', 'noise', 'leak', 'warning_light', 'cosmetic',
    'performance', 'obd_code', 'damage', 'accident', 'note',
  ]

  // Form state
  let showForm = $state(false)
  let editingId: number | null = $state(null)
  let saving = $state(false)
  let formError = $state('')

  let category = $state('general')
  let title = $state('')
  let description = $state('')
  let occurredAt = $state(new Date().toISOString().slice(0, 10))
  let odometer = $state<number | undefined>(undefined)
  let odometerTouched = $state(false)
  $effect(() => {
    if (estimatedMileage !== undefined && !odometerTouched && !editingId) {
      odometer = estimatedMileage
    }
  })
  let obdCodes = $state('')
  let notes = $state('')
  // Accident-only fieldset
  let fault = $state('')
  let otherPartyName = $state('')
  let otherPartyPhone = $state('')
  let otherPartyEmail = $state('')
  let otherPartyInsurance = $state('')
  let otherPartyPolicy = $state('')
  let claimNumber = $state('')
  let adjuster = $state('')
  let adjusterPhone = $state('')
  // Financial fields (dollars as strings; edit-only, matching old AccidentsTab)
  let repairCost = $state('')
  let deductible = $state('')
  let payout = $state('')
  // Service linking: sending service_record_ids REPLACES the set server-side,
  // so the form appends the selected service to the links captured at edit time.
  let existingServiceLinks: number[] = $state([])
  let linkServiceId = $state<number | ''>('')
  let resolvingId: number | null = $state(null)

  const faultOptions = ['', 'at_fault', 'not_at_fault', 'shared', 'unknown']

  // Followup form
  let followupFor: number | null = $state(null)
  let fuDate = $state(new Date().toISOString().slice(0, 10))
  let fuMethod = $state('')
  let fuWith = $state('')
  let fuSummary = $state('')
  let fuNotes = $state('')
  let fuSaving = $state(false)

  const contactMethods = ['phone', 'email', 'in_person', 'mail', 'other']
  const contactWithOptions = ['insurance_adjuster', 'other_party', 'police', 'attorney', 'repair_shop', 'other']

  onMount(loadData)

  async function loadData() {
    try {
      const [list, svcList] = await Promise.all([
        incidentsApi.list(vehicleId),
        servicesApi.list(vehicleId),
      ])
      items = list
      serviceRecords = svcList
    } catch (e) {
      console.error(e)
    } finally {
      loading = false
    }
  }

  let filtered = $derived(filter === 'all' ? items : items.filter(i => i.category === filter))
  let presentCategories = $derived([...new Set(items.map(i => i.category))])

  function label(c: string): string {
    return c.replace(/_/g, ' ').replace(/\b\w/g, l => l.toUpperCase())
  }

  function serviceLabel(id: number): string {
    const svc = serviceRecords.find(s => s.id === id)
    return svc ? `${formatDate(svc.service_date)} — ${svc.description || 'Service'}` : `Service #${id}`
  }

  function resetForm() {
    editingId = null
    category = 'general'
    title = ''; description = ''; notes = ''; obdCodes = ''
    occurredAt = new Date().toISOString().slice(0, 10)
    odometer = estimatedMileage; odometerTouched = false
    fault = ''; otherPartyName = ''; otherPartyPhone = ''; otherPartyInsurance = ''
    otherPartyEmail = ''; otherPartyPolicy = ''
    claimNumber = ''; adjuster = ''; adjusterPhone = ''
    repairCost = ''; deductible = ''; payout = ''
    existingServiceLinks = []; linkServiceId = ''
    formError = ''
    showForm = false
  }

  function startAdd() {
    resetForm()
    showForm = true
  }

  function startEdit(inc: IncidentWithDetails) {
    editingId = inc.id
    category = inc.category
    title = inc.title
    description = inc.description ?? ''
    occurredAt = inc.occurred_at.split('T')[0].split(' ')[0]
    odometer = inc.odometer ?? undefined
    odometerTouched = true
    obdCodes = inc.obd_codes ?? ''
    notes = inc.notes ?? ''
    fault = inc.fault ?? ''
    otherPartyName = inc.other_party_name ?? ''
    otherPartyPhone = inc.other_party_phone ?? ''
    otherPartyEmail = inc.other_party_email ?? ''
    otherPartyInsurance = inc.other_party_insurance ?? ''
    otherPartyPolicy = inc.other_party_policy_number ?? ''
    claimNumber = inc.insurance_claim_number ?? ''
    adjuster = inc.insurance_adjuster ?? ''
    adjusterPhone = inc.insurance_adjuster_phone ?? ''
    repairCost = inc.total_repair_cost_cents !== null ? (inc.total_repair_cost_cents / 100).toFixed(2) : ''
    deductible = inc.deductible_cents !== null ? (inc.deductible_cents / 100).toFixed(2) : ''
    payout = inc.insurance_payout_cents !== null ? (inc.insurance_payout_cents / 100).toFixed(2) : ''
    existingServiceLinks = [...inc.service_record_ids]
    linkServiceId = ''
    formError = ''
    showForm = true
  }

  async function submit() {
    if (!title.trim()) { formError = 'Title is required'; return }
    saving = true
    formError = ''
    // Full replacement set: existing links (captured at edit time) plus the
    // optional newly-selected service.
    const linkedIds = [...new Set([...existingServiceLinks, ...(linkServiceId !== '' ? [linkServiceId] : [])])]
    try {
      if (editingId) {
        await incidentsApi.update(vehicleId, editingId, {
          category,
          title: title.trim(),
          description: description || null,
          occurred_at: occurredAt || undefined,
          odometer: odometer ?? null,
          obd_codes: obdCodes || null,
          notes: notes || null,
          fault: fault || null,
          other_party_name: otherPartyName || null,
          other_party_phone: otherPartyPhone || null,
          other_party_email: otherPartyEmail || null,
          other_party_insurance: otherPartyInsurance || null,
          other_party_policy_number: otherPartyPolicy || null,
          insurance_claim_number: claimNumber || null,
          insurance_adjuster: adjuster || null,
          insurance_adjuster_phone: adjusterPhone || null,
          total_repair_cost_cents: repairCost ? Math.round(parseFloat(repairCost) * 100) : null,
          deductible_cents: deductible ? Math.round(parseFloat(deductible) * 100) : null,
          insurance_payout_cents: payout ? Math.round(parseFloat(payout) * 100) : null,
          service_record_ids: linkedIds,
        })
      } else {
        await incidentsApi.create(vehicleId, {
          category,
          title: title.trim(),
          description: description || undefined,
          occurred_at: occurredAt || undefined,
          odometer,
          obd_codes: obdCodes || undefined,
          notes: notes || undefined,
          fault: fault || undefined,
          other_party_name: otherPartyName || undefined,
          other_party_phone: otherPartyPhone || undefined,
          other_party_email: otherPartyEmail || undefined,
          other_party_insurance: otherPartyInsurance || undefined,
          other_party_policy_number: otherPartyPolicy || undefined,
          insurance_claim_number: claimNumber || undefined,
          insurance_adjuster: adjuster || undefined,
          insurance_adjuster_phone: adjusterPhone || undefined,
          service_record_ids: linkedIds.length > 0 ? linkedIds : undefined,
        })
      }
      resetForm()
      await loadData()
    } catch (e: any) {
      formError = e.message
    } finally {
      saving = false
    }
  }

  async function toggleResolved(inc: IncidentWithDetails) {
    await incidentsApi.update(vehicleId, inc.id, { resolved: !inc.resolved })
    await loadData()
  }

  // Resolve, optionally linking a service at the same time (old ObservationsTab UX).
  async function resolveWith(inc: IncidentWithDetails, serviceId: number | null) {
    const payload: UpdateIncident = { resolved: true }
    if (serviceId !== null) {
      payload.service_record_ids = [...new Set([...inc.service_record_ids, serviceId])]
    }
    await incidentsApi.update(vehicleId, inc.id, payload)
    resolvingId = null
    await loadData()
  }

  function openFollowupForm(incidentId: number) {
    followupFor = incidentId
    fuDate = new Date().toISOString().slice(0, 10)
    fuMethod = ''; fuWith = ''; fuSummary = ''; fuNotes = ''
  }

  async function submitFollowup(incidentId: number) {
    if (!fuSummary.trim()) return
    fuSaving = true
    try {
      await incidentsApi.addFollowup(vehicleId, incidentId, {
        occurred_at: fuDate,
        contact_method: fuMethod || undefined,
        contact_with: fuWith || undefined,
        summary: fuSummary.trim(),
        notes: fuNotes || undefined,
      })
      followupFor = null
      await loadData()
    } catch (e: any) {
      console.error(e)
    } finally {
      fuSaving = false
    }
  }

  function parseObdCodes(raw: string): string[] {
    try {
      const parsed = JSON.parse(raw)
      return Array.isArray(parsed) ? parsed : [raw]
    } catch {
      return [raw]
    }
  }
</script>

<div class="incidents">
  <div class="tab-header">
    <h3>Incidents</h3>
    <button class="btn btn-primary" onclick={() => (showForm ? resetForm() : startAdd())}>
      {showForm ? 'Cancel' : '+ Add Incident'}
    </button>
  </div>

  {#if showForm}
    <div class="form-card">
      <form onsubmit={(e) => { e.preventDefault(); submit() }}>
        <div class="form-row">
          <div class="field">
            <label for="inc-cat">Category</label>
            <select id="inc-cat" bind:value={category}>
              {#each categories as c}
                <option value={c}>{label(c)}</option>
              {/each}
            </select>
          </div>
          <div class="field">
            <label for="inc-date">Date</label>
            <input id="inc-date" type="date" bind:value={occurredAt} />
          </div>
          <div class="field">
            <label for="inc-odometer">Odometer</label>
            <input id="inc-odometer" type="number" bind:value={odometer} min="0" oninput={() => { odometerTouched = true }} />
          </div>
        </div>
        <div class="field">
          <label for="inc-title">Title</label>
          <input id="inc-title" type="text" bind:value={title} required placeholder="e.g., Rattle on cold start" />
        </div>
        <div class="field">
          <label for="inc-desc">Details</label>
          <textarea id="inc-desc" bind:value={description} rows="3" placeholder="What happened?"></textarea>
        </div>
        {#if category === 'obd_code'}
          <div class="field">
            <label for="inc-obd">OBD Codes (JSON array)</label>
            <input id="inc-obd" type="text" bind:value={obdCodes} placeholder='["P0301","P0302"]' />
          </div>
        {/if}
        {#if category === 'accident'}
          <fieldset class="accident-fields">
            <legend>Accident Details</legend>
            <div class="form-row">
              <div class="field">
                <label for="inc-fault">Fault</label>
                <select id="inc-fault" bind:value={fault}>
                  {#each faultOptions as f}
                    <option value={f}>{f ? label(f) : '-- Select --'}</option>
                  {/each}
                </select>
              </div>
              <div class="field">
                <label for="inc-op-name">Other Party Name</label>
                <input id="inc-op-name" type="text" bind:value={otherPartyName} />
              </div>
              <div class="field">
                <label for="inc-op-phone">Other Party Phone</label>
                <input id="inc-op-phone" type="tel" bind:value={otherPartyPhone} />
              </div>
            </div>
            <div class="form-row">
              <div class="field">
                <label for="inc-op-email">Other Party Email</label>
                <input id="inc-op-email" type="email" bind:value={otherPartyEmail} />
              </div>
              <div class="field">
                <label for="inc-op-ins">Other Party Insurance</label>
                <input id="inc-op-ins" type="text" bind:value={otherPartyInsurance} />
              </div>
              <div class="field">
                <label for="inc-op-policy">Policy Number</label>
                <input id="inc-op-policy" type="text" bind:value={otherPartyPolicy} />
              </div>
            </div>
            <div class="form-row">
              <div class="field">
                <label for="inc-claim">Claim Number</label>
                <input id="inc-claim" type="text" bind:value={claimNumber} />
              </div>
              <div class="field">
                <label for="inc-adjuster">Adjuster</label>
                <input id="inc-adjuster" type="text" bind:value={adjuster} />
              </div>
              <div class="field">
                <label for="inc-adjuster-phone">Adjuster Phone</label>
                <input id="inc-adjuster-phone" type="tel" bind:value={adjusterPhone} />
              </div>
            </div>
            {#if editingId}
              <div class="form-row">
                <div class="field">
                  <label for="inc-repair-cost">Total Repair Cost ($)</label>
                  <input id="inc-repair-cost" type="number" step="0.01" min="0" bind:value={repairCost} />
                </div>
                <div class="field">
                  <label for="inc-deductible">Deductible ($)</label>
                  <input id="inc-deductible" type="number" step="0.01" min="0" bind:value={deductible} />
                </div>
                <div class="field">
                  <label for="inc-payout">Insurance Payout ($)</label>
                  <input id="inc-payout" type="number" step="0.01" min="0" bind:value={payout} />
                </div>
              </div>
            {/if}
          </fieldset>
        {/if}
        {#if serviceRecords.length > 0}
          <div class="field">
            <label for="inc-link-svc">Link Service</label>
            <select id="inc-link-svc" bind:value={linkServiceId}>
              <option value="">-- No service --</option>
              {#each serviceRecords as svc (svc.id)}
                <option value={svc.id}>{formatDate(svc.service_date)} — {svc.description || 'Service'}</option>
              {/each}
            </select>
            {#if existingServiceLinks.length > 0}
              <p class="link-hint">Already linked: {existingServiceLinks.map(serviceLabel).join(', ')}. Selecting a service adds to these links.</p>
            {/if}
          </div>
        {/if}
        <div class="field">
          <label for="inc-notes">Notes</label>
          <textarea id="inc-notes" bind:value={notes} rows="2"></textarea>
        </div>
        {#if formError}
          <p class="error">{formError}</p>
        {/if}
        <div class="form-actions">
          <button type="submit" class="btn btn-primary" disabled={saving}>
            {saving ? 'Saving...' : editingId ? 'Update' : 'Save'}
          </button>
        </div>
      </form>
    </div>
  {/if}

  {#if loading}
    <p>Loading incidents...</p>
  {:else if items.length === 0}
    {#if !showForm}
      <p class="empty">No incidents yet.</p>
    {/if}
  {:else}
    {#if presentCategories.length > 1}
      <div class="filter-bar">
        <button class="filter-btn" class:active={filter === 'all'} onclick={() => (filter = 'all')}>All</button>
        {#each presentCategories as c (c)}
          <button class="filter-btn" class:active={filter === c} onclick={() => (filter = c)}>{label(c)}</button>
        {/each}
      </div>
    {/if}

    <div class="inc-list">
      {#each filtered as inc (inc.id)}
        <div class="inc-card" class:resolved={inc.resolved}>
          <div
            class="inc-header"
            role="button"
            tabindex="0"
            onclick={() => (expandedId = expandedId === inc.id ? null : inc.id)}
            onkeydown={(e) => { if (e.key === 'Enter') expandedId = expandedId === inc.id ? null : inc.id }}
          >
            <span class="inc-category">{label(inc.category)}</span>
            <span class="inc-title">{inc.title}</span>
            <span class="inc-date">{formatDate(inc.occurred_at)}</span>
            {#if inc.resolved}
              <span class="badge badge-ok">Resolved</span>
            {/if}
            <span class="expand-icon">{expandedId === inc.id ? '▲' : '▼'}</span>
          </div>

          {#if expandedId === inc.id}
            <div class="inc-details">
              <div class="detail-actions">
                <button class="btn btn-sm btn-secondary" onclick={() => startEdit(inc)}>Edit</button>
                {#if inc.resolved}
                  <button class="btn btn-sm btn-secondary" onclick={() => toggleResolved(inc)}>Reopen</button>
                {:else if resolvingId === inc.id}
                  <select
                    class="resolve-select"
                    aria-label="Link to a service"
                    onchange={(e) => {
                      const val = (e.target as HTMLSelectElement).value
                      resolveWith(inc, val === '' ? null : parseInt(val))
                    }}
                  >
                    <option value="" disabled selected>Link to a service...</option>
                    <option value="">Resolve without service</option>
                    {#each serviceRecords as svc (svc.id)}
                      <option value={svc.id}>{formatDate(svc.service_date)} — {svc.description || 'Service'}</option>
                    {/each}
                  </select>
                  <button class="btn btn-sm btn-secondary" onclick={() => (resolvingId = null)}>Cancel</button>
                {:else}
                  <button class="btn btn-sm btn-secondary" onclick={() => (resolvingId = inc.id)}>Mark Resolved</button>
                {/if}
              </div>

              {#if inc.description || inc.notes}
                <p class="inc-desc">{[inc.description, inc.notes].filter(Boolean).join(' — ')}</p>
              {/if}
              {#if inc.recurrence_of_id}
                <p class="recurrence-line">Recurrence of #{inc.recurrence_of_id}</p>
              {/if}
              {#if inc.odometer}
                <span class="inc-meta">{inc.odometer.toLocaleString()} mi</span>
              {/if}
              {#if inc.obd_codes}
                <div class="obd-codes">
                  {#each parseObdCodes(inc.obd_codes) as code}
                    <span class="obd-chip">{code}</span>
                  {/each}
                </div>
              {/if}

              {#if inc.category === 'accident'}
                <div class="detail-grid">
                  {#if inc.fault}
                    <div class="detail-item"><span class="detail-label">Fault</span><span>{label(inc.fault)}</span></div>
                  {/if}
                  {#if inc.other_party_name}
                    <div class="detail-item"><span class="detail-label">Other Party</span><span>{inc.other_party_name}{inc.other_party_insurance ? ` — ${inc.other_party_insurance}` : ''}</span></div>
                  {/if}
                  {#if inc.insurance_claim_number}
                    <div class="detail-item"><span class="detail-label">Claim #</span><span>{inc.insurance_claim_number}</span></div>
                  {/if}
                  {#if inc.insurance_adjuster}
                    <div class="detail-item"><span class="detail-label">Adjuster</span><span>{inc.insurance_adjuster}{inc.insurance_adjuster_phone ? ` (${inc.insurance_adjuster_phone})` : ''}</span></div>
                  {/if}
                  {#if inc.total_repair_cost_cents !== null}
                    <div class="detail-item"><span class="detail-label">Repair Cost</span><span>${(inc.total_repair_cost_cents / 100).toFixed(2)}</span></div>
                  {/if}
                  {#if inc.deductible_cents !== null}
                    <div class="detail-item"><span class="detail-label">Deductible</span><span>${(inc.deductible_cents / 100).toFixed(2)}</span></div>
                  {/if}
                  {#if inc.insurance_payout_cents !== null}
                    <div class="detail-item"><span class="detail-label">Payout</span><span>${(inc.insurance_payout_cents / 100).toFixed(2)}</span></div>
                  {/if}
                </div>
              {/if}

              {#if inc.service_record_ids.length > 0}
                <div class="linked-items">
                  <span class="linked-label">Services:</span>
                  {#each inc.service_record_ids as sid (sid)}
                    <span class="linked-chip">{serviceLabel(sid)}</span>
                  {/each}
                </div>
              {/if}

              <!-- Followups -->
              <div class="followups-section">
                <div class="fu-header">
                  <h5>Followups</h5>
                  <button class="btn btn-sm btn-secondary" onclick={() => openFollowupForm(inc.id)}>+ Add</button>
                </div>

                {#if followupFor === inc.id}
                  <form class="fu-form" onsubmit={(e) => { e.preventDefault(); submitFollowup(inc.id) }}>
                    <div class="form-row">
                      <div class="field">
                        <label for="fu-date">Date</label>
                        <input id="fu-date" type="date" bind:value={fuDate} required />
                      </div>
                      <div class="field">
                        <label for="fu-method">Method</label>
                        <select id="fu-method" bind:value={fuMethod}>
                          <option value="">-- Select --</option>
                          {#each contactMethods as m}
                            <option value={m}>{label(m)}</option>
                          {/each}
                        </select>
                      </div>
                      <div class="field">
                        <label for="fu-with">Contact</label>
                        <select id="fu-with" bind:value={fuWith}>
                          <option value="">-- Select --</option>
                          {#each contactWithOptions as c}
                            <option value={c}>{label(c)}</option>
                          {/each}
                        </select>
                      </div>
                    </div>
                    <div class="field">
                      <label for="fu-summary">Summary</label>
                      <textarea id="fu-summary" bind:value={fuSummary} rows="2" required placeholder="What happened next"></textarea>
                    </div>
                    <div class="field">
                      <label for="fu-notes">Notes</label>
                      <input id="fu-notes" type="text" bind:value={fuNotes} />
                    </div>
                    <div class="form-actions">
                      <button type="button" class="btn btn-secondary btn-sm" onclick={() => (followupFor = null)}>Cancel</button>
                      <button type="submit" class="btn btn-primary btn-sm" disabled={fuSaving}>
                        {fuSaving ? 'Saving...' : 'Add'}
                      </button>
                    </div>
                  </form>
                {/if}

                {#if inc.followups.length > 0}
                  <div class="fu-timeline">
                    {#each inc.followups as fu (fu.id)}
                      <div class="fu-entry">
                        <div class="fu-entry-header">
                          <span class="fu-date">{formatDate(fu.occurred_at)}</span>
                          {#if fu.contact_method}
                            <span class="badge badge-muted">{fu.contact_method.replace(/_/g, ' ')}</span>
                          {/if}
                          {#if fu.contact_with}
                            <span class="fu-meta">with {fu.contact_with.replace(/_/g, ' ')}</span>
                          {/if}
                        </div>
                        <p class="fu-summary">{fu.summary}</p>
                        {#if fu.notes}
                          <p class="fu-notes">{fu.notes}</p>
                        {/if}
                      </div>
                    {/each}
                  </div>
                {:else if followupFor !== inc.id}
                  <p class="empty-hint">No followups yet.</p>
                {/if}
              </div>
            </div>
          {/if}
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .tab-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: var(--sp-4);
  }

  .tab-header h3 { margin: 0; }

  .error { color: var(--danger); font-size: 0.85rem; }

  .filter-bar {
    display: flex; gap: var(--sp-1); margin-bottom: var(--sp-4); flex-wrap: wrap;
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

  .inc-list { display: flex; flex-direction: column; gap: var(--sp-2); }

  .inc-card {
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    background: var(--bg-raised);
    overflow: hidden;
    transition: border-color var(--duration-base) var(--ease-out);
  }

  .inc-card:hover { border-color: var(--border); }

  .inc-card.resolved { opacity: 0.6; }

  .inc-header {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    padding: var(--sp-3) var(--sp-4);
    cursor: pointer;
    flex-wrap: wrap;
  }

  .inc-header:hover { background: var(--surface-hover); }

  .inc-category {
    font-family: var(--font-display);
    font-size: 0.75rem; text-transform: uppercase; letter-spacing: 0.05em;
    color: var(--primary); font-weight: 500;
    white-space: nowrap;
  }

  .inc-title { font-weight: 600; flex: 1; }
  .inc-date { font-size: 0.85rem; color: var(--text-muted); white-space: nowrap; }
  .inc-desc { font-size: 0.85rem; color: var(--text-muted); margin: var(--sp-1) 0; }
  .inc-meta { font-size: 0.8rem; color: var(--text-muted); margin-right: var(--sp-2); }

  .recurrence-line {
    font-size: 0.8rem;
    color: var(--warning);
    margin: var(--sp-1) 0;
    font-style: italic;
  }

  .expand-icon { font-size: 0.7rem; color: var(--text-muted); }

  .inc-details {
    padding: 0 var(--sp-4) var(--sp-4);
    border-top: 1px solid var(--border-subtle);
  }

  .detail-actions {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    margin: var(--sp-3) 0;
    flex-wrap: wrap;
  }

  .resolve-select { font-size: 0.85rem; max-width: 320px; }

  .link-hint {
    margin: var(--sp-1) 0 0;
    font-size: 0.75rem;
    color: var(--text-muted);
  }

  .detail-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
    gap: var(--sp-2) var(--sp-4);
    margin: var(--sp-3) 0;
  }

  .detail-item { font-size: 0.85rem; }

  .detail-label {
    display: block;
    font-size: 0.75rem;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.03em;
    margin-bottom: 2px;
  }

  .badge-ok { background: var(--success-bg); color: var(--success); }

  .obd-codes {
    display: flex; flex-wrap: wrap; gap: var(--sp-1); margin-top: var(--sp-1);
  }

  .obd-chip {
    font-family: var(--font-mono, monospace);
    font-size: 0.75rem;
    font-weight: 600;
    padding: 0.1rem 0.5rem;
    border-radius: var(--radius-sm);
    background: var(--warning-bg);
    color: var(--warning);
    letter-spacing: 0.03em;
  }

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
    background: var(--success-bg); color: var(--success);
  }

  .accident-fields {
    margin: var(--sp-3) 0;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    padding: var(--sp-2) var(--sp-3) var(--sp-3);
  }

  .accident-fields legend {
    font-size: 0.85rem;
    font-weight: 600;
    color: var(--text-secondary);
    padding: 0 var(--sp-1);
  }

  .followups-section {
    border-top: 1px solid var(--border-subtle);
    padding-top: var(--sp-3);
    margin-top: var(--sp-3);
  }

  .fu-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: var(--sp-2);
  }

  .fu-header h5 { margin: 0; font-size: 0.85rem; }

  .fu-form {
    padding: var(--sp-3);
    background: var(--surface);
    border-radius: var(--radius-sm);
    margin-bottom: var(--sp-3);
  }

  .fu-timeline {
    padding-left: var(--sp-3);
    border-left: 2px solid var(--border-subtle);
  }

  .fu-entry { padding: var(--sp-2) 0; font-size: 0.85rem; }

  .fu-entry + .fu-entry { border-top: 1px solid var(--border-subtle); }

  .fu-entry-header {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    margin-bottom: var(--sp-1);
  }

  .fu-date { font-weight: 600; font-size: 0.8rem; }
  .fu-meta { font-size: 0.75rem; color: var(--text-muted); }
  .fu-summary { margin: 0; font-size: 0.85rem; }

  .fu-notes {
    margin: var(--sp-1) 0 0;
    font-size: 0.75rem;
    color: var(--text-muted);
    font-style: italic;
  }

  .btn-sm { font-size: 0.75rem; padding: var(--sp-1) var(--sp-2); }

  .empty { color: var(--text-muted); text-align: center; padding: var(--sp-8) 0; }
  .empty-hint { color: var(--text-muted); font-size: 0.8rem; text-align: center; padding: var(--sp-3) 0; }
</style>
