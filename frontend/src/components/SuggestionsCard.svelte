<script lang="ts">
  import { onMount } from 'svelte'
  import { ai } from '../lib/api'
  import type { AiSuggestion } from '../lib/types'

  let { vehicleId }: { vehicleId: number } = $props()

  let suggestions: AiSuggestion[] = $state([])
  let loading = $state(true)
  let configured = $state(false)

  onMount(async () => {
    try {
      const status = await ai.status()
      configured = status.configured
      if (configured) {
        suggestions = await ai.suggestions(vehicleId)
      }
    } catch (e) {
      console.error('Failed to load suggestions:', e)
    } finally {
      loading = false
    }
  })

  function urgencyClass(urgency: string): string {
    switch (urgency) {
      case 'high': return 'urgency-high'
      case 'medium': return 'urgency-medium'
      case 'low': return 'urgency-low'
      default: return ''
    }
  }
</script>

{#if configured}
  <div class="suggestions-card">
    <h4>AI Suggestions</h4>
    {#if loading}
      <p class="loading-text">Analyzing your vehicle data...</p>
    {:else if suggestions.length === 0}
      <p class="empty">No suggestions at this time.</p>
    {:else}
      <div class="suggestions-list">
        {#each suggestions as s}
          <div class="suggestion">
            <div class="suggestion-header">
              <span class="suggestion-title">{s.title}</span>
              <span class="urgency-badge {urgencyClass(s.urgency)}">{s.urgency}</span>
            </div>
            <p class="suggestion-reason">{s.reason}</p>
            {#if s.estimated_cost_range}
              <span class="cost-range">Est. {s.estimated_cost_range}</span>
            {/if}
          </div>
        {/each}
      </div>
    {/if}
  </div>
{/if}

<style>
  .suggestions-card {
    margin-bottom: 1.5rem;
    padding: 1rem;
    border: 1px dashed var(--border);
    border-radius: 8px;
    background: var(--surface);
  }

  .suggestions-card h4 {
    margin: 0 0 0.75rem;
    font-size: 0.95rem;
  }

  .loading-text {
    color: var(--text-muted);
    font-style: italic;
    font-size: 0.85rem;
  }

  .empty {
    color: var(--text-muted);
    font-size: 0.85rem;
  }

  .suggestions-list {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }

  .suggestion {
    padding: 0.6rem;
    border: 1px solid var(--border);
    border-radius: 6px;
  }

  .suggestion-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 0.25rem;
  }

  .suggestion-title {
    font-weight: 600;
    font-size: 0.9rem;
  }

  .urgency-badge {
    font-size: 0.7rem;
    font-weight: 600;
    text-transform: uppercase;
    padding: 0.1rem 0.4rem;
    border-radius: 3px;
  }

  .urgency-high {
    background: #fef2f2;
    color: #dc2626;
    border: 1px solid #fecaca;
  }

  .urgency-medium {
    background: #fffbeb;
    color: #d97706;
    border: 1px solid #fde68a;
  }

  .urgency-low {
    background: #f0fdf4;
    color: #16a34a;
    border: 1px solid #bbf7d0;
  }

  .suggestion-reason {
    margin: 0;
    font-size: 0.85rem;
    color: var(--text-muted);
    line-height: 1.4;
  }

  .cost-range {
    font-size: 0.8rem;
    color: var(--text-muted);
    margin-top: 0.25rem;
    display: inline-block;
  }
</style>
