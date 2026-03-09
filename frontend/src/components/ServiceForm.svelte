<script lang="ts">
  import { onMount } from 'svelte'
  import { services as servicesApi, schedules as schedulesApi, parts as partsApi, shops as shopsApi } from '../lib/api'
  import type { ResolvedScheduleItem, Part, Shop } from '../lib/types'

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
  let shopList: Shop[] = $state([])
  let selectedShopId: number | null = $state(null)
  let shopDropdownOpen = $state(false)
  let highlightedIndex = $state(-1)
  let filteredShops = $derived(
    shopName.trim()
      ? shopList.filter(s => s.name.toLowerCase().includes(shopName.trim().toLowerCase()))
      : shopList
  )
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
      const [items, purchasedParts, allShops] = await Promise.all([
        schedulesApi.resolve(vehicleId),
        partsApi.list(vehicleId, { status: 'purchased' }),
        shopsApi.list(),
      ])
      scheduleItems = items
      availableParts = purchasedParts
      shopList = allShops
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

  function selectShop(shop: Shop) {
    shopName = shop.name
    selectedShopId = shop.id
    shopDropdownOpen = false
  }

  function handleShopInput() {
    selectedShopId = null
    highlightedIndex = -1
    shopDropdownOpen = true
  }

  function handleShopBlur() {
    // Delay to allow click on dropdown item
    setTimeout(() => { shopDropdownOpen = false }, 150)
  }

  function handleShopKeydown(e: KeyboardEvent) {
    if (!shopDropdownOpen || filteredShops.length === 0) {
      if (e.key === 'ArrowDown' && filteredShops.length > 0) {
        shopDropdownOpen = true
        highlightedIndex = 0
        e.preventDefault()
      }
      return
    }

    switch (e.key) {
      case 'ArrowDown':
        e.preventDefault()
        highlightedIndex = (highlightedIndex + 1) % filteredShops.length
        break
      case 'ArrowUp':
        e.preventDefault()
        highlightedIndex = highlightedIndex <= 0 ? filteredShops.length - 1 : highlightedIndex - 1
        break
      case 'Enter':
        e.preventDefault()
        if (highlightedIndex >= 0 && highlightedIndex < filteredShops.length) {
          selectShop(filteredShops[highlightedIndex])
        }
        break
      case 'Escape':
        shopDropdownOpen = false
        highlightedIndex = -1
        break
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
        shop_id: selectedShopId ?? undefined,
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
      <div class="field shop-autocomplete">
        <label for="svc-shop">Shop</label>
        <input
          id="svc-shop"
          type="text"
          role="combobox"
          aria-haspopup="listbox"
          aria-expanded={shopDropdownOpen && filteredShops.length > 0}
          aria-controls="shop-listbox"
          aria-autocomplete="list"
          aria-activedescendant={highlightedIndex >= 0 ? `shop-option-${filteredShops[highlightedIndex]?.id}` : undefined}
          bind:value={shopName}
          oninput={handleShopInput}
          onfocus={() => { shopDropdownOpen = true }}
          onblur={handleShopBlur}
          onkeydown={handleShopKeydown}
          placeholder="Search or type shop name"
          autocomplete="off"
        />
        {#if shopDropdownOpen && filteredShops.length > 0}
          <ul id="shop-listbox" class="shop-dropdown" role="listbox">
            {#each filteredShops as shop, i (shop.id)}
              <li
                id="shop-option-{shop.id}"
                role="option"
                aria-selected={selectedShopId === shop.id}
                class:highlighted={i === highlightedIndex}
              >
                <button type="button" onmousedown={() => selectShop(shop)}>
                  <span class="shop-name">{shop.name}</span>
                  {#if shop.specialty}
                    <span class="shop-detail">{shop.specialty}</span>
                  {/if}
                </button>
              </li>
            {/each}
          </ul>
        {/if}
      </div>
    </div>

    <div class="field">
      <label for="svc-notes">Notes</label>
      <textarea id="svc-notes" bind:value={notes} rows="2"></textarea>
    </div>

    {#if scheduleItems.length > 0}
      <div class="field" role="group" aria-labelledby="schedule-items-label">
        <span id="schedule-items-label" class="field-label">Schedule items covered</span>
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
      <div class="field" role="group" aria-labelledby="parts-installed-label">
        <span id="parts-installed-label" class="field-label">Parts installed during this service</span>
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
  .form-card h3 {
    margin: 0 0 var(--sp-3);
  }

  .checkbox-list {
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
    max-height: 200px;
    overflow-y: auto;
    padding: var(--sp-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--bg);
  }

  .checkbox-item {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    font-size: 0.85rem;
    cursor: pointer;
  }

  .checkbox-item input[type="checkbox"] {
    width: auto;
  }

  .shop-autocomplete {
    position: relative;
  }

  .shop-dropdown {
    position: absolute;
    top: 100%;
    left: 0;
    right: 0;
    z-index: 10;
    margin: 2px 0 0;
    padding: 0;
    list-style: none;
    background: var(--bg-raised);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    max-height: 180px;
    overflow-y: auto;
  }

  .shop-dropdown li button {
    display: flex;
    flex-direction: column;
    width: 100%;
    padding: var(--sp-2) var(--sp-3);
    background: none;
    border: none;
    color: var(--text);
    text-align: left;
    cursor: pointer;
    font-size: 0.85rem;
  }

  .shop-dropdown li button:hover,
  .shop-dropdown li.highlighted button {
    background: var(--surface-hover);
  }

  .shop-detail {
    font-size: 0.75rem;
    color: var(--text-muted);
  }

  .error {
    color: var(--danger);
    font-size: 0.85rem;
  }
</style>
