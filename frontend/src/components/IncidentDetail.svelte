<script lang="ts">
  // Incident detail panel (actions, accident grid, linked services,
  // followups), re-homed from the retired IncidentsTab (unit F): Timeline
  // rows expand into this.
  import { incidents as incidentsApi } from '../lib/api'
  import { formatCents } from '../lib/money'
  import type { IncidentWithDetails, ServiceRecordWithLinks, UpdateIncident } from '../lib/types'
  import { formatDate } from '../lib/dates'

  let {
    vehicleId,
    incident,
    serviceRecords = [],
    onEdit,
    onChanged,
  }: {
    vehicleId: number
    incident: IncidentWithDetails
    serviceRecords?: ServiceRecordWithLinks[]
    onEdit: (incident: IncidentWithDetails) => void
    onChanged: () => void
  } = $props()

  let resolving = $state(false)

  // Followup form
  let showFollowupForm = $state(false)
  let fuDate = $state(new Date().toISOString().slice(0, 10))
  let fuMethod = $state('')
  let fuWith = $state('')
  let fuSummary = $state('')
  let fuNotes = $state('')
  let fuSaving = $state(false)

  const contactMethods = ['phone', 'email', 'in_person', 'mail', 'other']
  const contactWithOptions = ['insurance_adjuster', 'other_party', 'police', 'attorney', 'repair_shop', 'other']

  function label(c: string): string {
    return c.replace(/_/g, ' ').replace(/\b\w/g, (l) => l.toUpperCase())
  }

  function serviceLabel(id: number): string {
    const svc = serviceRecords.find((s) => s.id === id)
    return svc ? `${formatDate(svc.service_date)} — ${svc.description || 'Service'}` : `Service #${id}`
  }

  async function toggleResolved() {
    await incidentsApi.update(vehicleId, incident.id, { resolved: !incident.resolved })
    onChanged()
  }

  // Resolve, optionally linking a service at the same time.
  async function resolveWith(serviceId: number | null) {
    const payload: UpdateIncident = { resolved: true }
    if (serviceId !== null) {
      payload.service_record_ids = [...new Set([...incident.service_record_ids, serviceId])]
    }
    await incidentsApi.update(vehicleId, incident.id, payload)
    resolving = false
    onChanged()
  }

  function openFollowupForm() {
    showFollowupForm = true
    fuDate = new Date().toISOString().slice(0, 10)
    fuMethod = ''
    fuWith = ''
    fuSummary = ''
    fuNotes = ''
  }

  async function submitFollowup() {
    if (!fuSummary.trim()) return
    fuSaving = true
    try {
      await incidentsApi.addFollowup(vehicleId, incident.id, {
        occurred_at: fuDate,
        contact_method: fuMethod || undefined,
        contact_with: fuWith || undefined,
        summary: fuSummary.trim(),
        notes: fuNotes || undefined,
      })
      showFollowupForm = false
      onChanged()
    } catch (e) {
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

<div class="inc-details">
  <div class="detail-actions">
    <button class="btn btn-sm btn-secondary" onclick={() => onEdit(incident)}>Edit</button>
    {#if incident.resolved}
      <button class="btn btn-sm btn-secondary" onclick={toggleResolved}>Reopen</button>
    {:else if resolving}
      <select
        class="resolve-select"
        aria-label="Link to a service"
        onchange={(e) => {
          const val = (e.target as HTMLSelectElement).value
          resolveWith(val === '' ? null : parseInt(val))
        }}
      >
        <option value="" disabled selected>Link to a service...</option>
        <option value="">Resolve without service</option>
        {#each serviceRecords as svc (svc.id)}
          <option value={svc.id}>{formatDate(svc.service_date)} — {svc.description || 'Service'}</option>
        {/each}
      </select>
      <button class="btn btn-sm btn-secondary" onclick={() => (resolving = false)}>Cancel</button>
    {:else}
      <button class="btn btn-sm btn-secondary" onclick={() => (resolving = true)}>Mark Resolved</button>
    {/if}
  </div>

  {#if incident.description || incident.notes}
    <p class="inc-desc">{[incident.description, incident.notes].filter(Boolean).join(' — ')}</p>
  {/if}
  {#if incident.recurrence_of_id}
    <p class="recurrence-line">Recurrence of #{incident.recurrence_of_id}</p>
  {/if}
  {#if incident.odometer}
    <span class="inc-meta">{incident.odometer.toLocaleString()} mi</span>
  {/if}
  {#if incident.obd_codes}
    <div class="obd-codes">
      {#each parseObdCodes(incident.obd_codes) as code}
        <span class="obd-chip">{code}</span>
      {/each}
    </div>
  {/if}

  {#if incident.category === 'accident'}
    <div class="detail-grid">
      {#if incident.fault}
        <div class="detail-item"><span class="detail-label">Fault</span><span>{label(incident.fault)}</span></div>
      {/if}
      {#if incident.other_party_name}
        <div class="detail-item"><span class="detail-label">Other Party</span><span>{incident.other_party_name}{incident.other_party_insurance ? ` — ${incident.other_party_insurance}` : ''}</span></div>
      {/if}
      {#if incident.insurance_claim_number}
        <div class="detail-item"><span class="detail-label">Claim #</span><span>{incident.insurance_claim_number}</span></div>
      {/if}
      {#if incident.insurance_adjuster}
        <div class="detail-item"><span class="detail-label">Adjuster</span><span>{incident.insurance_adjuster}{incident.insurance_adjuster_phone ? ` (${incident.insurance_adjuster_phone})` : ''}</span></div>
      {/if}
      {#if incident.total_repair_cost_cents !== null}
        <div class="detail-item"><span class="detail-label">Repair Cost</span><span>{formatCents(incident.total_repair_cost_cents)}</span></div>
      {/if}
      {#if incident.deductible_cents !== null}
        <div class="detail-item"><span class="detail-label">Deductible</span><span>{formatCents(incident.deductible_cents)}</span></div>
      {/if}
      {#if incident.insurance_payout_cents !== null}
        <div class="detail-item"><span class="detail-label">Payout</span><span>{formatCents(incident.insurance_payout_cents)}</span></div>
      {/if}
    </div>
  {/if}

  {#if incident.service_record_ids.length > 0}
    <div class="linked-items">
      <span class="linked-label">Services:</span>
      {#each incident.service_record_ids as sid (sid)}
        <span class="linked-chip">{serviceLabel(sid)}</span>
      {/each}
    </div>
  {/if}

  <!-- Followups -->
  <div class="followups-section">
    <div class="fu-header">
      <h5>Followups</h5>
      <button class="btn btn-sm btn-secondary" onclick={openFollowupForm}>+ Add</button>
    </div>

    {#if showFollowupForm}
      <form class="fu-form" onsubmit={(e) => { e.preventDefault(); submitFollowup() }}>
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
          <button type="button" class="btn btn-secondary btn-sm" onclick={() => (showFollowupForm = false)}>Cancel</button>
          <button type="submit" class="btn btn-primary btn-sm" disabled={fuSaving}>
            {fuSaving ? 'Saving...' : 'Add'}
          </button>
        </div>
      </form>
    {/if}

    {#if incident.followups.length > 0}
      <div class="fu-timeline">
        {#each incident.followups as fu (fu.id)}
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
    {:else if !showFollowupForm}
      <p class="empty-hint">No followups yet.</p>
    {/if}
  </div>
</div>

<style>
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

  .resolve-select {
    font-size: 0.85rem;
    max-width: 320px;
  }

  .inc-desc {
    font-size: 0.85rem;
    color: var(--text-muted);
    margin: var(--sp-1) 0;
  }

  .inc-meta {
    font-size: 0.8rem;
    color: var(--text-muted);
    margin-right: var(--sp-2);
  }

  .recurrence-line {
    font-size: 0.8rem;
    color: var(--warning);
    margin: var(--sp-1) 0;
    font-style: italic;
  }

  .detail-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
    gap: var(--sp-2) var(--sp-4);
    margin: var(--sp-3) 0;
  }

  .detail-item {
    font-size: 0.85rem;
  }

  .detail-label {
    display: block;
    font-size: 0.75rem;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.03em;
    margin-bottom: 2px;
  }

  .obd-codes {
    display: flex;
    flex-wrap: wrap;
    gap: var(--sp-1);
    margin-top: var(--sp-1);
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
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--sp-1);
    margin-top: var(--sp-2);
    font-size: 0.8rem;
  }

  .linked-label {
    font-weight: 600;
    color: var(--text-muted);
    font-size: 0.75rem;
    text-transform: uppercase;
    letter-spacing: 0.03em;
  }

  .linked-chip {
    padding: 0.1rem 0.55rem;
    border-radius: 999px;
    font-size: 0.78rem;
    background: var(--success-bg);
    color: var(--success);
    border: 1px solid var(--success-border);
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

  .fu-header h5 {
    margin: 0;
    font-size: 0.85rem;
  }

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

  .fu-entry {
    padding: var(--sp-2) 0;
    font-size: 0.85rem;
  }

  .fu-entry + .fu-entry {
    border-top: 1px solid var(--border-subtle);
  }

  .fu-entry-header {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    margin-bottom: var(--sp-1);
  }

  .fu-date {
    font-weight: 600;
    font-size: 0.8rem;
  }

  .fu-meta {
    font-size: 0.75rem;
    color: var(--text-muted);
  }

  .fu-summary {
    margin: 0;
    font-size: 0.85rem;
  }

  .fu-notes {
    margin: var(--sp-1) 0 0;
    font-size: 0.75rem;
    color: var(--text-muted);
    font-style: italic;
  }

  .form-actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--sp-2);
  }

  .btn-sm {
    font-size: 0.75rem;
    padding: var(--sp-1) var(--sp-2);
  }

  .empty-hint {
    color: var(--text-muted);
    font-size: 0.8rem;
    text-align: center;
    padding: var(--sp-3) 0;
  }
</style>
