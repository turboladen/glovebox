<script lang="ts">
  // Incident create/edit form, re-homed from the retired IncidentsTab
  // (unit F): the Timeline is now where incidents are logged and edited.
  import { incidents as incidentsApi } from '../lib/api'
  import type { IncidentWithDetails, ServiceRecordWithLinks } from '../lib/types'
  import { formatDate } from '../lib/dates'

  let {
    vehicleId,
    incident = null,
    serviceRecords = [],
    estimatedMileage,
    onComplete,
    onCancel,
  }: {
    vehicleId: number
    incident?: IncidentWithDetails | null
    serviceRecords?: ServiceRecordWithLinks[]
    estimatedMileage?: number
    onComplete: () => void
    onCancel: () => void
  } = $props()

  const categories = [
    'general', 'noise', 'leak', 'warning_light', 'cosmetic',
    'performance', 'obd_code', 'damage', 'accident', 'note',
  ]
  const faultOptions = ['', 'at_fault', 'not_at_fault', 'shared', 'unknown']

  let saving = $state(false)
  let formError = $state('')

  // The form deliberately snapshots the incident ONCE at open — it's an
  // edit buffer, not a live view (the old IncidentsTab did the same
  // imperatively in startEdit).
  // svelte-ignore state_referenced_locally
  const inc = incident

  let category = $state(inc?.category ?? 'general')
  let title = $state(inc?.title ?? '')
  let description = $state(inc?.description ?? '')
  let occurredAt = $state(
    inc
      ? inc.occurred_at.split('T')[0].split(' ')[0]
      : new Date().toISOString().slice(0, 10),
  )
  // svelte-ignore state_referenced_locally — odometer prefills once at open
  let odometer = $state<number | undefined>(inc?.odometer ?? estimatedMileage ?? undefined)
  let obdCodes = $state(inc?.obd_codes ?? '')
  let notes = $state(inc?.notes ?? '')
  // Accident-only fieldset
  let fault = $state(inc?.fault ?? '')
  let otherPartyName = $state(inc?.other_party_name ?? '')
  let otherPartyPhone = $state(inc?.other_party_phone ?? '')
  let otherPartyEmail = $state(inc?.other_party_email ?? '')
  let otherPartyInsurance = $state(inc?.other_party_insurance ?? '')
  let otherPartyPolicy = $state(inc?.other_party_policy_number ?? '')
  let claimNumber = $state(inc?.insurance_claim_number ?? '')
  let adjuster = $state(inc?.insurance_adjuster ?? '')
  let adjusterPhone = $state(inc?.insurance_adjuster_phone ?? '')
  // Financial fields (dollars as strings; edit-only)
  let repairCost = $state(
    inc?.total_repair_cost_cents != null ? (inc.total_repair_cost_cents / 100).toFixed(2) : '',
  )
  let deductible = $state(
    inc?.deductible_cents != null ? (inc.deductible_cents / 100).toFixed(2) : '',
  )
  let payout = $state(
    inc?.insurance_payout_cents != null ? (inc.insurance_payout_cents / 100).toFixed(2) : '',
  )
  // Service linking: sending service_record_ids REPLACES the set server-side,
  // so the form appends the selected service to the links captured at open.
  let existingServiceLinks: number[] = $state(inc ? [...inc.service_record_ids] : [])
  let linkServiceId = $state<number | ''>('')

  function label(c: string): string {
    return c.replace(/_/g, ' ').replace(/\b\w/g, (l) => l.toUpperCase())
  }

  function serviceLabel(id: number): string {
    const svc = serviceRecords.find((s) => s.id === id)
    return svc ? `${formatDate(svc.service_date)} — ${svc.description || 'Service'}` : `Service #${id}`
  }

  async function submit() {
    if (!title.trim()) {
      formError = 'Title is required'
      return
    }
    saving = true
    formError = ''
    const linkedIds = [
      ...new Set([...existingServiceLinks, ...(linkServiceId !== '' ? [linkServiceId] : [])]),
    ]
    try {
      if (incident) {
        // Edit clears send explicit null (double-option update).
        await incidentsApi.update(vehicleId, incident.id, {
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
      onComplete()
    } catch (e: any) {
      formError = e.message
    } finally {
      saving = false
    }
  }
</script>

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
        <input id="inc-odometer" type="number" bind:value={odometer} min="0" />
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
        {#if incident}
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
          <p class="link-hint">
            Already linked: {existingServiceLinks.map(serviceLabel).join(', ')}. Selecting a service adds to these links.
          </p>
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
      <button type="button" class="btn btn-secondary" onclick={onCancel} disabled={saving}>Cancel</button>
      <button type="submit" class="btn btn-primary" disabled={saving}>
        {saving ? 'Saving...' : incident ? 'Update' : 'Save'}
      </button>
    </div>
  </form>
</div>

<style>
  .error {
    color: var(--danger);
    font-size: 0.85rem;
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

  .link-hint {
    margin: var(--sp-1) 0 0;
    font-size: 0.75rem;
    color: var(--text-muted);
  }

  .form-actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--sp-2);
  }
</style>
