<script lang="ts">
  import { onMount } from 'svelte'
  import { link, push } from '@keenmate/svelte-spa-router'
  import { services as servicesApi, schedules as schedulesApi } from '../lib/api'
  import { formatCents as formatCentsShared } from '../lib/money'
  import type { RemindersResponse, ReminderStatus, ServiceRecordWithLinks } from '../lib/types'
  import { formatDate } from '../lib/dates'
  import { anchorId, flashHighlightFromQuery } from '../lib/highlight'

  let { reminderData, vehicleId, onScheduleChanged, plannedWorkItems, onPlanIt, onUnplanIt }: {
    reminderData: RemindersResponse | null
    vehicleId: number
    onScheduleChanged?: () => Promise<void> | void
    // schedule_item_id → the open work item already linking it (Plan tab
    // wiring) — the "planned" chip links to that work item; inVisit hides
    // the ⊖ unplan (deleting would silently strip it from the visit).
    plannedWorkItems?: Map<number, { id: number; inVisit: boolean }>
    onPlanIt?: (reminder: ReminderStatus) => Promise<void> | void
    onUnplanIt?: (workItemId: number) => Promise<void> | void
  } = $props()

  let allServices: ServiceRecordWithLinks[] = $state([])
  let expandedItemId: number | null = $state(null)
  let actionError = $state('')

  async function loadOwnData() {
    try {
      allServices = await servicesApi.list(vehicleId)
    } catch (e) {
      console.error(e)
    }
  }

  onMount(loadOwnData)

  // Deep-link highlight (?hl=schedule_item:N from dashboard attention
  // rows / to-do source badges) once the reminder cards have rendered.
  let flashedHighlight = false
  $effect(() => {
    if (reminderData && !flashedHighlight) {
      flashedHighlight = true
      flashHighlightFromQuery('schedule_item')
    }
  })

  async function refresh() {
    await loadOwnData()
    await onScheduleChanged?.()
  }

  // Record service… / Mark done previously route to the ONE real
  // ServiceForm on the Timeline, prefilled (round-2 feedback #9) — the
  // stripped mini-forms produced records the user never saw take shape.
  // TimelineTab consumes these params the same way as ?action=record.
  function recordService(reminder: ReminderStatus, retro: boolean) {
    const params = new URLSearchParams({
      action: 'record',
      schedule_item: String(reminder.schedule_item.id),
      desc: reminder.schedule_item.name,
    })
    if (retro) params.set('retro', '1')
    push(`/vehicles/${vehicleId}/timeline?${params}`)
  }

  async function dismissItem(reminder: ReminderStatus) {
    actionError = ''
    try {
      await schedulesApi.dismiss(vehicleId, reminder.schedule_item.id)
      await refresh()
    } catch (e: any) {
      actionError = e.message
    }
  }

  // ⊖ unplan (round-3 feedback #5): same labeled affordance as the
  // dashboard's attention rows — delete the linking work item, the row
  // reverts to "Plan it". Confirm-free because it's cheaply reversible.
  let unplanningId: number | null = $state(null)

  async function unplanIt(workItemId: number) {
    actionError = ''
    unplanningId = workItemId
    try {
      await onUnplanIt?.(workItemId)
    } catch (e: any) {
      actionError = e.message
    } finally {
      unplanningId = null
    }
  }

  // "Link existing service…" (round-3 feedback #6): retroactive
  // reconciliation — the work already happened, it just isn't linked.
  // Picking a record PUTs schedule_item_ids = its existing links + this
  // item (the update endpoint replaces links wholesale), clearing the
  // reminder without inventing a new record.
  let linkPickerFor: number | null = $state(null)
  let linkingServiceId: number | null = $state(null)

  let servicesNewestFirst = $derived(
    [...allServices].sort((a, b) => b.service_date.localeCompare(a.service_date)),
  )

  function toggleLinkPicker(reminder: ReminderStatus) {
    actionError = ''
    linkPickerFor = linkPickerFor === reminder.schedule_item.id ? null : reminder.schedule_item.id
  }

  async function linkExistingService(reminder: ReminderStatus, svc: ServiceRecordWithLinks) {
    actionError = ''
    linkingServiceId = svc.id
    try {
      if (!svc.schedule_item_ids.includes(reminder.schedule_item.id)) {
        await servicesApi.update(vehicleId, svc.id, {
          schedule_item_ids: [...svc.schedule_item_ids, reminder.schedule_item.id],
        })
      }
      linkPickerFor = null
      await refresh()
    } catch (e: any) {
      actionError = e.message
    } finally {
      linkingServiceId = null
    }
  }

  /** Description for the "Last done" reference (last_service carries only
   *  id/date/odometer; the loaded service list has the words). */
  function serviceDescription(serviceId: number): string | null {
    return allServices.find((s) => s.id === serviceId)?.description ?? null
  }

  function groupByStatus(reminders: ReminderStatus[]) {
    return {
      overdue: reminders.filter((r) => r.status === 'overdue'),
      upcoming: reminders.filter((r) => r.status === 'upcoming'),
      ok: reminders.filter((r) => r.status === 'ok'),
    }
  }

  function servicesForItem(itemId: number): ServiceRecordWithLinks[] {
    return allServices
      .filter(s => s.schedule_item_ids.includes(itemId))
      .sort((a, b) => b.service_date.localeCompare(a.service_date))
  }

  function formatMileage(n: number | null): string {
    return n != null ? n.toLocaleString() : '—'
  }

  function formatCents(cents: number | null): string {
    if (cents == null) return ''
    return formatCentsShared(cents)
  }

</script>

{#snippet reminderActions(reminder: ReminderStatus)}
  <div class="reminder-actions">
    {#if onPlanIt}
      {#if plannedWorkItems?.has(reminder.schedule_item.id)}
        {@const planned = plannedWorkItems.get(reminder.schedule_item.id)!}
        <!-- Hypermedia: the state display links to the work item itself. -->
        <a
          class="planned-chip"
          href="/vehicles/{vehicleId}/plan/todo?hl=work_item:{planned.id}"
          use:link
          title="View the planned work item"
        >
          planned
        </a>
        {#if onUnplanIt && !planned.inVisit}
          <button
            class="unplan"
            title="Un-plan (removes the to-do item)"
            aria-label="Un-plan"
            onclick={() => unplanIt(planned.id)}
            disabled={unplanningId === planned.id}
          >
            <span aria-hidden="true">⊖</span> unplan
          </button>
        {/if}
      {:else}
        <button class="action-link" onclick={() => onPlanIt(reminder)}>Plan it</button>
      {/if}
    {/if}
    <button class="action-link" onclick={() => recordService(reminder, false)}>Record service…</button>
    <button class="action-link" onclick={() => recordService(reminder, true)}>Mark done previously</button>
    <button class="action-link" onclick={() => toggleLinkPicker(reminder)}>Link existing service…</button>
    <button class="action-link dismiss" onclick={() => dismissItem(reminder)}>Dismiss for this vehicle</button>
  </div>
  {#if linkPickerFor === reminder.schedule_item.id}
    <!-- Compact picker: the vehicle's records, newest first. Choosing one
         links it to this schedule item and the reminder recomputes. -->
    <div class="link-picker" data-testid="link-picker">
      <div class="picker-label">Link a service record to “{reminder.schedule_item.name}”</div>
      {#if servicesNewestFirst.length === 0}
        <p class="picker-empty">No service records on this vehicle yet.</p>
      {:else}
        <div class="picker-rows">
          {#each servicesNewestFirst as svc (svc.id)}
            {@const alreadyLinked = svc.schedule_item_ids.includes(reminder.schedule_item.id)}
            <button
              class="picker-row"
              disabled={alreadyLinked || linkingServiceId != null}
              onclick={() => linkExistingService(reminder, svc)}
            >
              <span class="picker-date num">{formatDate(svc.service_date)}</span>
              <span class="picker-desc">{svc.description || 'Service'}</span>
              <span class="picker-miles num">
                {#if svc.mileage != null}{formatMileage(svc.mileage)} mi{/if}
              </span>
              {#if alreadyLinked}
                <span class="picker-linked">linked</span>
              {:else if linkingServiceId === svc.id}
                <span class="picker-linking">linking…</span>
              {/if}
            </button>
          {/each}
        </div>
      {/if}
    </div>
  {/if}
{/snippet}

{#snippet lastDone(reminder: ReminderStatus)}
  {#if reminder.last_service}
    {@const desc = serviceDescription(reminder.last_service.id)}
    <div class="last-service">
      <!-- Hypermedia: the last-done reference deep-links to the record on
           the Timeline (?hl= flashes the exact row). -->
      <a
        class="last-link"
        href="/vehicles/{vehicleId}/timeline?hl=service:{reminder.last_service.id}"
        use:link
        title="View this service on the Timeline"
      >
        Last done {formatDate(reminder.last_service.date)}{#if desc}&nbsp;— {desc}{/if}{#if reminder.last_service.odometer}&nbsp;@ {formatMileage(reminder.last_service.odometer)} mi{/if}
      </a>
    </div>
  {:else}
    <div class="last-service">No service recorded</div>
  {/if}
{/snippet}

{#if !reminderData}
  <p>No reminder data available.</p>
{:else}
  {@const groups = groupByStatus(reminderData.reminders)}

  {#if actionError}
    <p class="action-error">{actionError}</p>
  {/if}

  {#if groups.overdue.length > 0}
    <section class="reminder-group">
      <h3 class="group-label overdue-label">Overdue</h3>
      <div class="ledger">
      {#each groups.overdue as reminder (reminder.schedule_item.id)}
        <div class="reminder-card overdue" id={anchorId('schedule_item', reminder.schedule_item.id)}>
          <div class="reminder-header">
            <strong>{reminder.schedule_item.name}</strong>
            <span class="due-readout num">
              {#if reminder.due_at_miles}Due at {formatMileage(reminder.due_at_miles)} mi{/if}
              {#if reminder.due_at_miles && reminder.due_at_date}·{/if}
              {#if reminder.due_at_date}{formatDate(reminder.due_at_date)}{/if}
            </span>
          </div>
          <div class="reminder-meta">
            <span class="trigger">{reminder.trigger}</span>
            {#if reminder.miles_remaining != null}
              <span>{formatMileage(reminder.miles_remaining)} mi remaining</span>
            {/if}
            {#if reminder.days_remaining != null}
              <span>{reminder.days_remaining} days remaining</span>
            {/if}
          </div>
          {@render lastDone(reminder)}
          {#if servicesForItem(reminder.schedule_item.id).length > 0}
            <button class="history-toggle" onclick={() => expandedItemId = expandedItemId === reminder.schedule_item.id ? null : reminder.schedule_item.id}>
              {servicesForItem(reminder.schedule_item.id).length} completion{servicesForItem(reminder.schedule_item.id).length !== 1 ? 's' : ''}
              {expandedItemId === reminder.schedule_item.id ? '▾' : '▸'}
            </button>
          {/if}
          {#if expandedItemId === reminder.schedule_item.id}
            <div class="completion-history">
              {#each servicesForItem(reminder.schedule_item.id) as svc (svc.id)}
                <div class="completion-row">
                  <span>{formatDate(svc.service_date)}</span>
                  {#if svc.mileage}<span>{formatMileage(svc.mileage)} mi</span>{/if}
                  {#if svc.total_cost_cents}<span>{formatCents(svc.total_cost_cents)}</span>{/if}
                  {#if svc.shop_name}<span class="shop">at {svc.shop_name}</span>{/if}
                </div>
              {/each}
            </div>
          {/if}
          {@render reminderActions(reminder)}
        </div>
      {/each}
      </div>
    </section>
  {/if}

  {#if groups.upcoming.length > 0}
    <section class="reminder-group">
      <h3 class="group-label upcoming-label">Upcoming</h3>
      <div class="ledger">
      {#each groups.upcoming as reminder (reminder.schedule_item.id)}
        <div class="reminder-card upcoming" id={anchorId('schedule_item', reminder.schedule_item.id)}>
          <div class="reminder-header">
            <strong>{reminder.schedule_item.name}</strong>
            <span class="due-readout num">
              {#if reminder.due_at_miles}Due at {formatMileage(reminder.due_at_miles)} mi{/if}
              {#if reminder.due_at_miles && reminder.due_at_date}·{/if}
              {#if reminder.due_at_date}{formatDate(reminder.due_at_date)}{/if}
            </span>
          </div>
          <div class="reminder-meta">
            {#if reminder.miles_remaining != null}
              <span>~{formatMileage(reminder.miles_remaining)} mi</span>
            {/if}
            {#if reminder.days_remaining != null}
              <span>/ {reminder.days_remaining} days remaining</span>
            {/if}
          </div>
          {@render lastDone(reminder)}
          {#if servicesForItem(reminder.schedule_item.id).length > 0}
            <button class="history-toggle" onclick={() => expandedItemId = expandedItemId === reminder.schedule_item.id ? null : reminder.schedule_item.id}>
              {servicesForItem(reminder.schedule_item.id).length} completion{servicesForItem(reminder.schedule_item.id).length !== 1 ? 's' : ''}
              {expandedItemId === reminder.schedule_item.id ? '▾' : '▸'}
            </button>
          {/if}
          {#if expandedItemId === reminder.schedule_item.id}
            <div class="completion-history">
              {#each servicesForItem(reminder.schedule_item.id) as svc (svc.id)}
                <div class="completion-row">
                  <span>{formatDate(svc.service_date)}</span>
                  {#if svc.mileage}<span>{formatMileage(svc.mileage)} mi</span>{/if}
                  {#if svc.total_cost_cents}<span>{formatCents(svc.total_cost_cents)}</span>{/if}
                  {#if svc.shop_name}<span class="shop">at {svc.shop_name}</span>{/if}
                </div>
              {/each}
            </div>
          {/if}
          {@render reminderActions(reminder)}
        </div>
      {/each}
      </div>
    </section>
  {/if}

  {#if reminderData.bundle_suggestions.length > 0}
    <section class="bundles">
      <h3 class="group-label bundle-label">Bundle Suggestions</h3>
      {#each reminderData.bundle_suggestions as suggestion}
        <div class="bundle-card">
          <p>{suggestion.reason}</p>
        </div>
      {/each}
    </section>
  {/if}

  {#if groups.ok.length > 0}
    <section class="reminder-group">
      <h3 class="group-label ok-label">OK (not yet due)</h3>
      <div class="ledger">
      {#each groups.ok as reminder (reminder.schedule_item.id)}
        <div class="reminder-card ok" id={anchorId('schedule_item', reminder.schedule_item.id)}>
          <div class="reminder-header">
            <strong>{reminder.schedule_item.name}</strong>
            <span class="due-readout num">
              {#if reminder.due_at_miles}Due at {formatMileage(reminder.due_at_miles)} mi{/if}
              {#if reminder.due_at_miles && reminder.due_at_date}·{/if}
              {#if reminder.due_at_date}{formatDate(reminder.due_at_date)}{/if}
            </span>
          </div>
          {#if reminder.last_service}
            {@render lastDone(reminder)}
          {/if}
          {#if servicesForItem(reminder.schedule_item.id).length > 0}
            <button class="history-toggle" onclick={() => expandedItemId = expandedItemId === reminder.schedule_item.id ? null : reminder.schedule_item.id}>
              {servicesForItem(reminder.schedule_item.id).length} completion{servicesForItem(reminder.schedule_item.id).length !== 1 ? 's' : ''}
              {expandedItemId === reminder.schedule_item.id ? '▾' : '▸'}
            </button>
          {/if}
          {#if expandedItemId === reminder.schedule_item.id}
            <div class="completion-history">
              {#each servicesForItem(reminder.schedule_item.id) as svc (svc.id)}
                <div class="completion-row">
                  <span>{formatDate(svc.service_date)}</span>
                  {#if svc.mileage}<span>{formatMileage(svc.mileage)} mi</span>{/if}
                  {#if svc.total_cost_cents}<span>{formatCents(svc.total_cost_cents)}</span>{/if}
                  {#if svc.shop_name}<span class="shop">at {svc.shop_name}</span>{/if}
                </div>
              {/each}
            </div>
          {/if}
        </div>
      {/each}
      </div>
    </section>
  {/if}

{/if}

<style>
  .reminder-group {
    margin-bottom: var(--sp-6);
  }

  .group-label {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    font-family: var(--font-display);
    font-size: 0.8rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.13em;
    margin-bottom: var(--sp-2);
  }

  .group-label::after {
    content: '';
    flex: 1;
    height: 1px;
    background: var(--border-subtle);
  }

  .overdue-label { color: var(--danger); }
  .upcoming-label { color: var(--warning); }
  .ok-label { color: var(--success); }
  .bundle-label { color: var(--primary); }

  /* Ledger rows (round-2 feedback #5): each group is ONE card of
     hairline-ruled entries with a status rail — the same row grammar as
     the dashboard's attention table and the Timeline. */
  .reminder-card {
    padding: var(--sp-3) var(--sp-4);
    border-left: 3px solid var(--border-subtle);
    background: none;
    transition: background var(--duration-fast) var(--ease-out);
  }

  .reminder-card:hover {
    background: var(--surface);
  }

  .reminder-card.overdue { border-left-color: var(--danger); }
  .reminder-card.upcoming { border-left-color: var(--warning); }
  .reminder-card.ok { border-left-color: var(--success); }

  .reminder-header {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: var(--sp-3);
  }

  .reminder-header strong {
    font-family: var(--font-display);
    font-size: 1.02rem;
    font-weight: 600;
    letter-spacing: 0.015em;
  }

  /* The due figures: mono, right-aligned — the ledger's number column. */
  .due-readout {
    font-size: 0.8rem;
    color: var(--text-secondary);
    text-align: right;
    white-space: nowrap;
  }

  .trigger {
    font-size: 0.75rem;
    color: var(--text-muted);
  }

  .reminder-meta, .last-service {
    font-size: 0.82rem;
    color: var(--text-muted);
    margin-top: 2px;
    display: flex;
    gap: var(--sp-3);
  }

  /* The last-done reference is hypermedia: it goes to the record. */
  .last-link {
    color: var(--text-muted);
    text-decoration: none;
    transition: color var(--duration-fast) var(--ease-out);
  }

  .last-link:hover {
    color: var(--text);
    text-decoration: underline;
    text-decoration-color: var(--primary);
    text-underline-offset: 3px;
  }

  .bundle-card {
    padding: var(--sp-3) var(--sp-4);
    background: var(--bg-raised);
    border-radius: var(--radius-sm);
    border: 1px dashed var(--primary);
    font-size: 0.85rem;
  }

  .bundle-card p {
    margin: 0;
  }

  .history-toggle {
    display: inline-flex;
    align-items: center;
    gap: var(--sp-1);
    margin-top: var(--sp-2);
    padding: 0;
    border: none;
    background: none;
    font-size: 0.8rem;
    color: var(--primary);
    cursor: pointer;
    font-weight: 500;
  }

  .history-toggle:hover {
    text-decoration: underline;
  }

  .completion-history {
    margin-top: var(--sp-2);
    padding-left: var(--sp-3);
    border-left: 2px solid var(--border-subtle);
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
  }

  .completion-row {
    display: flex;
    gap: var(--sp-3);
    font-size: 0.8rem;
    color: var(--text-muted);
  }

  .completion-row .shop {
    font-style: italic;
  }

  .reminder-actions {
    display: flex;
    gap: var(--sp-3);
    margin-top: var(--sp-2);
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

  .action-link.dismiss {
    color: var(--text-muted);
  }

  .planned-chip {
    font-family: var(--font-display);
    font-size: 0.68rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.07em;
    padding: 0 var(--sp-2);
    border-radius: 999px;
    background: var(--planned-bg);
    color: var(--planned);
    border: 1px solid var(--planned-border);
    align-self: center;
    text-decoration: none;
  }

  a.planned-chip:hover {
    text-decoration: underline;
    border-color: var(--planned);
    color: var(--planned);
  }

  /* "⊖ unplan": same explicit control as the dashboard's attention rows. */
  .unplan {
    padding: 0;
    border: none;
    background: none;
    font-size: 0.74rem;
    font-weight: 500;
    line-height: 1.4;
    color: var(--text-muted);
    cursor: pointer;
    white-space: nowrap;
    align-self: center;
  }

  .unplan:hover {
    color: var(--danger);
    text-decoration: underline;
  }

  .unplan:disabled {
    opacity: 0.5;
    cursor: default;
  }

  /* Compact link-existing picker: a small ledger of the vehicle's
     records — date | description | miles, newest first. */
  .link-picker {
    margin-top: var(--sp-2);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    background: var(--bg);
    overflow: hidden;
    animation: fade-in-down var(--duration-base) var(--ease-out) both;
  }

  .picker-label {
    font-family: var(--font-display);
    font-size: 0.72rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.1em;
    color: var(--text-muted);
    padding: var(--sp-2) var(--sp-3) var(--sp-1);
  }

  .picker-rows {
    display: flex;
    flex-direction: column;
    max-height: 220px;
    overflow-y: auto;
  }

  .picker-row {
    display: grid;
    grid-template-columns: max-content minmax(0, 1fr) max-content max-content;
    column-gap: var(--sp-3);
    align-items: baseline;
    width: 100%;
    padding: var(--sp-1) var(--sp-3);
    border: none;
    border-top: 1px dotted var(--border-subtle);
    background: none;
    color: var(--text-secondary);
    font-size: 0.82rem;
    text-align: left;
    cursor: pointer;
    transition:
      background var(--duration-fast) var(--ease-out),
      color var(--duration-fast) var(--ease-out);
  }

  .picker-row:hover:not(:disabled) {
    background: var(--surface);
    color: var(--text);
  }

  .picker-row:disabled {
    cursor: default;
    opacity: 0.55;
  }

  .picker-date {
    color: var(--text);
    white-space: nowrap;
  }

  .picker-desc {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .picker-miles {
    white-space: nowrap;
    text-align: right;
  }

  .picker-linked,
  .picker-linking {
    font-family: var(--font-display);
    font-size: 0.66rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.07em;
    padding: 0 var(--sp-2);
    border-radius: 999px;
    background: var(--success-bg);
    color: var(--success);
    border: 1px solid var(--success-border);
  }

  .picker-linking {
    background: var(--surface);
    color: var(--text-muted);
    border-color: var(--border-subtle);
  }

  .picker-empty {
    margin: 0;
    padding: var(--sp-2) var(--sp-3) var(--sp-3);
    font-size: 0.82rem;
    color: var(--text-muted);
  }

  .action-error {
    color: var(--danger);
    font-size: 0.85rem;
  }
</style>
