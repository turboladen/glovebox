<script lang="ts">
  import { onMount } from 'svelte'
  import { services as servicesApi, schedules as schedulesApi, parts as partsApi } from '../lib/api'
  import type { ResolvedScheduleItem, Part } from '../lib/types'

  let { vehicleId, onComplete, onCancel }: {
    vehicleId: number
    onComplete: () => void
    onCancel: () => void
  } = $props()

  let serviceDate = $state(new Date().toISOString().split('T')[0])
  let odometer = $state(0)
  let description = $state('')
  let totalCostDollars = $state('')
  let shopName = $state('')
  let notes = $state('')
  let isDiy = $state(false)
  let selectedScheduleIds: number[] = $state([])
  let scheduleItems: ResolvedScheduleItem[] = $state([])
  let availableParts: Part[] = $state([])
  let selectedPartIds: number[] = $state([])
  let saving = $state(false)
  let error = $state('')

  onMount(async () => {
    try {
      const [items, purchasedParts] = await Promise.all([
        schedulesApi.resolve(vehicleId),
        partsApi.list(vehicleId, { status: 'purchased' }),
      ])
      scheduleItems = items
      availableParts = purchasedParts
    } catch (e) {
      console.error('Failed to load form data:', e)
    }
  })

  function togglePartId(id: number) {
    if (selectedPartIds.includes(id)) {
      selectedPartIds = selectedPartIds.filter((i) => i !== id)
    } else {
      selectedPartIds = [...selectedPartIds, id]
    }
  }

  function toggleScheduleItem(id: number) {
    if (selectedScheduleIds.includes(id)) {
      selectedScheduleIds = selectedScheduleIds.filter((i) => i !== id)
    } else {
      selectedScheduleIds = [...selectedScheduleIds, id]
    }
  }

  async function submit() {
    saving = true
    error = ''
    try {
      const costCents = totalCostDollars ? Math.round(parseFloat(totalCostDollars) * 100) : undefined
      await servicesApi.create(vehicleId, {
        service_date: serviceDate,
        mileage: odometer || undefined,
        description: description || undefined,
        total_cost_cents: costCents,
        shop_name: shopName || undefined,
        notes: notes || undefined,
        schedule_item_ids: selectedScheduleIds.length > 0 ? selectedScheduleIds : undefined,
        part_ids: selectedPartIds.length > 0 ? selectedPartIds : undefined,
      })
      onComplete()
    } catch (e: any) {
      error = e.message
    } finally {
      saving = false
    }
  }
</script>

<div class="form-card">
  <h3>Log Service</h3>
  <form onsubmit={(e) => { e.preventDefault(); submit() }}>
    <div class="form-row">
      <div class="field">
        <label for="svc-date">Date</label>
        <input id="svc-date" type="date" bind:value={serviceDate} required />
      </div>
      <div class="field">
        <label for="svc-odometer">Odometer</label>
        <input id="svc-odometer" type="number" bind:value={odometer} min="0" />
      </div>
    </div>

    <div class="field">
      <label for="svc-desc">Description</label>
      <input id="svc-desc" type="text" bind:value={description} placeholder="e.g., Oil Change, 60k Service" />
    </div>

    <div class="form-row">
      <div class="field">
        <label for="svc-cost">Total Cost ($)</label>
        <input id="svc-cost" type="number" step="0.01" min="0" bind:value={totalCostDollars} />
      </div>
      <div class="field">
        <label for="svc-shop">Shop</label>
        <input id="svc-shop" type="text" bind:value={shopName} placeholder="Shop name" />
      </div>
    </div>

    <div class="field">
      <label for="svc-notes">Notes</label>
      <textarea id="svc-notes" bind:value={notes} rows="2"></textarea>
    </div>

    {#if scheduleItems.length > 0}
      <div class="field">
        <label>Schedule items covered</label>
        <div class="checkbox-list">
          {#each scheduleItems as item (item.effective_item.id)}
            <label class="checkbox-item">
              <input
                type="checkbox"
                checked={selectedScheduleIds.includes(item.effective_item.id)}
                onchange={() => toggleScheduleItem(item.effective_item.id)}
              />
              {item.effective_item.name}
            </label>
          {/each}
        </div>
      </div>
    {/if}

    {#if availableParts.length > 0}
      <div class="field">
        <label>Parts installed during this service</label>
        <div class="checkbox-list">
          {#each availableParts as p (p.id)}
            <label class="checkbox-item">
              <input
                type="checkbox"
                checked={selectedPartIds.includes(p.id)}
                onchange={() => togglePartId(p.id)}
              />
              {p.name}{p.manufacturer ? ` (${p.manufacturer})` : ''}
            </label>
          {/each}
        </div>
      </div>
    {/if}

    {#if error}
      <p class="error">{error}</p>
    {/if}
    <div class="form-actions">
      <button type="button" class="btn btn-secondary" onclick={onCancel}>Cancel</button>
      <button type="submit" class="btn btn-primary" disabled={saving}>
        {saving ? 'Saving...' : 'Save Service'}
      </button>
    </div>
  </form>
</div>

<style>
  .form-card {
    padding: 1rem;
    border: 1px solid var(--border);
    border-radius: 8px;
    margin-bottom: 1rem;
    background: var(--surface);
  }

  .form-card h3 {
    margin: 0 0 0.75rem;
  }

  .form-row {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 0.75rem;
  }

  .field {
    margin-bottom: 0.75rem;
  }

  .field label {
    display: block;
    font-size: 0.85rem;
    margin-bottom: 0.25rem;
    color: var(--text-muted);
  }

  .field input, .field textarea {
    width: 100%;
    padding: 0.4rem 0.6rem;
    border: 1px solid var(--border);
    border-radius: 4px;
    font-size: 0.9rem;
    background: var(--bg);
    color: var(--text);
    font-family: inherit;
  }

  .checkbox-list {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    max-height: 200px;
    overflow-y: auto;
    padding: 0.5rem;
    border: 1px solid var(--border);
    border-radius: 4px;
  }

  .checkbox-item {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    font-size: 0.85rem;
    cursor: pointer;
  }

  .form-actions {
    display: flex;
    gap: 0.5rem;
    justify-content: flex-end;
  }

  .error {
    color: var(--danger);
    font-size: 0.85rem;
  }
</style>
