<script lang="ts">
  import { onMount } from 'svelte'
  import { partSlots as slotsApi, parts as partsApi } from '../lib/api'
  import type { PartSlot, Part } from '../lib/types'
  import { formatDate } from '../lib/dates'

  let { vehicleId }: { vehicleId: number } = $props()

  let slots: PartSlot[] = $state([])
  let allParts: Part[] = $state([])
  let loading = $state(true)
  let expandedSlotId: number | null = $state(null)

  // Slot form
  let showSlotForm = $state(false)
  let editingSlot: PartSlot | null = $state(null)
  let slotName = $state('')
  let slotCategory = $state('')
  let slotOeSpec = $state('')
  let slotOePartNumber = $state('')
  let slotNotes = $state('')
  let slotSaving = $state(false)
  let slotError = $state('')

  // Part form
  let showPartForm = $state(false)
  let partSlotId: number | null = $state(null)
  let editingPart: Part | null = $state(null)
  let partName = $state('')
  let partManufacturer = $state('')
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
  let partNotes = $state('')
  let partSaving = $state(false)
  let partError = $state('')

  const categories = ['engine', 'suspension', 'brakes', 'wheels_tires', 'interior', 'exterior', 'electrical', 'drivetrain', 'exhaust', 'other']
  const statuses = ['purchased', 'installed', 'replaced', 'returned']

  onMount(loadData)

  async function loadData() {
    try {
      ;[slots, allParts] = await Promise.all([
        slotsApi.list(vehicleId),
        partsApi.list(vehicleId),
      ])
    } catch (e) {
      console.error(e)
    } finally {
      loading = false
    }
  }

  function partsForSlot(slotId: number): Part[] {
    return allParts.filter(p => p.slot_id === slotId)
  }

  function currentPart(slotId: number): Part | undefined {
    return allParts.find(p => p.slot_id === slotId && p.status === 'installed')
  }

  function unslottedParts(): Part[] {
    return allParts.filter(p => p.slot_id === null)
  }

  function groupedSlots(): Record<string, PartSlot[]> {
    const groups: Record<string, PartSlot[]> = {}
    for (const slot of slots) {
      const cat = slot.category || 'uncategorized'
      if (!groups[cat]) groups[cat] = []
      groups[cat].push(slot)
    }
    return groups
  }

  function categoryLabel(c: string): string {
    return c.replace(/_/g, ' ').replace(/\b\w/g, l => l.toUpperCase())
  }

  function formatCost(cents: number | null): string {
    if (cents === null) return ''
    return `$${(cents / 100).toFixed(2)}`
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

  // Slot form handlers
  function openSlotForm(slot?: PartSlot) {
    editingSlot = slot || null
    slotName = slot?.name || ''
    slotCategory = slot?.category || ''
    slotOeSpec = slot?.oe_spec || ''
    slotOePartNumber = slot?.oe_part_number || ''
    slotNotes = slot?.notes || ''
    slotError = ''
    showSlotForm = true
    showPartForm = false
  }

  function closeSlotForm() {
    showSlotForm = false
    editingSlot = null
  }

  async function submitSlot() {
    if (!slotName.trim()) { slotError = 'Slot name is required'; return }
    slotSaving = true
    slotError = ''
    try {
      const data = {
        name: slotName.trim(),
        category: slotCategory || undefined,
        oe_spec: slotOeSpec || undefined,
        oe_part_number: slotOePartNumber || undefined,
        notes: slotNotes || undefined,
      }
      if (editingSlot) {
        await slotsApi.update(vehicleId, editingSlot.id, data)
      } else {
        await slotsApi.create(vehicleId, data)
      }
      closeSlotForm()
      await loadData()
    } catch (e: any) {
      slotError = e.message
    } finally {
      slotSaving = false
    }
  }

  async function deleteSlot(slot: PartSlot) {
    if (!confirm(`Delete slot "${slot.name}"? Parts in this slot will become unslotted.`)) return
    try {
      await slotsApi.delete(vehicleId, slot.id)
      await loadData()
    } catch (e: any) {
      alert(`Failed to delete slot: ${e.message}`)
    }
  }

  // Part form handlers
  function openPartForm(slotId: number | null, part?: Part) {
    editingPart = part || null
    partSlotId = slotId
    partName = part?.name || ''
    partManufacturer = part?.manufacturer || ''
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
    partNotes = part?.notes || ''
    partError = ''
    showPartForm = true
    showSlotForm = false
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
      const costCents = partCost ? Math.round(parseFloat(partCost) * 100) : undefined
      const data: any = {
        slot_id: partSlotId,
        name: partName.trim(),
        manufacturer: partManufacturer || undefined,
        part_number: partPartNumber || undefined,
        oe_part_number_replaced: partOeReplaced || undefined,
        seller: partSeller || undefined,
        purchase_date: partPurchaseDate || undefined,
        cost_cents: costCents,
        status: partStatus,
        installed_date: partInstalledDate || undefined,
        installed_odometer: partInstalledOdometer,
        replaced_date: partReplacedDate || undefined,
        replaced_odometer: partReplacedOdometer,
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

  async function deletePart(part: Part) {
    if (!confirm(`Delete part "${part.name}"?`)) return
    try {
      await partsApi.delete(vehicleId, part.id)
      await loadData()
    } catch (e: any) {
      alert(`Failed to delete part: ${e.message}`)
    }
  }
</script>

<div class="parts-tab">
  <div class="parts-header">
    <h3>Parts & Slots</h3>
    <div class="header-actions">
      <button class="btn btn-secondary" onclick={() => openPartForm(null)}>+ Add Part</button>
      <button class="btn btn-primary" onclick={() => openSlotForm()}>+ Add Slot</button>
    </div>
  </div>

  {#if showSlotForm}
    <div class="form-card">
      <h4>{editingSlot ? 'Edit Slot' : 'New Part Slot'}</h4>
      <form onsubmit={(e) => { e.preventDefault(); submitSlot() }}>
        <div class="form-row">
          <div class="field">
            <label for="slot-name">Name</label>
            <input id="slot-name" type="text" bind:value={slotName} required placeholder="e.g., Diverter Valve" />
          </div>
          <div class="field">
            <label for="slot-category">Category</label>
            <select id="slot-category" bind:value={slotCategory}>
              <option value="">-- Select --</option>
              {#each categories as c}
                <option value={c}>{categoryLabel(c)}</option>
              {/each}
            </select>
          </div>
        </div>
        <div class="form-row">
          <div class="field">
            <label for="slot-oe-spec">OE Spec</label>
            <input id="slot-oe-spec" type="text" bind:value={slotOeSpec} placeholder='e.g., 18" 245/40R18' />
          </div>
          <div class="field">
            <label for="slot-oe-pn">OE Part Number</label>
            <input id="slot-oe-pn" type="text" bind:value={slotOePartNumber} />
          </div>
        </div>
        <div class="field">
          <label for="slot-notes">Notes</label>
          <input id="slot-notes" type="text" bind:value={slotNotes} />
        </div>
        {#if slotError}
          <p class="error">{slotError}</p>
        {/if}
        <div class="form-actions">
          <button type="button" class="btn btn-secondary" onclick={closeSlotForm}>Cancel</button>
          <button type="submit" class="btn btn-primary" disabled={slotSaving}>
            {slotSaving ? 'Saving...' : editingSlot ? 'Update Slot' : 'Create Slot'}
          </button>
        </div>
      </form>
    </div>
  {/if}

  {#if showPartForm}
    <div class="form-card">
      <h4>{editingPart ? 'Edit Part' : 'New Part'}</h4>
      <form onsubmit={(e) => { e.preventDefault(); submitPart() }}>
        <div class="form-row">
          <div class="field">
            <label for="part-slot">Slot</label>
            <select id="part-slot" bind:value={partSlotId}>
              <option value={null}>None (unslotted)</option>
              {#each slots as slot}
                <option value={slot.id}>{slot.name}{slot.category ? ` (${categoryLabel(slot.category)})` : ''}</option>
              {/each}
            </select>
          </div>
          <div class="field">
            <label for="part-status">Status</label>
            <select id="part-status" bind:value={partStatus}>
              {#each statuses as s}
                <option value={s}>{categoryLabel(s)}</option>
              {/each}
            </select>
          </div>
        </div>
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
        {#if partStatus === 'installed' || partStatus === 'replaced'}
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
  {:else if slots.length === 0 && allParts.length === 0}
    <p class="empty">No parts or slots yet. Add a slot to define a position on your vehicle, then add parts to it.</p>
  {:else}
    {#each Object.entries(groupedSlots()) as [category, categorySlots] (category)}
      <div class="category-group">
        <h4 class="category-heading">{categoryLabel(category)}</h4>
        {#each categorySlots as slot (slot.id)}
          {@const installed = currentPart(slot.id)}
          {@const slotParts = partsForSlot(slot.id)}
          <div class="slot-card">
            <div class="slot-header">
              <div class="slot-info">
                <span class="slot-name">{slot.name}</span>
                {#if slot.oe_spec}
                  <span class="slot-oe">OE: {slot.oe_spec}</span>
                {/if}
              </div>
              <div class="slot-actions">
                <button class="btn btn-sm btn-secondary" onclick={() => openPartForm(slot.id)}>+ Part</button>
                <button class="btn btn-sm btn-secondary" onclick={() => openSlotForm(slot)}>Edit</button>
                <button class="btn btn-sm btn-danger" onclick={() => deleteSlot(slot)}>Delete</button>
              </div>
            </div>
            {#if installed}
              <div class="installed-part">
                <span class="part-name">{installed.name}</span>
                {#if installed.manufacturer}
                  <span class="part-meta">by {installed.manufacturer}</span>
                {/if}
                {#if installed.installed_date}
                  <span class="part-meta">installed {formatDate(installed.installed_date)}</span>
                {/if}
                {#if installed.installed_odometer}
                  <span class="part-meta">@ {installed.installed_odometer.toLocaleString()} mi</span>
                {/if}
                <span class="badge {statusBadgeClass(installed.status)}">{installed.status}</span>
              </div>
            {:else}
              <div class="installed-part empty-slot">No part installed</div>
            {/if}
            {#if slotParts.length > 1 || (slotParts.length === 1 && !installed)}
              <button class="btn-link" onclick={() => expandedSlotId = expandedSlotId === slot.id ? null : slot.id}>
                {expandedSlotId === slot.id ? 'Hide' : 'Show'} history ({slotParts.length} part{slotParts.length !== 1 ? 's' : ''})
              </button>
            {/if}
            {#if expandedSlotId === slot.id}
              <div class="part-history">
                {#each slotParts as part (part.id)}
                  <div class="part-row" class:current={part.status === 'installed'}>
                    <span class="part-name">{part.name}</span>
                    <span class="badge {statusBadgeClass(part.status)}">{part.status}</span>
                    {#if part.cost_cents !== null}
                      <span class="part-meta">{formatCost(part.cost_cents)}</span>
                    {/if}
                    <div class="part-row-actions">
                      <button class="btn btn-sm btn-secondary" onclick={() => openPartForm(slot.id, part)}>Edit</button>
                      <button class="btn btn-sm btn-danger" onclick={() => deletePart(part)}>Delete</button>
                    </div>
                  </div>
                {/each}
              </div>
            {/if}
          </div>
        {/each}
      </div>
    {/each}

    {@const unslotted = unslottedParts()}
    {#if unslotted.length > 0}
      <div class="category-group">
        <h4 class="category-heading">Unslotted Parts</h4>
        {#each unslotted as part (part.id)}
          <div class="slot-card">
            <div class="slot-header">
              <div class="slot-info">
                <span class="slot-name">{part.name}</span>
                {#if part.manufacturer}
                  <span class="slot-oe">by {part.manufacturer}</span>
                {/if}
              </div>
              <div class="slot-actions">
                <button class="btn btn-sm btn-secondary" onclick={() => openPartForm(null, part)}>Edit</button>
                <button class="btn btn-sm btn-danger" onclick={() => deletePart(part)}>Delete</button>
              </div>
            </div>
            <div class="installed-part">
              <span class="badge {statusBadgeClass(part.status)}">{part.status}</span>
              {#if part.cost_cents !== null}
                <span class="part-meta">{formatCost(part.cost_cents)}</span>
              {/if}
              {#if part.seller}
                <span class="part-meta">from {part.seller}</span>
              {/if}
            </div>
          </div>
        {/each}
      </div>
    {/if}
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

  .category-group { margin-bottom: var(--sp-6); }

  .category-heading {
    font-family: var(--font-display);
    font-size: 0.85rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--text-muted);
    margin: 0 0 var(--sp-2);
    padding-bottom: var(--sp-1);
    border-bottom: 1px solid var(--border-subtle);
  }

  .slot-card {
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    padding: var(--sp-3) var(--sp-4);
    margin-bottom: var(--sp-2);
    background: var(--bg-raised);
    transition: border-color var(--duration-base) var(--ease-out);
  }

  .slot-card:hover {
    border-color: var(--border);
  }

  .slot-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }

  .slot-info { display: flex; align-items: baseline; gap: var(--sp-2); }
  .slot-name { font-weight: 600; }
  .slot-oe { font-size: 0.8rem; color: var(--text-muted); }

  .slot-actions { display: flex; gap: var(--sp-1); }

  .installed-part {
    margin-top: var(--sp-2);
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    flex-wrap: wrap;
  }

  .installed-part.empty-slot {
    color: var(--text-muted);
    font-style: italic;
    font-size: 0.85rem;
  }

  .part-name { font-weight: 500; }
  .part-meta { font-size: 0.8rem; color: var(--text-muted); }

  .badge-ok { background: var(--success-bg); color: var(--success); }
  .badge-upcoming { background: var(--warning-bg); color: var(--warning); }

  .btn-link {
    background: none;
    border: none;
    color: var(--primary);
    cursor: pointer;
    font-size: 0.8rem;
    padding: var(--sp-1) 0;
    text-decoration: underline;
    transition: color var(--duration-fast) var(--ease-out);
  }

  .btn-link:hover {
    color: var(--primary-hover);
  }

  .part-history {
    margin-top: var(--sp-2);
    padding-left: var(--sp-2);
    border-left: 2px solid var(--border-subtle);
  }

  .part-row {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    padding: var(--sp-1) 0;
    flex-wrap: wrap;
  }

  .part-row.current { font-weight: 500; }
  .part-row-actions { margin-left: auto; display: flex; gap: var(--sp-1); }

  .empty { color: var(--text-muted); text-align: center; padding: var(--sp-8) 0; }
</style>
