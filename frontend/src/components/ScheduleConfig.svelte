<script lang="ts">
  // Schedule ⚙ (Plan tab sub-view): the maintenance-schedule CRUD half —
  // resolved items with intervals + estimated cost, vehicle-level item
  // create/edit/delete, and the dismissed-overrides section (moved here
  // from the old Schedule tab).
  import { onMount } from 'svelte'
  import { schedules as schedulesApi } from '../lib/api'
  import { anchorId, flashHighlightFromQuery } from '../lib/highlight'
  import { formatCents as formatCentsShared } from '../lib/money'
  import type { ResolvedScheduleItem, ScheduleItem } from '../lib/types'

  let { vehicleId, onChanged }: {
    vehicleId: number
    onChanged?: () => Promise<void> | void
  } = $props()

  let resolved: ResolvedScheduleItem[] = $state([])
  let dismissedItems: ScheduleItem[] = $state([])
  let loading = $state(true)
  let error = $state('')

  // Create/edit form
  let showForm = $state(false)
  let editingId: number | null = $state(null)
  let saving = $state(false)
  let name = $state('')
  let intervalMiles = $state('')
  let intervalMonths = $state('')
  let estCost = $state('')
  let notes = $state('')

  async function loadData() {
    try {
      const [res, vehicleItems] = await Promise.all([
        schedulesApi.resolve(vehicleId),
        schedulesApi.list({ vehicle_id: vehicleId }),
      ])
      resolved = res
      // Vehicle-level enabled=false overrides — dismissed items (in-place
      // or shadows of inherited items).
      dismissedItems = vehicleItems.filter((i) => !i.enabled)
    } catch (e: any) {
      error = e.message
    } finally {
      loading = false
    }
  }

  onMount(loadData)

  // Deep-link highlight (?hl=schedule_item:N from a global-search hit's
  // ⚙ link) once the item cards have rendered.
  let flashedHighlight = false
  $effect(() => {
    if (!loading && !flashedHighlight) {
      flashedHighlight = true
      flashHighlightFromQuery('schedule_item')
    }
  })

  async function refresh() {
    await loadData()
    await onChanged?.()
  }

  function isVehicleOwned(item: ScheduleItem): boolean {
    return item.vehicle_id === vehicleId
  }

  function startAdd() {
    editingId = null
    name = ''
    intervalMiles = ''
    intervalMonths = ''
    estCost = ''
    notes = ''
    error = ''
    showForm = true
  }

  function startEdit(item: ScheduleItem) {
    editingId = item.id
    name = item.name
    intervalMiles = item.interval_miles != null ? String(item.interval_miles) : ''
    intervalMonths = item.interval_months != null ? String(item.interval_months) : ''
    estCost = item.est_cost_cents != null ? (item.est_cost_cents / 100).toFixed(2) : ''
    notes = item.notes ?? ''
    error = ''
    showForm = true
  }

  async function submit() {
    if (!name.trim()) {
      error = 'Name is required'
      return
    }
    saving = true
    error = ''
    try {
      // Edit clears send explicit null (double-option update DTO).
      const fields = {
        name: name.trim(),
        interval_miles: intervalMiles ? parseInt(intervalMiles, 10) : null,
        interval_months: intervalMonths ? parseInt(intervalMonths, 10) : null,
        est_cost_cents: estCost ? Math.round(parseFloat(estCost) * 100) : null,
        notes: notes || null,
      }
      if (editingId) {
        await schedulesApi.update(editingId, fields)
      } else {
        await schedulesApi.create({ vehicle_id: vehicleId, ...fields })
      }
      showForm = false
      await refresh()
    } catch (e: any) {
      error = e.message
    } finally {
      saving = false
    }
  }

  async function deleteItem(item: ScheduleItem) {
    if (!confirm(`Delete schedule item "${item.name}"?`)) return
    try {
      await schedulesApi.delete(item.id)
      await refresh()
    } catch (e: any) {
      error = e.message
    }
  }

  async function reenableItem(item: ScheduleItem) {
    error = ''
    try {
      await schedulesApi.undismiss(vehicleId, item.id)
      await refresh()
    } catch (e: any) {
      error = e.message
    }
  }

  function intervalText(item: ScheduleItem): string {
    const parts: string[] = []
    if (item.interval_miles) parts.push(`every ${item.interval_miles.toLocaleString()} mi`)
    if (item.interval_months) parts.push(`every ${item.interval_months} mo`)
    return parts.join(' / ') || 'no interval'
  }

  function formatCents(cents: number | null): string {
    if (cents == null) return ''
    return formatCentsShared(cents)
  }
</script>

<div class="schedule-config">
  <div class="config-header">
    <h3>Schedule items</h3>
    <button class="btn btn-primary" onclick={() => (showForm ? (showForm = false) : startAdd())}>
      {showForm ? 'Cancel' : '+ Add item'}
    </button>
  </div>

  {#if error}
    <p class="error">{error}</p>
  {/if}

  {#if showForm}
    <div class="form-card">
      <form onsubmit={(e) => { e.preventDefault(); submit() }}>
        <div class="field">
          <label for="sched-name">Name</label>
          <input id="sched-name" type="text" bind:value={name} required placeholder="e.g., Oil change" />
        </div>
        <div class="form-row">
          <div class="field">
            <label for="sched-miles">Interval (miles)</label>
            <input id="sched-miles" type="number" min="1" bind:value={intervalMiles} placeholder="e.g., 5000" />
          </div>
          <div class="field">
            <label for="sched-months">Interval (months)</label>
            <input id="sched-months" type="number" min="1" bind:value={intervalMonths} placeholder="e.g., 12" />
          </div>
          <div class="field">
            <label for="sched-cost">Est. cost ($)</label>
            <input id="sched-cost" type="number" step="0.01" min="0" bind:value={estCost} placeholder="feeds the forecast" />
          </div>
        </div>
        <div class="field">
          <label for="sched-notes">Notes</label>
          <input id="sched-notes" type="text" bind:value={notes} />
        </div>
        <div class="form-actions">
          <button type="button" class="btn btn-secondary" onclick={() => (showForm = false)} disabled={saving}>Cancel</button>
          <button type="submit" class="btn btn-primary" disabled={saving}>
            {saving ? 'Saving…' : editingId ? 'Update' : 'Add'}
          </button>
        </div>
      </form>
    </div>
  {/if}

  {#if loading}
    <p>Loading schedule…</p>
  {:else if resolved.length === 0}
    <p class="empty">No schedule items yet — add one to start getting reminders.</p>
  {:else}
    <div class="item-list">
      {#each resolved as r (r.effective_item.id)}
        {@const item = r.effective_item}
        <div class="item-card" id={anchorId('schedule_item', item.id)}>
          <div class="item-main">
            <strong>{item.name}</strong>
            <span class="item-interval">{intervalText(item)}</span>
            {#if item.est_cost_cents != null}
              <span class="item-cost">{formatCents(item.est_cost_cents)}/occurrence</span>
            {/if}
          </div>
          <div class="item-meta">
            {#if r.inherited_from}
              <span class="inherited-badge">from {r.inherited_from}</span>
            {:else}
              <button class="action-link" onclick={() => startEdit(item)}>Edit</button>
              <button class="action-link delete" onclick={() => deleteItem(item)}>Delete</button>
            {/if}
          </div>
          {#if item.notes}
            <p class="item-notes">{item.notes}</p>
          {/if}
        </div>
      {/each}
    </div>
  {/if}

  {#if dismissedItems.length > 0}
    <section class="dismissed-section">
      <h3 class="dismissed-label">Dismissed</h3>
      {#each dismissedItems as item (item.id)}
        <div class="item-card dismissed" id={anchorId('schedule_item', item.id)}>
          <div class="item-main">
            <strong>{item.name}</strong>
            <span class="overridden-badge">overridden</span>
          </div>
          {#if item.notes}
            <p class="item-notes">{item.notes}</p>
          {/if}
          <div class="item-meta">
            <button class="action-link" onclick={() => reenableItem(item)}>Re-enable</button>
          </div>
        </div>
      {/each}
    </section>
  {/if}
</div>

<style>
  .config-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: var(--sp-4);
  }

  .config-header h3 {
    margin: 0;
  }

  .item-list {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
  }

  .item-card {
    padding: var(--sp-3) var(--sp-4);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-lg);
    background: var(--bg-raised);
    box-shadow: inset 0 1px 0 var(--edge-highlight);
  }

  .item-main {
    display: flex;
    align-items: baseline;
    gap: var(--sp-3);
    flex-wrap: wrap;
  }

  .item-interval,
  .item-cost {
    font-size: 0.82rem;
    color: var(--text-muted);
  }

  .item-meta {
    display: flex;
    gap: var(--sp-3);
    margin-top: var(--sp-1);
    align-items: center;
  }

  .item-notes {
    font-size: 0.82rem;
    color: var(--text-muted);
    margin: var(--sp-1) 0 0;
    font-style: italic;
  }

  .inherited-badge {
    font-family: var(--font-display);
    font-size: 0.68rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--text-muted);
    border: 1px solid var(--border);
    border-radius: 999px;
    padding: 1px var(--sp-2);
  }

  .action-link {
    padding: 0;
    border: none;
    background: none;
    font-size: 0.8rem;
    color: var(--primary);
    cursor: pointer;
    font-weight: 500;
  }

  .action-link:hover {
    text-decoration: underline;
  }

  .action-link.delete {
    color: var(--danger);
  }

  .dismissed-section {
    margin-top: var(--sp-6);
  }

  .dismissed-label {
    font-family: var(--font-display);
    font-size: 0.85rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    margin-bottom: var(--sp-2);
    color: var(--text-muted);
  }

  .item-card.dismissed {
    opacity: 0.6;
  }

  .overridden-badge {
    font-family: var(--font-display);
    font-size: 0.68rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--text-muted);
    border: 1px solid var(--border);
    border-radius: 999px;
    padding: 1px var(--sp-2);
  }

  .form-actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--sp-2);
  }

  .error {
    color: var(--danger);
    font-size: 0.85rem;
  }

  .empty {
    color: var(--text-muted);
    text-align: center;
    padding: var(--sp-8) 0;
  }
</style>
