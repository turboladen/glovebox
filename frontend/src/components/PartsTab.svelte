<script lang="ts">
  import { onMount } from 'svelte'
  import { documents as documentsApi, parts as partsApi, services as servicesApi } from '../lib/api'
  import ConfirmDelete from './ConfirmDelete.svelte'
  import { formatCents } from '../lib/money'
  import type { DocumentDisposition, Part, ServiceRecordWithLinks } from '../lib/types'
  import { formatDate } from '../lib/dates'

  let { vehicleId }: { vehicleId: number } = $props()

  let allParts: Part[] = $state([])
  let loading = $state(true)

  // Part form
  let showPartForm = $state(false)
  let editingPart: Part | null = $state(null)
  let partName = $state('')
  let partManufacturer = $state('')
  let partLocation = $state('')
  let partPartNumber = $state('')
  let partOeReplaced = $state('')
  let partSeller = $state('')
  let partPurchaseDate = $state('')
  let partCost = $state('')
  let partStatus = $state('purchased')
  let partInstalledDate = $state('')
  let partInstalledOdometer = $state<number | undefined>()
  let partReplacedDate = $state('')
  let partReplacedOdometer = $state<number | undefined>()
  let partManufacturerUrl = $state('')
  let partRetailerUrl = $state('')
  let partNotes = $state('')
  let partSaving = $state(false)
  let partError = $state('')

  // Service linking for installed parts
  let serviceRecords: ServiceRecordWithLinks[] = $state([])
  let partServiceOption: 'none' | 'link' | 'create' = $state('none')
  let partLinkedServiceId: number | null = $state(null)
  // Inline service creation fields
  let newServiceDate = $state(new Date().toISOString().split('T')[0])
  let newServiceDescription = $state('')

  const statuses = ['purchased', 'installed', 'replaced', 'returned']

  onMount(loadData)

  async function loadData() {
    try {
      ;[allParts, serviceRecords] = await Promise.all([
        partsApi.list(vehicleId),
        servicesApi.list(vehicleId),
      ])
    } catch (e) {
      console.error(e)
    } finally {
      loading = false
    }
  }

  function linkedService(part: Part): ServiceRecordWithLinks | undefined {
    if (!part.installed_service_id) return undefined
    return serviceRecords.find(s => s.id === part.installed_service_id)
  }

  function statusLabel(s: string): string {
    return s.replace(/_/g, ' ').replace(/\b\w/g, l => l.toUpperCase())
  }

  function formatCost(cents: number | null): string {
    if (cents === null) return ''
    return formatCents(cents)
  }

  function statusBadgeClass(status: string): string {
    switch (status) {
      case 'installed': return 'badge-ok'
      case 'purchased': return 'badge-upcoming'
      case 'replaced': return 'badge-muted'
      case 'returned': return 'badge-muted'
      default: return ''
    }
  }

  // Part form handlers
  function openPartForm(part?: Part) {
    editingPart = part || null
    partName = part?.name || ''
    partManufacturer = part?.manufacturer || ''
    partLocation = part?.location || ''
    partPartNumber = part?.part_number || ''
    partOeReplaced = part?.oe_part_number_replaced || ''
    partSeller = part?.seller || ''
    partPurchaseDate = part?.purchase_date || ''
    partCost = part?.cost_cents !== null && part?.cost_cents !== undefined ? (part.cost_cents / 100).toFixed(2) : ''
    partStatus = part?.status || 'purchased'
    partInstalledDate = part?.installed_date || ''
    partInstalledOdometer = part?.installed_odometer ?? undefined
    partReplacedDate = part?.replaced_date || ''
    partReplacedOdometer = part?.replaced_odometer ?? undefined
    partManufacturerUrl = part?.manufacturer_url || ''
    partRetailerUrl = part?.retailer_url || ''
    partNotes = part?.notes || ''
    partLinkedServiceId = part?.installed_service_id ?? null
    partServiceOption = part?.installed_service_id ? 'link' : 'none'
    newServiceDate = new Date().toISOString().split('T')[0]
    newServiceDescription = ''
    partError = ''
    showPartForm = true
  }

  function closePartForm() {
    showPartForm = false
    editingPart = null
  }

  async function submitPart() {
    if (!partName.trim()) { partError = 'Part name is required'; return }
    partSaving = true
    partError = ''
    try {
      // If creating a new service, do it first
      let serviceId: number | null | undefined = undefined
      if ((partStatus === 'installed' || partStatus === 'replaced') && partServiceOption === 'create') {
        if (!newServiceDate) { partError = 'Service date is required'; partSaving = false; return }
        const svc = await servicesApi.create(vehicleId, {
          service_date: newServiceDate,
          description: newServiceDescription || `Installed ${partName.trim()}`,
        })
        serviceId = svc.id
      } else if ((partStatus === 'installed' || partStatus === 'replaced') && partServiceOption === 'link') {
        serviceId = partLinkedServiceId
      }

      const costCents = partCost ? Math.round(parseFloat(partCost) * 100) : undefined
      const data: any = {
        name: partName.trim(),
        manufacturer: partManufacturer || undefined,
        // On edit, a blanked location must SEND null (explicit clear) — the
        // `|| undefined` idiom omits the key, which the backend's double-option
        // convention reads as "not sent" and the stale value persists. Users
        // will want to clear slot-name backfills right after migration 000016.
        location: editingPart ? partLocation.trim() || null : partLocation || undefined,
        part_number: partPartNumber || undefined,
        oe_part_number_replaced: partOeReplaced || undefined,
        seller: partSeller || undefined,
        purchase_date: partPurchaseDate || undefined,
        cost_cents: costCents,
        status: partStatus,
        installed_date: partInstalledDate || undefined,
        installed_odometer: partInstalledOdometer,
        installed_service_id: serviceId,
        replaced_date: partReplacedDate || undefined,
        replaced_odometer: partReplacedOdometer,
        manufacturer_url: partManufacturerUrl || undefined,
        retailer_url: partRetailerUrl || undefined,
        notes: partNotes || undefined,
      }
      if (editingPart) {
        await partsApi.update(vehicleId, editingPart.id, data)
      } else {
        await partsApi.create(vehicleId, data)
      }
      closePartForm()
      await loadData()
    } catch (e: any) {
      partError = e.message
    } finally {
      partSaving = false
    }
  }

  // No catch: a failure must propagate to ConfirmDelete, which keeps the
  // confirm row open and shows the error.
  async function deletePart(part: Part, documents: DocumentDisposition) {
    await partsApi.delete(vehicleId, part.id, documents)
    // The inline confirm isn't modal (window.confirm was): the edit form can
    // be open on the part being deleted — close it, or saving would 404.
    if (editingPart?.id === part.id) closePartForm()
    await loadData()
  }
</script>

<div class="parts-tab">
  <div class="parts-header">
    <h3>Parts</h3>
    <div class="header-actions">
      <button class="btn btn-primary" onclick={() => openPartForm()}>+ Add Part</button>
    </div>
  </div>

  {#if showPartForm}
    <div class="form-card">
      <h4>{editingPart ? 'Edit Part' : 'New Part'}</h4>
      <form onsubmit={(e) => { e.preventDefault(); submitPart() }}>
        <div class="form-row">
          <div class="field">
            <label for="part-name">Part Name</label>
            <input id="part-name" type="text" bind:value={partName} required placeholder="e.g., GFB DV+" />
          </div>
          <div class="field">
            <label for="part-manufacturer">Manufacturer</label>
            <input id="part-manufacturer" type="text" bind:value={partManufacturer} />
          </div>
        </div>
        <div class="form-row">
          <div class="field">
            <label for="part-location">Location</label>
            <input id="part-location" type="text" bind:value={partLocation} placeholder="e.g., Front brakes" />
          </div>
          <div class="field">
            <label for="part-status">Status</label>
            <select id="part-status" bind:value={partStatus}>
              {#each statuses as s}
                <option value={s}>{statusLabel(s)}</option>
              {/each}
            </select>
          </div>
        </div>
        <div class="form-row">
          <div class="field">
            <label for="part-pn">Part Number</label>
            <input id="part-pn" type="text" bind:value={partPartNumber} />
          </div>
          <div class="field">
            <label for="part-oe-replaced">OE Part Replaced</label>
            <input id="part-oe-replaced" type="text" bind:value={partOeReplaced} />
          </div>
        </div>
        <div class="form-row">
          <div class="field">
            <label for="part-seller">Seller</label>
            <input id="part-seller" type="text" bind:value={partSeller} placeholder="e.g., ECS Tuning" />
          </div>
          <div class="field">
            <label for="part-purchase-date">Purchase Date</label>
            <input id="part-purchase-date" type="date" bind:value={partPurchaseDate} />
          </div>
        </div>
        <div class="form-row">
          <div class="field">
            <label for="part-cost">Cost ($)</label>
            <input id="part-cost" type="number" step="0.01" min="0" bind:value={partCost} />
          </div>
        </div>
        <div class="form-row">
          <div class="field">
            <label for="part-manufacturer-url">Manufacturer URL</label>
            <input id="part-manufacturer-url" type="url" bind:value={partManufacturerUrl} placeholder="https://..." />
          </div>
          <div class="field">
            <label for="part-retailer-url">Retailer URL</label>
            <input id="part-retailer-url" type="url" bind:value={partRetailerUrl} placeholder="https://..." />
          </div>
        </div>
        {#if partStatus === 'installed' || partStatus === 'replaced'}
          <fieldset class="service-link-fieldset">
            <legend>Service Record</legend>
            <div class="service-options">
              <label class="radio-label">
                <input type="radio" name="service-option" value="none" bind:group={partServiceOption} />
                No service record
              </label>
              <label class="radio-label">
                <input type="radio" name="service-option" value="link" bind:group={partServiceOption} />
                Link to existing service
              </label>
              <label class="radio-label">
                <input type="radio" name="service-option" value="create" bind:group={partServiceOption} />
                Create new service
              </label>
            </div>

            {#if partServiceOption === 'link'}
              <div class="field">
                <label for="part-service-link">Service</label>
                <select id="part-service-link" bind:value={partLinkedServiceId}>
                  <option value={null}>-- Select a service --</option>
                  {#each serviceRecords as svc (svc.id)}
                    <option value={svc.id}>
                      {svc.service_date}{svc.description ? ` — ${svc.description}` : ''}{svc.shop_name ? ` (${svc.shop_name})` : ''}
                    </option>
                  {/each}
                </select>
              </div>
              {#if partLinkedServiceId}
                {@const selectedSvc = serviceRecords.find(s => s.id === partLinkedServiceId)}
                {#if selectedSvc}
                  <div class="linked-service-info">
                    <span>Date: {formatDate(selectedSvc.service_date)}</span>
                    {#if selectedSvc.mileage}
                      <span>Mileage: {selectedSvc.mileage.toLocaleString()} mi</span>
                    {/if}
                    {#if selectedSvc.shop_name}
                      <span>Shop: {selectedSvc.shop_name}</span>
                    {/if}
                  </div>
                {/if}
              {/if}
            {/if}

            {#if partServiceOption === 'create'}
              <div class="form-row">
                <div class="field">
                  <label for="new-svc-date">Service Date</label>
                  <input id="new-svc-date" type="date" bind:value={newServiceDate} required />
                </div>
                <div class="field">
                  <label for="new-svc-desc">Description</label>
                  <input id="new-svc-desc" type="text" bind:value={newServiceDescription} placeholder="e.g., Diverter valve install" />
                </div>
              </div>
            {/if}

            {#if partServiceOption === 'none'}
              <div class="form-row">
                <div class="field">
                  <label for="part-installed-date">Installed Date</label>
                  <input id="part-installed-date" type="date" bind:value={partInstalledDate} />
                </div>
                <div class="field">
                  <label for="part-installed-odo">Installed Odometer</label>
                  <input id="part-installed-odo" type="number" min="0" bind:value={partInstalledOdometer} />
                </div>
              </div>
            {/if}
          </fieldset>
        {/if}
        {#if partStatus === 'replaced'}
          <div class="form-row">
            <div class="field">
              <label for="part-replaced-date">Replaced Date</label>
              <input id="part-replaced-date" type="date" bind:value={partReplacedDate} />
            </div>
            <div class="field">
              <label for="part-replaced-odo">Replaced Odometer</label>
              <input id="part-replaced-odo" type="number" min="0" bind:value={partReplacedOdometer} />
            </div>
          </div>
        {/if}
        <div class="field">
          <label for="part-notes">Notes</label>
          <input id="part-notes" type="text" bind:value={partNotes} />
        </div>
        {#if partError}
          <p class="error">{partError}</p>
        {/if}
        <div class="form-actions">
          <button type="button" class="btn btn-secondary" onclick={closePartForm}>Cancel</button>
          <button type="submit" class="btn btn-primary" disabled={partSaving}>
            {partSaving ? 'Saving...' : editingPart ? 'Update Part' : 'Add Part'}
          </button>
        </div>
      </form>
    </div>
  {/if}

  {#if loading}
    <p>Loading parts...</p>
  {:else if allParts.length === 0}
    <p class="empty">No parts yet. Track what you've bought and installed on this vehicle.</p>
  {:else}
    {#each allParts as part (part.id)}
      {@const svc = linkedService(part)}
      <div class="part-card">
        <div class="part-header">
          <div class="part-info">
            <span class="part-name">{part.name}</span>
            {#if part.manufacturer}
              <span class="part-meta">by {part.manufacturer}</span>
            {/if}
            {#if part.location}
              <span class="part-location">{part.location}</span>
            {/if}
          </div>
          <div class="part-actions">
            <button class="btn btn-sm btn-secondary" onclick={() => openPartForm(part)}>Edit</button>
            <ConfirmDelete
              label={`Delete part "${part.name}"?`}
              getDocCount={() => documentsApi.countFor('part', part.id)}
              onDelete={(docs) => deletePart(part, docs)}
            />
          </div>
        </div>
        <div class="part-detail">
          <span class="badge {statusBadgeClass(part.status)}">{part.status}</span>
          {#if part.cost_cents !== null}
            <span class="part-meta">{formatCost(part.cost_cents)}</span>
          {/if}
          {#if part.seller}
            <span class="part-meta">from {part.seller}</span>
          {/if}
          {#if svc}
            <span class="part-meta">via service {formatDate(svc.service_date)}{svc.description ? ` — ${svc.description}` : ''}</span>
            {#if svc.mileage}
              <span class="part-meta">@ {svc.mileage.toLocaleString()} mi</span>
            {/if}
          {:else}
            {#if part.installed_date}
              <span class="part-meta">installed {formatDate(part.installed_date)}</span>
            {/if}
            {#if part.installed_odometer}
              <span class="part-meta">@ {part.installed_odometer.toLocaleString()} mi</span>
            {/if}
          {/if}
          {#if part.manufacturer_url}
            <a href={part.manufacturer_url} target="_blank" class="part-link">Manufacturer</a>
          {/if}
          {#if part.retailer_url}
            <a href={part.retailer_url} target="_blank" class="part-link">Retailer</a>
          {/if}
        </div>
      </div>
    {/each}
  {/if}
</div>

<style>
  .parts-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: var(--sp-4);
  }

  .parts-header h3 { margin: 0; }

  .header-actions {
    display: flex;
    gap: var(--sp-2);
  }

  .form-card h4 { margin: 0 0 var(--sp-3); }

  .error { color: var(--danger); font-size: 0.85rem; }

  .part-card {
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-lg);
    padding: var(--sp-3) var(--sp-4);
    margin-bottom: var(--sp-2);
    background: var(--bg-raised);
    box-shadow: inset 0 1px 0 var(--edge-highlight);
    transition: border-color var(--duration-base) var(--ease-out);
  }

  .part-card:hover {
    border-color: var(--border);
  }

  .part-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }

  .part-info { display: flex; align-items: baseline; gap: var(--sp-2); flex-wrap: wrap; }
  .part-name { font-weight: 600; }

  .part-location {
    font-size: 0.78rem;
    color: var(--text-muted);
    border: 1px solid var(--border-subtle);
    border-radius: 999px;
    padding: 0 var(--sp-2);
  }

  .part-actions { display: flex; gap: var(--sp-1); }

  .part-detail {
    margin-top: var(--sp-2);
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    flex-wrap: wrap;
  }

  .part-meta { font-size: 0.8rem; color: var(--text-muted); }
  .part-link { font-size: 0.8rem; color: var(--primary); text-decoration: none; }
  .part-link:hover { text-decoration: underline; }

  .badge-ok { background: var(--success-bg); color: var(--success); border: 1px solid var(--success-border); }
  .badge-upcoming { background: var(--info-bg); color: var(--info); border: 1px solid var(--info-border); }

  .empty { color: var(--text-muted); text-align: center; padding: var(--sp-8) 0; }

  .service-link-fieldset {
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    padding: var(--sp-3) var(--sp-4);
    margin: var(--sp-2) 0;
  }

  .service-link-fieldset legend {
    font-size: 0.85rem;
    font-weight: 600;
    padding: 0 var(--sp-1);
  }

  .service-options {
    display: flex;
    gap: var(--sp-4);
    margin-bottom: var(--sp-3);
    flex-wrap: wrap;
  }

  .radio-label {
    display: flex;
    align-items: center;
    gap: var(--sp-1);
    font-size: 0.85rem;
    cursor: pointer;
  }

  .radio-label input[type="radio"] {
    width: auto;
    margin: 0;
  }

  .linked-service-info {
    display: flex;
    gap: var(--sp-3);
    flex-wrap: wrap;
    font-size: 0.8rem;
    color: var(--text-muted);
    padding: var(--sp-2) var(--sp-3);
    background: var(--surface);
    border-radius: var(--radius-sm);
    margin-top: var(--sp-2);
  }
</style>
