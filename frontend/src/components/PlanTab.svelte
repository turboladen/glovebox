<script lang="ts">
  // Plan tab (unit F): the whole future in one place — sub-nav
  // Due / To-do / Visits / Research / Schedule ⚙. Due re-homes the
  // reminders view (ScheduleTab) with a "plan it" affordance; To-do is the
  // work-item backlog; Visits groups items into shop trips with the
  // complete/cancel lifecycle; Research re-homes ResearchTab (recalls and
  // findings are future work, so they live under Plan); Schedule ⚙ is the
  // schedule CRUD (ScheduleConfig).
  import { onMount } from 'svelte'
  import { push } from '@keenmate/svelte-spa-router'
  import {
    workItems as workItemsApi,
    visits as visitsApi,
    shops as shopsApi,
  } from '../lib/api'
  import type {
    RemindersResponse,
    ReminderStatus,
    Shop,
    VisitWithItems,
    WorkItem,
  } from '../lib/types'
  import { formatDate } from '../lib/dates'
  import { refreshDashboard } from '../lib/stores'
  import { anchorId, flashHighlightFromQuery } from '../lib/highlight'
  import ScheduleTab from './ScheduleTab.svelte'
  import ScheduleConfig from './ScheduleConfig.svelte'
  import ResearchTab from './ResearchTab.svelte'

  let { vehicleId, reminderData, sub = 'due', onScheduleChanged }: {
    vehicleId: number
    reminderData: RemindersResponse | null
    sub?: string
    onScheduleChanged?: () => Promise<void> | void
  } = $props()

  const subTabs = [
    { id: 'due', label: 'Due' },
    { id: 'todo', label: 'To-do' },
    { id: 'visits', label: 'Visits' },
    { id: 'research', label: 'Research' },
    { id: 'schedule', label: 'Schedule ⚙' },
  ]

  // Unknown :sub params fall back to Due instead of a blank pane.
  let activeSub = $derived(subTabs.some((t) => t.id === sub) ? sub : 'due')

  let items: WorkItem[] = $state([])
  let visitList: VisitWithItems[] = $state([])
  let shopList: Shop[] = $state([])
  let includeDone = $state(false)
  let includeClosed = $state(false)
  let loading = $state(true)
  let error = $state('')

  async function loadData() {
    try {
      const [i, v, s] = await Promise.all([
        workItemsApi.list(vehicleId, includeDone),
        visitsApi.list(vehicleId, includeClosed),
        shopsApi.list(),
      ])
      items = i
      visitList = v
      shopList = s
    } catch (e: any) {
      error = e.message
    } finally {
      loading = false
    }
  }

  onMount(loadData)

  // Deep-link highlight: e.g. the dashboard's "planned" chip links to
  // /plan/todo?hl=work_item:{id} (see lib/highlight.ts). Reactive on the
  // querystring so in-tab navigation (Due → To-do) highlights too.
  $effect(() => {
    if (activeSub === 'todo' && !loading) {
      flashHighlightFromQuery('work_item')
    }
  })

  async function refresh() {
    await loadData()
    await onScheduleChanged?.()
    refreshDashboard().catch(() => {})
  }

  function openSub(id: string) {
    push(`/vehicles/${vehicleId}/plan${id === 'due' ? '' : `/${id}`}`)
  }

  // schedule_item_id → linking work item id (first participating item
  // wins), so Due's "planned" chip can LINK to the work item.
  let plannedWorkItems = $derived.by(() => {
    const map = new Map<number, number>()
    for (const i of items) {
      if ((i.status === 'planned' || i.status === 'scheduled') && i.schedule_item_id != null && !map.has(i.schedule_item_id)) {
        map.set(i.schedule_item_id, i.id)
      }
    }
    return map
  })

  async function planIt(reminder: ReminderStatus) {
    await workItemsApi.create(vehicleId, {
      title: reminder.schedule_item.name,
      schedule_item_id: reminder.schedule_item.id,
    })
    await refresh()
  }

  // --- To-do (work items) ---

  let showItemForm = $state(false)
  let editingItem: WorkItem | null = $state(null)
  let itemTitle = $state('')
  let itemEstCost = $state('')
  let itemNotes = $state('')
  let itemSaving = $state(false)

  function startAddItem() {
    editingItem = null
    itemTitle = ''
    itemEstCost = ''
    itemNotes = ''
    showItemForm = true
  }

  function startEditItem(item: WorkItem) {
    editingItem = item
    itemTitle = item.title
    itemEstCost = item.est_cost_cents != null ? (item.est_cost_cents / 100).toFixed(2) : ''
    itemNotes = item.notes ?? ''
    showItemForm = true
  }

  async function submitItem() {
    if (!itemTitle.trim()) return
    itemSaving = true
    error = ''
    try {
      if (editingItem) {
        // Edit clears send explicit null (double-option update).
        await workItemsApi.update(vehicleId, editingItem.id, {
          title: itemTitle.trim(),
          est_cost_cents: itemEstCost ? Math.round(parseFloat(itemEstCost) * 100) : null,
          notes: itemNotes || null,
        })
      } else {
        await workItemsApi.create(vehicleId, {
          title: itemTitle.trim(),
          est_cost_cents: itemEstCost ? Math.round(parseFloat(itemEstCost) * 100) : undefined,
          notes: itemNotes || undefined,
        })
      }
      showItemForm = false
      editingItem = null
      await refresh()
    } catch (e: any) {
      error = e.message
    } finally {
      itemSaving = false
    }
  }

  async function setItemStatus(item: WorkItem, status: string) {
    error = ''
    try {
      await workItemsApi.update(vehicleId, item.id, { status })
      await refresh()
    } catch (e: any) {
      error = e.message
    }
  }

  async function deleteItem(item: WorkItem) {
    if (!confirm(`Delete "${item.title}" from the to-do list?`)) return
    error = ''
    try {
      await workItemsApi.delete(vehicleId, item.id)
      await refresh()
    } catch (e: any) {
      error = e.message
    }
  }

  type SourceBadge = { label: string; target: string }

  function sourceBadges(item: WorkItem): SourceBadge[] {
    const badges: SourceBadge[] = []
    if (item.schedule_item_id != null)
      badges.push({ label: 'schedule', target: `/vehicles/${vehicleId}/plan?hl=schedule_item:${item.schedule_item_id}` })
    if (item.research_finding_id != null)
      badges.push({ label: 'recall/finding', target: `/vehicles/${vehicleId}/plan/research?hl=finding:${item.research_finding_id}` })
    if (item.incident_id != null)
      badges.push({ label: 'incident', target: `/vehicles/${vehicleId}/timeline?hl=incident:${item.incident_id}` })
    if (item.build_id != null)
      badges.push({ label: 'build', target: `/vehicles/${vehicleId}/builds` })
    return badges
  }

  function visitLabel(visitId: number | null): string {
    if (visitId == null) return ''
    const v = visitList.find((x) => x.id === visitId)
    if (!v) return `visit #${visitId}`
    return `visit ${v.planned_date ? formatDate(v.planned_date) : `#${v.id}`}`
  }

  // --- Visits ---

  let showVisitForm = $state(false)
  let editingVisit: VisitWithItems | null = $state(null)
  let visitDate = $state('')
  let visitShopId = $state<number | ''>('')
  let visitShopName = $state('')
  let visitNotes = $state('')
  let visitItemIds: number[] = $state([])
  let visitSaving = $state(false)

  function startAddVisit() {
    editingVisit = null
    visitDate = ''
    visitShopId = ''
    visitShopName = ''
    visitNotes = ''
    visitItemIds = []
    showVisitForm = true
  }

  function startEditVisit(v: VisitWithItems) {
    editingVisit = v
    visitDate = v.planned_date ?? ''
    visitShopId = v.shop_id ?? ''
    // When a saved shop is selected, the select is authoritative — the
    // free-text field only holds a custom name (no saved shop). Selecting
    // a different shop on save replaces the stored name with that shop's.
    visitShopName = v.shop_id != null ? '' : (v.shop_name ?? '')
    visitNotes = v.notes ?? ''
    visitItemIds = v.items.filter((i) => participates(i)).map((i) => i.id)
    showVisitForm = true
  }

  function participates(item: WorkItem): boolean {
    return item.status === 'planned' || item.status === 'scheduled'
  }

  /** Items offered in the visit form: the unattached open backlog plus
   *  (when editing) the visit's own attached items. */
  let attachableItems = $derived(
    items.filter(
      (i) =>
        participates(i) &&
        (i.visit_id == null || (editingVisit != null && i.visit_id === editingVisit.id)),
    ),
  )

  function toggleVisitItem(id: number) {
    visitItemIds = visitItemIds.includes(id)
      ? visitItemIds.filter((x) => x !== id)
      : [...visitItemIds, id]
  }

  async function submitVisit() {
    visitSaving = true
    error = ''
    try {
      // A selected shop is authoritative: its name is stored as the
      // visit's shop_name. Free text applies only with no shop selected.
      const shop = shopList.find((s) => s.id === visitShopId)
      const shopName = shop ? shop.name : visitShopName.trim() || null
      if (editingVisit) {
        // Edit clears send explicit null; work_item_ids is replace-all.
        await visitsApi.update(vehicleId, editingVisit.id, {
          planned_date: visitDate || null,
          shop_id: visitShopId === '' ? null : visitShopId,
          shop_name: shopName,
          notes: visitNotes || null,
          work_item_ids: visitItemIds,
        })
      } else {
        await visitsApi.create(vehicleId, {
          planned_date: visitDate || null,
          shop_id: visitShopId === '' ? null : visitShopId,
          shop_name: shopName,
          notes: visitNotes || null,
          work_item_ids: visitItemIds,
        })
      }
      showVisitForm = false
      editingVisit = null
      await refresh()
    } catch (e: any) {
      error = e.message
    } finally {
      visitSaving = false
    }
  }

  async function cancelVisit(v: VisitWithItems) {
    if (!confirm('Cancel this visit? Its work items return to the to-do list.')) return
    error = ''
    try {
      await visitsApi.cancel(vehicleId, v.id)
      await refresh()
    } catch (e: any) {
      error = e.message
    }
  }

  // Complete-visit form
  let completingVisit: VisitWithItems | null = $state(null)
  let cDate = $state('')
  let cMileage = $state('')
  let cTotal = $state('')
  let cParts = $state('')
  let cLabor = $state('')
  let cPaidBy = $state('self')
  let cPayerNote = $state('')
  let cNotes = $state('')
  let cSaving = $state(false)

  function startComplete(v: VisitWithItems) {
    completingVisit = v
    cDate = new Date().toISOString().slice(0, 10)
    cMileage = ''
    cTotal = ''
    cParts = ''
    cLabor = ''
    cPaidBy = 'self'
    cPayerNote = ''
    cNotes = ''
  }

  async function submitComplete() {
    if (!completingVisit || !cDate) return
    cSaving = true
    error = ''
    try {
      await visitsApi.complete(vehicleId, completingVisit.id, {
        service_date: cDate,
        mileage: cMileage ? parseInt(cMileage, 10) : null,
        total_cost_cents: cTotal ? Math.round(parseFloat(cTotal) * 100) : null,
        parts_cost_cents: cParts ? Math.round(parseFloat(cParts) * 100) : null,
        labor_cost_cents: cLabor ? Math.round(parseFloat(cLabor) * 100) : null,
        paid_by: cPaidBy,
        payer_note: cPaidBy !== 'self' && cPayerNote ? cPayerNote : null,
        notes: cNotes || null,
      })
      completingVisit = null
      await refresh()
    } catch (e: any) {
      error = e.message
    } finally {
      cSaving = false
    }
  }

  function formatCents(cents: number | null): string {
    if (cents == null) return ''
    return `$${(cents / 100).toFixed(2)}`
  }

  let openItems = $derived(items.filter(participates))
  let doneItems = $derived(items.filter((i) => !participates(i)))
</script>

<div class="plan-tab">
  <div class="sub-nav">
    {#each subTabs as t (t.id)}
      <button class="sub-btn" class:active={activeSub === t.id} onclick={() => openSub(t.id)}>
        {t.label}
      </button>
    {/each}
  </div>

  {#if error}
    <p class="error">{error}</p>
  {/if}

  {#if activeSub === 'due'}
    <ScheduleTab
      {reminderData}
      {vehicleId}
      onScheduleChanged={refresh}
      {plannedWorkItems}
      onPlanIt={planIt}
    />
  {:else if activeSub === 'research'}
    <ResearchTab {vehicleId} />
  {:else if activeSub === 'schedule'}
    <ScheduleConfig {vehicleId} onChanged={refresh} />
  {:else if activeSub === 'todo'}
    <div class="section-header">
      <h3>To-do</h3>
      <div class="header-actions">
        <label class="toggle-label">
          <input
            type="checkbox"
            bind:checked={includeDone}
            onchange={() => { loading = true; loadData() }}
          />
          Show finished
        </label>
        <button class="btn btn-primary" onclick={() => (showItemForm ? (showItemForm = false) : startAddItem())}>
          {showItemForm ? 'Cancel' : '+ Add work item'}
        </button>
      </div>
    </div>

    {#if showItemForm}
      <div class="form-card">
        <form onsubmit={(e) => { e.preventDefault(); submitItem() }}>
          <div class="form-row">
            <div class="field grow">
              <label for="wi-title">Title</label>
              <input id="wi-title" type="text" bind:value={itemTitle} required placeholder="e.g., Replace wiper blades" />
            </div>
            <div class="field">
              <label for="wi-cost">Est. cost ($)</label>
              <input id="wi-cost" type="number" step="0.01" min="0" bind:value={itemEstCost} />
            </div>
          </div>
          <div class="field">
            <label for="wi-notes">Notes</label>
            <input id="wi-notes" type="text" bind:value={itemNotes} />
          </div>
          <div class="form-actions">
            <button type="button" class="btn btn-secondary" onclick={() => (showItemForm = false)} disabled={itemSaving}>Cancel</button>
            <button type="submit" class="btn btn-primary" disabled={itemSaving}>
              {itemSaving ? 'Saving…' : editingItem ? 'Update' : 'Add'}
            </button>
          </div>
        </form>
      </div>
    {/if}

    {#if loading}
      <p>Loading…</p>
    {:else if openItems.length === 0 && (!includeDone || doneItems.length === 0)}
      <p class="empty">Nothing on the to-do list. Plan work from the Due view, a recall, or an incident — or add one here.</p>
    {:else}
      <div class="item-list" data-testid="todo-list">
        {#each openItems as item (item.id)}
          <div class="work-card" id={anchorId('work_item', item.id)}>
            <div class="work-main">
              <span class="status-badge status-{item.status}">{item.status}</span>
              <strong class="work-title">{item.title}</strong>
              {#if item.est_cost_cents != null}
                <span class="work-cost">{formatCents(item.est_cost_cents)}</span>
              {/if}
            </div>
            {#if item.notes}
              <p class="work-notes">{item.notes}</p>
            {/if}
            <div class="work-meta">
              {#each sourceBadges(item) as b (b.label)}
                <button class="source-badge" onclick={() => push(b.target)}>{b.label}</button>
              {/each}
              {#if item.visit_id != null}
                <span class="visit-ref">{visitLabel(item.visit_id)}</span>
              {/if}
              <span class="work-actions">
                <button class="action-link" onclick={() => startEditItem(item)}>Edit</button>
                <button class="action-link" onclick={() => setItemStatus(item, 'dropped')}>Drop</button>
                <button class="action-link delete" onclick={() => deleteItem(item)}>Delete</button>
              </span>
            </div>
          </div>
        {/each}
        {#if includeDone}
          {#each doneItems as item (item.id)}
            <div class="work-card finished">
              <div class="work-main">
                <span class="status-badge status-{item.status}">{item.status}</span>
                <strong class="work-title">{item.title}</strong>
                {#if item.est_cost_cents != null}
                  <span class="work-cost">{formatCents(item.est_cost_cents)}</span>
                {/if}
              </div>
            </div>
          {/each}
        {/if}
      </div>
    {/if}
  {:else if activeSub === 'visits'}
    <div class="section-header">
      <h3>Visits</h3>
      <div class="header-actions">
        <label class="toggle-label">
          <input
            type="checkbox"
            bind:checked={includeClosed}
            onchange={() => { loading = true; loadData() }}
          />
          Show closed
        </label>
        <button class="btn btn-primary" onclick={() => (showVisitForm ? (showVisitForm = false) : startAddVisit())}>
          {showVisitForm ? 'Cancel' : '+ Schedule visit'}
        </button>
      </div>
    </div>

    {#if showVisitForm}
      <div class="form-card">
        <form onsubmit={(e) => { e.preventDefault(); submitVisit() }}>
          <div class="form-row">
            <div class="field">
              <label for="v-date">Planned date</label>
              <input id="v-date" type="date" bind:value={visitDate} />
            </div>
            <div class="field">
              <label for="v-shop">Shop</label>
              <select id="v-shop" bind:value={visitShopId}>
                <option value="">— DIY / not from list —</option>
                {#each shopList as s (s.id)}
                  <option value={s.id}>{s.name}</option>
                {/each}
              </select>
            </div>
            <div class="field">
              <label for="v-shop-name">Shop name (free text)</label>
              <input id="v-shop-name" type="text" bind:value={visitShopName} placeholder="used when no shop is selected" disabled={visitShopId !== ''} />
            </div>
          </div>
          {#if attachableItems.length > 0}
            <div class="field">
              <span class="field-label">Work items</span>
              <div class="attach-list">
                {#each attachableItems as item (item.id)}
                  <label class="attach-row">
                    <input
                      type="checkbox"
                      checked={visitItemIds.includes(item.id)}
                      onchange={() => toggleVisitItem(item.id)}
                    />
                    {item.title}
                    {#if item.est_cost_cents != null}
                      <span class="work-cost">{formatCents(item.est_cost_cents)}</span>
                    {/if}
                  </label>
                {/each}
              </div>
            </div>
          {/if}
          <div class="field">
            <label for="v-notes">Notes</label>
            <input id="v-notes" type="text" bind:value={visitNotes} />
          </div>
          <div class="form-actions">
            <button type="button" class="btn btn-secondary" onclick={() => (showVisitForm = false)} disabled={visitSaving}>Cancel</button>
            <button type="submit" class="btn btn-primary" disabled={visitSaving}>
              {visitSaving ? 'Saving…' : editingVisit ? 'Update visit' : 'Schedule visit'}
            </button>
          </div>
        </form>
      </div>
    {/if}

    {#if loading}
      <p>Loading…</p>
    {:else if visitList.length === 0}
      <p class="empty">No visits planned. Group to-do items into a shop trip or DIY session.</p>
    {:else}
      <div class="item-list" data-testid="visit-list">
        {#each visitList as v (v.id)}
          <div class="visit-card" class:closed={v.status === 'completed' || v.status === 'canceled'}>
            <div class="work-main">
              <span class="status-badge status-{v.status}">{v.status}</span>
              <strong>
                {v.planned_date ? formatDate(v.planned_date) : 'No date'}
                {#if v.shop_name}· {v.shop_name}{/if}
              </strong>
              {#if v.est_total_cents > 0}
                <span class="work-cost">est. {formatCents(v.est_total_cents)}</span>
              {/if}
            </div>
            {#if v.notes}
              <p class="work-notes">{v.notes}</p>
            {/if}
            {#if v.items.length > 0}
              <ul class="visit-items">
                {#each v.items as item (item.id)}
                  <li>
                    {item.title}
                    {#if item.est_cost_cents != null}<span class="work-cost">{formatCents(item.est_cost_cents)}</span>{/if}
                  </li>
                {/each}
              </ul>
            {/if}
            {#if v.status === 'planned' || v.status === 'scheduled'}
              <div class="work-meta">
                <span class="work-actions">
                  <button class="action-link" onclick={() => startEditVisit(v)}>Edit / attach items</button>
                  <button class="action-link" onclick={() => startComplete(v)}>Complete…</button>
                  <button class="action-link delete" onclick={() => cancelVisit(v)}>Cancel visit</button>
                </span>
              </div>
            {:else if v.status === 'completed'}
              <div class="work-meta">
                <button class="action-link" onclick={() => push(`/vehicles/${vehicleId}/timeline`)}>
                  service record →
                </button>
              </div>
            {/if}

            {#if completingVisit?.id === v.id}
              <form class="complete-form" onsubmit={(e) => { e.preventDefault(); submitComplete() }}>
                <div class="form-row">
                  <div class="field">
                    <label for="c-date">Service date</label>
                    <input id="c-date" type="date" bind:value={cDate} required />
                  </div>
                  <div class="field">
                    <label for="c-mileage">Odometer</label>
                    <input id="c-mileage" type="number" min="0" bind:value={cMileage} />
                  </div>
                  <div class="field">
                    <label for="c-total">Total ($)</label>
                    <input id="c-total" type="number" step="0.01" min="0" bind:value={cTotal} />
                  </div>
                </div>
                <div class="form-row">
                  <div class="field">
                    <label for="c-parts">Parts ($)</label>
                    <input id="c-parts" type="number" step="0.01" min="0" bind:value={cParts} />
                  </div>
                  <div class="field">
                    <label for="c-labor">Labor ($)</label>
                    <input id="c-labor" type="number" step="0.01" min="0" bind:value={cLabor} />
                  </div>
                  <div class="field">
                    <label for="c-paid-by">Paid by</label>
                    <select id="c-paid-by" bind:value={cPaidBy}>
                      <option value="self">Me</option>
                      <option value="insurance">Insurance</option>
                      <option value="third_party">Third party</option>
                    </select>
                  </div>
                </div>
                {#if cPaidBy !== 'self'}
                  <div class="field">
                    <label for="c-payer-note">Payer note</label>
                    <input id="c-payer-note" type="text" bind:value={cPayerNote} placeholder="e.g., Progressive claim #12345" />
                  </div>
                {/if}
                <div class="field">
                  <label for="c-notes">Notes</label>
                  <input id="c-notes" type="text" bind:value={cNotes} />
                </div>
                <div class="form-actions">
                  <button type="button" class="btn btn-secondary" onclick={() => (completingVisit = null)} disabled={cSaving}>Cancel</button>
                  <button type="submit" class="btn btn-primary" disabled={cSaving || !cDate}>
                    {cSaving ? 'Completing…' : 'Complete visit'}
                  </button>
                </div>
              </form>
            {/if}
          </div>
        {/each}
      </div>
    {/if}
  {/if}
</div>

<style>
  .sub-nav {
    display: flex;
    gap: var(--sp-1);
    margin-bottom: var(--sp-4);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    overflow: hidden;
    width: fit-content;
  }

  .sub-btn {
    padding: var(--sp-1) var(--sp-3);
    border: none;
    background: none;
    font-family: var(--font-display);
    font-size: 0.85rem;
    cursor: pointer;
    color: var(--text-muted);
    transition:
      background var(--duration-fast) var(--ease-out),
      color var(--duration-fast) var(--ease-out);
  }

  .sub-btn.active {
    background: var(--primary);
    color: var(--primary-text);
  }

  .section-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: var(--sp-3);
    margin-bottom: var(--sp-4);
  }

  .section-header h3 {
    margin: 0;
  }

  .header-actions {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
  }

  .toggle-label {
    display: inline-flex;
    align-items: center;
    gap: var(--sp-1);
    font-size: 0.8rem;
    color: var(--text-muted);
    margin: 0;
  }

  .toggle-label input {
    width: auto;
  }

  .item-list {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
  }

  .work-card,
  .visit-card {
    padding: var(--sp-3) var(--sp-4);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    background: var(--bg-raised);
  }

  .work-card.finished,
  .visit-card.closed {
    opacity: 0.6;
  }

  .work-main {
    display: flex;
    align-items: baseline;
    gap: var(--sp-2);
    flex-wrap: wrap;
  }

  .work-title {
    flex: 1;
  }

  .work-cost {
    font-size: 0.82rem;
    color: var(--text-secondary);
    font-weight: 600;
  }

  .work-notes {
    font-size: 0.82rem;
    color: var(--text-muted);
    margin: var(--sp-1) 0 0;
    font-style: italic;
  }

  .work-meta {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    margin-top: var(--sp-2);
    flex-wrap: wrap;
  }

  .work-actions {
    display: flex;
    gap: var(--sp-3);
    margin-left: auto;
  }

  .status-badge {
    font-family: var(--font-display);
    font-size: 0.65rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    padding: 0.1rem 0.4rem;
    border-radius: var(--radius-sm);
  }

  .status-planned { background: var(--info-bg); color: var(--info); }
  .status-scheduled { background: var(--warning-bg); color: var(--warning); }
  .status-done, .status-completed { background: var(--success-bg); color: var(--success); }
  .status-dropped, .status-canceled { background: var(--surface); color: var(--text-muted); }

  .source-badge {
    font-size: 0.7rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    padding: 0 var(--sp-1);
    border-radius: var(--radius-sm);
    background: var(--surface);
    color: var(--text-secondary);
    border: 1px solid var(--border);
    cursor: pointer;
  }

  .source-badge:hover {
    color: var(--primary);
    border-color: var(--primary);
  }

  .visit-ref {
    font-size: 0.75rem;
    color: var(--text-muted);
  }

  .visit-items {
    margin: var(--sp-2) 0 0;
    padding-left: var(--sp-5);
    font-size: 0.85rem;
    color: var(--text-secondary);
  }

  .visit-items li {
    margin-bottom: 2px;
  }

  .attach-list {
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    padding: var(--sp-2) var(--sp-3);
    background: var(--bg);
  }

  .attach-row {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    font-size: 0.85rem;
    margin: 0;
    color: var(--text);
  }

  .attach-row input {
    width: auto;
  }

  .complete-form {
    margin-top: var(--sp-3);
    padding: var(--sp-3);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg);
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

  .form-actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--sp-2);
    margin-top: var(--sp-2);
  }

  .field.grow {
    flex: 2;
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
