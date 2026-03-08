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
      configured = status.providers.some(p => p.enabled)
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
    margin-bottom: var(--sp-6);
    padding: var(--sp-4);
    border: 1px dashed var(--border);
    border-radius: var(--radius-md);
    background: var(--bg-raised);
  }

  .suggestions-card h4 {
    margin: 0 0 var(--sp-3);
    font-family: var(--font-display);
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
    gap: var(--sp-2);
  }

  .suggestion {
    padding: var(--sp-3);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    transition: border-color var(--duration-base) var(--ease-out);
  }

  .suggestion:hover {
    border-color: var(--border);
  }

  .suggestion-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: var(--sp-1);
  }

  .suggestion-title {
    font-weight: 600;
    font-size: 0.9rem;
  }

  .urgency-badge {
    font-family: var(--font-display);
    font-size: 0.7rem;
    font-weight: 600;
    text-transform: uppercase;
    padding: 0.1rem 0.4rem;
    border-radius: var(--radius-sm);
  }

  .urgency-high {
    background: var(--danger-bg);
    color: var(--danger);
    border: 1px solid var(--danger-border);
  }

  .urgency-medium {
    background: var(--warning-bg);
    color: var(--warning);
    border: 1px solid var(--warning-border);
  }

  .urgency-low {
    background: var(--success-bg);
    color: var(--success);
    border: 1px solid var(--success-border);
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
    margin-top: var(--sp-1);
    display: inline-block;
  }
</style>
