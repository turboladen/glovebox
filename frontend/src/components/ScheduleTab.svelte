<script lang="ts">
  import type { RemindersResponse, ReminderStatus } from '../lib/types'

  let { reminderData, vehicleId }: { reminderData: RemindersResponse | null; vehicleId: number } = $props()

  function groupByStatus(reminders: ReminderStatus[]) {
    return {
      overdue: reminders.filter((r) => r.status === 'overdue'),
      upcoming: reminders.filter((r) => r.status === 'upcoming'),
      ok: reminders.filter((r) => r.status === 'ok'),
    }
  }

  function formatMileage(n: number | null): string {
    return n != null ? n.toLocaleString() : '—'
  }

  function formatDate(d: string | null): string {
    return d ?? '—'
  }
</script>

{#if !reminderData}
  <p>No reminder data available.</p>
{:else}
  {@const groups = groupByStatus(reminderData.reminders)}

  {#if groups.overdue.length > 0}
    <section class="reminder-group">
      <h3 class="group-label overdue-label">Overdue</h3>
      {#each groups.overdue as reminder (reminder.schedule_item.id)}
        <div class="reminder-card overdue">
          <div class="reminder-header">
            <strong>{reminder.schedule_item.name}</strong>
            <span class="trigger">{reminder.trigger}</span>
          </div>
          <div class="reminder-details">
            {#if reminder.due_at_miles}
              <span>Due at {formatMileage(reminder.due_at_miles)} mi</span>
            {/if}
            {#if reminder.due_at_date}
              <span>or {formatDate(reminder.due_at_date)}</span>
            {/if}
          </div>
          <div class="reminder-meta">
            {#if reminder.miles_remaining != null}
              <span>{formatMileage(reminder.miles_remaining)} mi remaining</span>
            {/if}
            {#if reminder.days_remaining != null}
              <span>{reminder.days_remaining} days remaining</span>
            {/if}
          </div>
          {#if reminder.last_service}
            <div class="last-service">
              Last: {reminder.last_service.date}
              {#if reminder.last_service.odometer}@ {formatMileage(reminder.last_service.odometer)} mi{/if}
            </div>
          {:else}
            <div class="last-service">No service recorded</div>
          {/if}
        </div>
      {/each}
    </section>
  {/if}

  {#if groups.upcoming.length > 0}
    <section class="reminder-group">
      <h3 class="group-label upcoming-label">Upcoming</h3>
      {#each groups.upcoming as reminder (reminder.schedule_item.id)}
        <div class="reminder-card upcoming">
          <div class="reminder-header">
            <strong>{reminder.schedule_item.name}</strong>
          </div>
          <div class="reminder-details">
            {#if reminder.due_at_miles}
              <span>Due at {formatMileage(reminder.due_at_miles)} mi</span>
            {/if}
            {#if reminder.due_at_date}
              <span>or {formatDate(reminder.due_at_date)}</span>
            {/if}
          </div>
          <div class="reminder-meta">
            {#if reminder.miles_remaining != null}
              <span>~{formatMileage(reminder.miles_remaining)} mi</span>
            {/if}
            {#if reminder.days_remaining != null}
              <span>/ {reminder.days_remaining} days remaining</span>
            {/if}
          </div>
        </div>
      {/each}
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
      {#each groups.ok as reminder (reminder.schedule_item.id)}
        <div class="reminder-card ok">
          <div class="reminder-header">
            <strong>{reminder.schedule_item.name}</strong>
          </div>
          <div class="reminder-details">
            {#if reminder.due_at_miles}
              <span>Due at {formatMileage(reminder.due_at_miles)} mi</span>
            {/if}
            {#if reminder.due_at_date}
              <span>or {formatDate(reminder.due_at_date)}</span>
            {/if}
          </div>
        </div>
      {/each}
    </section>
  {/if}
{/if}

<style>
  .reminder-group {
    margin-bottom: 1.5rem;
  }

  .group-label {
    font-size: 0.85rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    margin-bottom: 0.5rem;
  }

  .overdue-label { color: var(--danger); }
  .upcoming-label { color: var(--warning); }
  .ok-label { color: var(--success); }
  .bundle-label { color: var(--primary); }

  .reminder-card {
    padding: 0.75rem 1rem;
    border-left: 3px solid var(--border);
    margin-bottom: 0.5rem;
    background: var(--surface);
    border-radius: 0 4px 4px 0;
  }

  .reminder-card.overdue { border-left-color: var(--danger); }
  .reminder-card.upcoming { border-left-color: var(--warning); }
  .reminder-card.ok { border-left-color: var(--success); }

  .reminder-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }

  .trigger {
    font-size: 0.75rem;
    color: var(--text-muted);
  }

  .reminder-details, .reminder-meta, .last-service {
    font-size: 0.85rem;
    color: var(--text-muted);
    margin-top: 0.25rem;
  }

  .bundle-card {
    padding: 0.75rem 1rem;
    background: var(--surface);
    border-radius: 4px;
    border: 1px dashed var(--primary);
    font-size: 0.85rem;
  }

  .bundle-card p {
    margin: 0;
  }
</style>
