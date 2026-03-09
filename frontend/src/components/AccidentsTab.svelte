<script lang="ts">
  import { onMount } from 'svelte'
  import { accidents as accidentsApi } from '../lib/api'
  import type { AccidentWithDetails, CreateCorrespondence } from '../lib/types'
  import { formatDate } from '../lib/dates'

  let { vehicleId }: { vehicleId: number } = $props()

  let items: AccidentWithDetails[] = $state([])
  let loading = $state(true)
  let expandedId: number | null = $state(null)

  // Accident form
  let showForm = $state(false)
  let editingId: number | null = $state(null)
  let saving = $state(false)
  let formError = $state('')

  let formDate = $state(new Date().toISOString().split('T')[0])
  let formOdometer = $state<number | undefined>()
  let formDescription = $state('')
  let formFault = $state('')
  let formOtherPartyName = $state('')
  let formOtherPartyPhone = $state('')
  let formOtherPartyEmail = $state('')
  let formOtherPartyInsurance = $state('')
  let formOtherPartyPolicy = $state('')
  let formClaimNumber = $state('')
  let formAdjuster = $state('')
  let formAdjusterPhone = $state('')
  let formRepairCost = $state('')
  let formDeductible = $state('')
  let formPayout = $state('')
  let formResolved = $state(false)
  let formNotes = $state('')

  // Correspondence form
  let showCorrespondenceForm: number | null = $state(null)
  let corrDate = $state(new Date().toISOString().split('T')[0])
  let corrMethod = $state('')
  let corrWith = $state('')
  let corrSummary = $state('')
  let corrNotes = $state('')
  let corrSaving = $state(false)

  const faultOptions = ['', 'at_fault', 'not_at_fault', 'shared', 'unknown']
  const contactMethods = ['phone', 'email', 'in_person', 'mail', 'other']
  const contactWithOptions = ['insurance_adjuster', 'other_party', 'police', 'attorney', 'repair_shop', 'other']

  onMount(loadData)

  async function loadData() {
    try {
      items = await accidentsApi.list(vehicleId)
    } catch (e) {
      console.error(e)
    } finally {
      loading = false
    }
  }

  function formatCost(cents: number | null): string {
    if (cents === null) return ''
    return `$${(cents / 100).toFixed(2)}`
  }

  function faultLabel(f: string | null): string {
    if (!f) return 'Unknown'
    return f.replace(/_/g, ' ').replace(/\b\w/g, l => l.toUpperCase())
  }

  function resetForm() {
    editingId = null
    formDate = new Date().toISOString().split('T')[0]
    formOdometer = undefined
    formDescription = ''
    formFault = ''
    formOtherPartyName = ''
    formOtherPartyPhone = ''
    formOtherPartyEmail = ''
    formOtherPartyInsurance = ''
    formOtherPartyPolicy = ''
    formClaimNumber = ''
    formAdjuster = ''
    formAdjusterPhone = ''
    formRepairCost = ''
    formDeductible = ''
    formPayout = ''
    formResolved = false
    formNotes = ''
    formError = ''
    showForm = false
  }

  function startAdd() {
    resetForm()
    showForm = true
  }

  function startEdit(a: AccidentWithDetails) {
    editingId = a.id
    formDate = a.occurred_at.split('T')[0].split(' ')[0]
    formOdometer = a.odometer ?? undefined
    formDescription = a.description
    formFault = a.fault ?? ''
    formOtherPartyName = a.other_party_name ?? ''
    formOtherPartyPhone = a.other_party_phone ?? ''
    formOtherPartyEmail = a.other_party_email ?? ''
    formOtherPartyInsurance = a.other_party_insurance ?? ''
    formOtherPartyPolicy = a.other_party_policy_number ?? ''
    formClaimNumber = a.insurance_claim_number ?? ''
    formAdjuster = a.insurance_adjuster ?? ''
    formAdjusterPhone = a.insurance_adjuster_phone ?? ''
    formRepairCost = a.total_repair_cost_cents !== null ? (a.total_repair_cost_cents / 100).toFixed(2) : ''
    formDeductible = a.deductible_cents !== null ? (a.deductible_cents / 100).toFixed(2) : ''
    formPayout = a.insurance_payout_cents !== null ? (a.insurance_payout_cents / 100).toFixed(2) : ''
    formResolved = a.resolved
    formNotes = a.notes ?? ''
    formError = ''
    showForm = true
  }

  async function submitAccident() {
    if (!formDescription.trim()) { formError = 'Description is required'; return }
    saving = true
    formError = ''
    try {
      if (editingId) {
        await accidentsApi.update(vehicleId, editingId, {
          occurred_at: formDate,
          odometer: formOdometer ?? null,
          description: formDescription.trim(),
          fault: formFault || null,
          other_party_name: formOtherPartyName || null,
          other_party_phone: formOtherPartyPhone || null,
          other_party_email: formOtherPartyEmail || null,
          other_party_insurance: formOtherPartyInsurance || null,
          other_party_policy_number: formOtherPartyPolicy || null,
          insurance_claim_number: formClaimNumber || null,
          insurance_adjuster: formAdjuster || null,
          insurance_adjuster_phone: formAdjusterPhone || null,
          total_repair_cost_cents: formRepairCost ? Math.round(parseFloat(formRepairCost) * 100) : null,
          deductible_cents: formDeductible ? Math.round(parseFloat(formDeductible) * 100) : null,
          insurance_payout_cents: formPayout ? Math.round(parseFloat(formPayout) * 100) : null,
          resolved: formResolved,
          notes: formNotes || null,
        })
      } else {
        await accidentsApi.create(vehicleId, {
          occurred_at: formDate,
          odometer: formOdometer ?? undefined,
          description: formDescription.trim(),
          fault: formFault || undefined,
          other_party_name: formOtherPartyName || undefined,
          other_party_phone: formOtherPartyPhone || undefined,
          other_party_email: formOtherPartyEmail || undefined,
          other_party_insurance: formOtherPartyInsurance || undefined,
          other_party_policy_number: formOtherPartyPolicy || undefined,
          insurance_claim_number: formClaimNumber || undefined,
          insurance_adjuster: formAdjuster || undefined,
          insurance_adjuster_phone: formAdjusterPhone || undefined,
          notes: formNotes || undefined,
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

  function openCorrespondenceForm(accidentId: number) {
    showCorrespondenceForm = accidentId
    corrDate = new Date().toISOString().split('T')[0]
    corrMethod = ''
    corrWith = ''
    corrSummary = ''
    corrNotes = ''
  }

  async function submitCorrespondence(accidentId: number) {
    if (!corrSummary.trim()) return
    corrSaving = true
    try {
      await accidentsApi.addCorrespondence(vehicleId, accidentId, {
        occurred_at: corrDate,
        contact_method: corrMethod || undefined,
        contact_with: corrWith || undefined,
        summary: corrSummary.trim(),
        notes: corrNotes || undefined,
      })
      showCorrespondenceForm = null
      await loadData()
    } catch (e: any) {
      console.error(e)
    } finally {
      corrSaving = false
    }
  }

  async function toggleResolved(a: AccidentWithDetails) {
    await accidentsApi.update(vehicleId, a.id, { resolved: !a.resolved })
    await loadData()
  }
</script>

<div class="accidents-tab">
  <div class="tab-header">
    <h3>Accidents & Claims</h3>
    {#if !showForm}
      <button class="btn btn-primary" onclick={startAdd}>Log Accident</button>
    {/if}
  </div>

  {#if showForm}
    <div class="form-card">
      <h4>{editingId ? 'Edit Accident' : 'Log Accident'}</h4>
      <form onsubmit={(e) => { e.preventDefault(); submitAccident() }}>
        <div class="form-row">
          <div class="field">
            <label for="acc-date">Date</label>
            <input id="acc-date" type="date" bind:value={formDate} required />
          </div>
          <div class="field">
            <label for="acc-odo">Odometer</label>
            <input id="acc-odo" type="number" min="0" bind:value={formOdometer} />
          </div>
          <div class="field">
            <label for="acc-fault">Fault</label>
            <select id="acc-fault" bind:value={formFault}>
              {#each faultOptions as f}
                <option value={f}>{f ? faultLabel(f) : '-- Select --'}</option>
              {/each}
            </select>
          </div>
        </div>

        <div class="field">
          <label for="acc-desc">Description</label>
          <textarea id="acc-desc" bind:value={formDescription} rows="2" required placeholder="What happened"></textarea>
        </div>

        <details class="form-section">
          <summary>Other Party</summary>
          <div class="form-row">
            <div class="field">
              <label for="acc-op-name">Name</label>
              <input id="acc-op-name" type="text" bind:value={formOtherPartyName} />
            </div>
            <div class="field">
              <label for="acc-op-phone">Phone</label>
              <input id="acc-op-phone" type="tel" bind:value={formOtherPartyPhone} />
            </div>
          </div>
          <div class="form-row">
            <div class="field">
              <label for="acc-op-email">Email</label>
              <input id="acc-op-email" type="email" bind:value={formOtherPartyEmail} />
            </div>
            <div class="field">
              <label for="acc-op-ins">Insurance Company</label>
              <input id="acc-op-ins" type="text" bind:value={formOtherPartyInsurance} />
            </div>
          </div>
          <div class="field">
            <label for="acc-op-policy">Policy Number</label>
            <input id="acc-op-policy" type="text" bind:value={formOtherPartyPolicy} />
          </div>
        </details>

        <details class="form-section">
          <summary>Insurance Claim</summary>
          <div class="form-row">
            <div class="field">
              <label for="acc-claim">Claim Number</label>
              <input id="acc-claim" type="text" bind:value={formClaimNumber} />
            </div>
            <div class="field">
              <label for="acc-adjuster">Adjuster</label>
              <input id="acc-adjuster" type="text" bind:value={formAdjuster} />
            </div>
          </div>
          <div class="field">
            <label for="acc-adjuster-phone">Adjuster Phone</label>
            <input id="acc-adjuster-phone" type="tel" bind:value={formAdjusterPhone} />
          </div>
        </details>

        {#if editingId}
          <details class="form-section">
            <summary>Financial</summary>
            <div class="form-row">
              <div class="field">
                <label for="acc-repair-cost">Total Repair Cost ($)</label>
                <input id="acc-repair-cost" type="number" step="0.01" min="0" bind:value={formRepairCost} />
              </div>
              <div class="field">
                <label for="acc-deductible">Deductible ($)</label>
                <input id="acc-deductible" type="number" step="0.01" min="0" bind:value={formDeductible} />
              </div>
              <div class="field">
                <label for="acc-payout">Insurance Payout ($)</label>
                <input id="acc-payout" type="number" step="0.01" min="0" bind:value={formPayout} />
              </div>
            </div>
          </details>

          <label class="checkbox-label">
            <input type="checkbox" bind:checked={formResolved} />
            Resolved
          </label>
        {/if}

        <div class="field">
          <label for="acc-notes">Notes</label>
          <textarea id="acc-notes" bind:value={formNotes} rows="2"></textarea>
        </div>

        {#if formError}
          <p class="error">{formError}</p>
        {/if}
        <div class="form-actions">
          <button type="button" class="btn btn-secondary" onclick={resetForm}>Cancel</button>
          <button type="submit" class="btn btn-primary" disabled={saving}>
            {saving ? 'Saving...' : editingId ? 'Update' : 'Log Accident'}
          </button>
        </div>
      </form>
    </div>
  {/if}

  {#if loading}
    <p>Loading...</p>
  {:else if items.length === 0 && !showForm}
    <p class="empty">No accidents recorded.</p>
  {:else}
    {#each items as accident (accident.id)}
      <div class="accident-card" class:resolved={accident.resolved}>
        <div class="accident-header" role="button" tabindex="0" onclick={() => expandedId = expandedId === accident.id ? null : accident.id} onkeydown={(e) => { if (e.key === 'Enter') expandedId = expandedId === accident.id ? null : accident.id }}>
          <div class="accident-summary">
            <span class="accident-date">{formatDate(accident.occurred_at)}</span>
            <span class="accident-desc">{accident.description}</span>
            {#if accident.fault}
              <span class="badge" class:badge-danger={accident.fault === 'at_fault'} class:badge-ok={accident.fault === 'not_at_fault'} class:badge-muted={accident.fault === 'unknown' || accident.fault === 'shared'}>
                {faultLabel(accident.fault)}
              </span>
            {/if}
            {#if accident.resolved}
              <span class="badge badge-ok">Resolved</span>
            {/if}
          </div>
          <span class="expand-icon">{expandedId === accident.id ? '\u25B2' : '\u25BC'}</span>
        </div>

        {#if expandedId === accident.id}
          <div class="accident-details">
            <div class="detail-actions">
              <button class="btn btn-sm btn-secondary" onclick={() => startEdit(accident)}>Edit</button>
              <button class="btn btn-sm btn-secondary" onclick={() => toggleResolved(accident)}>
                {accident.resolved ? 'Reopen' : 'Mark Resolved'}
              </button>
            </div>

            <div class="detail-grid">
              {#if accident.odometer}
                <div class="detail-item">
                  <span class="detail-label">Odometer</span>
                  <span>{accident.odometer.toLocaleString()} mi</span>
                </div>
              {/if}
              {#if accident.insurance_claim_number}
                <div class="detail-item">
                  <span class="detail-label">Claim #</span>
                  <span>{accident.insurance_claim_number}</span>
                </div>
              {/if}
              {#if accident.insurance_adjuster}
                <div class="detail-item">
                  <span class="detail-label">Adjuster</span>
                  <span>{accident.insurance_adjuster}{accident.insurance_adjuster_phone ? ` (${accident.insurance_adjuster_phone})` : ''}</span>
                </div>
              {/if}
              {#if accident.other_party_name}
                <div class="detail-item">
                  <span class="detail-label">Other Party</span>
                  <span>{accident.other_party_name}{accident.other_party_insurance ? ` — ${accident.other_party_insurance}` : ''}</span>
                </div>
              {/if}
              {#if accident.total_repair_cost_cents !== null}
                <div class="detail-item">
                  <span class="detail-label">Repair Cost</span>
                  <span>{formatCost(accident.total_repair_cost_cents)}</span>
                </div>
              {/if}
              {#if accident.deductible_cents !== null}
                <div class="detail-item">
                  <span class="detail-label">Deductible</span>
                  <span>{formatCost(accident.deductible_cents)}</span>
                </div>
              {/if}
              {#if accident.insurance_payout_cents !== null}
                <div class="detail-item">
                  <span class="detail-label">Payout</span>
                  <span>{formatCost(accident.insurance_payout_cents)}</span>
                </div>
              {/if}
              {#if accident.notes}
                <div class="detail-item full-width">
                  <span class="detail-label">Notes</span>
                  <span>{accident.notes}</span>
                </div>
              {/if}
            </div>

            <!-- Correspondence Timeline -->
            <div class="correspondence-section">
              <div class="corr-header">
                <h5>Correspondence</h5>
                <button class="btn btn-sm btn-secondary" onclick={() => openCorrespondenceForm(accident.id)}>+ Add</button>
              </div>

              {#if showCorrespondenceForm === accident.id}
                <form class="corr-form" onsubmit={(e) => { e.preventDefault(); submitCorrespondence(accident.id) }}>
                  <div class="form-row">
                    <div class="field">
                      <label for="corr-date">Date</label>
                      <input id="corr-date" type="date" bind:value={corrDate} required />
                    </div>
                    <div class="field">
                      <label for="corr-method">Method</label>
                      <select id="corr-method" bind:value={corrMethod}>
                        <option value="">-- Select --</option>
                        {#each contactMethods as m}
                          <option value={m}>{m.replace(/_/g, ' ').replace(/\b\w/g, l => l.toUpperCase())}</option>
                        {/each}
                      </select>
                    </div>
                    <div class="field">
                      <label for="corr-with">Contact</label>
                      <select id="corr-with" bind:value={corrWith}>
                        <option value="">-- Select --</option>
                        {#each contactWithOptions as c}
                          <option value={c}>{c.replace(/_/g, ' ').replace(/\b\w/g, l => l.toUpperCase())}</option>
                        {/each}
                      </select>
                    </div>
                  </div>
                  <div class="field">
                    <label for="corr-summary">Summary</label>
                    <textarea id="corr-summary" bind:value={corrSummary} rows="2" required placeholder="What was discussed"></textarea>
                  </div>
                  <div class="field">
                    <label for="corr-notes">Notes</label>
                    <input id="corr-notes" type="text" bind:value={corrNotes} />
                  </div>
                  <div class="form-actions">
                    <button type="button" class="btn btn-secondary btn-sm" onclick={() => showCorrespondenceForm = null}>Cancel</button>
                    <button type="submit" class="btn btn-primary btn-sm" disabled={corrSaving}>
                      {corrSaving ? 'Saving...' : 'Add'}
                    </button>
                  </div>
                </form>
              {/if}

              {#if accident.correspondence.length > 0}
                <div class="corr-timeline">
                  {#each accident.correspondence as entry (entry.id)}
                    <div class="corr-entry">
                      <div class="corr-entry-header">
                        <span class="corr-date">{formatDate(entry.occurred_at)}</span>
                        {#if entry.contact_method}
                          <span class="badge badge-muted">{entry.contact_method.replace(/_/g, ' ')}</span>
                        {/if}
                        {#if entry.contact_with}
                          <span class="corr-meta">with {entry.contact_with.replace(/_/g, ' ')}</span>
                        {/if}
                      </div>
                      <p class="corr-summary">{entry.summary}</p>
                      {#if entry.notes}
                        <p class="corr-notes">{entry.notes}</p>
                      {/if}
                    </div>
                  {/each}
                </div>
              {:else if showCorrespondenceForm !== accident.id}
                <p class="empty-hint">No correspondence recorded yet.</p>
              {/if}
            </div>
          </div>
        {/if}
      </div>
    {/each}
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

  .accident-card {
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    margin-bottom: var(--sp-3);
    background: var(--bg-raised);
    overflow: hidden;
    transition: border-color var(--duration-base) var(--ease-out);
  }

  .accident-card:hover {
    border-color: var(--border);
  }

  .accident-card.resolved {
    opacity: 0.7;
  }

  .accident-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: var(--sp-3) var(--sp-4);
    cursor: pointer;
  }

  .accident-header:hover {
    background: var(--surface-hover);
  }

  .accident-summary {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    flex-wrap: wrap;
  }

  .accident-date {
    font-weight: 600;
    font-size: 0.85rem;
    white-space: nowrap;
  }

  .accident-desc {
    font-size: 0.85rem;
    color: var(--text-secondary);
  }

  .expand-icon {
    font-size: 0.7rem;
    color: var(--text-muted);
  }

  .accident-details {
    padding: 0 var(--sp-4) var(--sp-4);
    border-top: 1px solid var(--border-subtle);
  }

  .detail-actions {
    display: flex;
    gap: var(--sp-2);
    margin: var(--sp-3) 0;
  }

  .detail-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
    gap: var(--sp-2) var(--sp-4);
    margin-bottom: var(--sp-4);
  }

  .detail-item {
    font-size: 0.85rem;
  }

  .detail-item.full-width {
    grid-column: 1 / -1;
  }

  .detail-label {
    display: block;
    font-size: 0.75rem;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.03em;
    margin-bottom: 2px;
  }

  .badge-danger { background: var(--danger-bg); color: var(--danger); }
  .badge-ok { background: var(--success-bg); color: var(--success); }

  .correspondence-section {
    border-top: 1px solid var(--border-subtle);
    padding-top: var(--sp-3);
  }

  .corr-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: var(--sp-2);
  }

  .corr-header h5 {
    margin: 0;
    font-size: 0.85rem;
  }

  .corr-form {
    padding: var(--sp-3);
    background: var(--surface);
    border-radius: var(--radius-sm);
    margin-bottom: var(--sp-3);
  }

  .corr-timeline {
    padding-left: var(--sp-3);
    border-left: 2px solid var(--border-subtle);
  }

  .corr-entry {
    padding: var(--sp-2) 0;
    font-size: 0.85rem;
  }

  .corr-entry + .corr-entry {
    border-top: 1px solid var(--border-subtle);
  }

  .corr-entry-header {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    margin-bottom: var(--sp-1);
  }

  .corr-date {
    font-weight: 600;
    font-size: 0.8rem;
  }

  .corr-meta {
    font-size: 0.75rem;
    color: var(--text-muted);
  }

  .corr-summary {
    margin: 0;
    font-size: 0.85rem;
  }

  .corr-notes {
    margin: var(--sp-1) 0 0;
    font-size: 0.75rem;
    color: var(--text-muted);
    font-style: italic;
  }

  .form-section {
    margin: var(--sp-3) 0;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    padding: var(--sp-2) var(--sp-3);
  }

  .form-section summary {
    cursor: pointer;
    font-size: 0.85rem;
    font-weight: 600;
    color: var(--text-secondary);
    padding: var(--sp-1) 0;
  }

  .form-section[open] summary {
    margin-bottom: var(--sp-2);
  }

  .checkbox-label {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    font-size: 0.85rem;
    cursor: pointer;
    margin: var(--sp-3) 0;
  }

  .checkbox-label input[type="checkbox"] {
    width: auto;
    margin: 0;
  }

  .empty { color: var(--text-muted); text-align: center; padding: var(--sp-8) 0; }
  .empty-hint { color: var(--text-muted); font-size: 0.8rem; text-align: center; padding: var(--sp-3) 0; }
  .error { color: var(--danger); font-size: 0.85rem; }
</style>
