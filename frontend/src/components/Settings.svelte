<script lang="ts">
  import { onMount } from 'svelte'
  import { aiProviders as providersApi, ai as aiApi } from '../lib/api'
  import type { AiProvider, ModelInfo } from '../lib/types'

  let loading = $state(true)
  let error = $state('')
  let success = $state('')
  let providers: AiProvider[] = $state([])

  // Form state
  let showForm = $state(false)
  let editingId: number | null = $state(null)
  let saving = $state(false)

  let formName = $state('')
  let formType = $state('claude')
  let formApiKey = $state('')
  let formApiBase = $state('')
  let formModel = $state('')
  let formIsDefault = $state(false)
  let formEnabled = $state(true)

  // Model fetching
  let availableModels: ModelInfo[] = $state([])
  let fetchingModels = $state(false)
  let modelsError = $state('')

  const DEFAULTS: Record<string, string> = {
    claude_model: 'claude-sonnet-4-6',
    openai_api_base: 'http://localhost:11434/v1',
    openai_model: 'llama3',
  }

  const TYPE_LABELS: Record<string, string> = {
    claude: 'Claude (Anthropic)',
    openai_compat: 'OpenAI-Compatible',
  }

  onMount(async () => {
    try {
      providers = await providersApi.list()
    } catch (e: any) {
      error = e.message
    } finally {
      loading = false
    }
  })

  function resetForm() {
    formName = ''
    formType = 'claude'
    formApiKey = ''
    formApiBase = ''
    formModel = ''
    formIsDefault = false
    formEnabled = true
    availableModels = []
    modelsError = ''
    editingId = null
    showForm = false
  }

  function startAdd() {
    resetForm()
    formIsDefault = providers.length === 0
    showForm = true
  }

  function startEdit(p: AiProvider) {
    editingId = p.id
    formName = p.name
    formType = p.provider_type
    formApiKey = ''  // Don't prefill API key for security
    formApiBase = p.api_base ?? ''
    formModel = p.model ?? ''
    formIsDefault = p.is_default
    formEnabled = p.enabled
    availableModels = []
    modelsError = ''
    showForm = true
  }

  async function fetchModels() {
    fetchingModels = true
    modelsError = ''
    try {
      const apiKey = formApiKey
      const apiBase = formType === 'openai_compat' ? (formApiBase || DEFAULTS.openai_api_base) : undefined
      const models = await aiApi.fetchModels(formType, apiKey, apiBase)
      models.sort((a, b) => (a.display_name || a.id).localeCompare(b.display_name || b.id))
      availableModels = models
      if (models.length > 0 && !formModel) {
        formModel = models[0].id
      }
    } catch (e: any) {
      modelsError = e.message
    } finally {
      fetchingModels = false
    }
  }

  async function saveProvider() {
    saving = true
    error = ''
    success = ''
    try {
      if (editingId) {
        const data: Record<string, any> = {
          name: formName,
          provider_type: formType,
          api_base: formType === 'openai_compat' ? (formApiBase || DEFAULTS.openai_api_base) : null,
          model: formModel || (formType === 'claude' ? DEFAULTS.claude_model : DEFAULTS.openai_model),
          is_default: formIsDefault,
          enabled: formEnabled,
        }
        // Only send api_key if user entered a new one
        if (formApiKey) {
          data.api_key = formApiKey
        }
        await providersApi.update(editingId, data)
        success = 'Provider updated.'
      } else {
        await providersApi.create({
          name: formName,
          provider_type: formType,
          api_key: formApiKey || undefined,
          api_base: formType === 'openai_compat' ? (formApiBase || DEFAULTS.openai_api_base) : undefined,
          model: formModel || (formType === 'claude' ? DEFAULTS.claude_model : DEFAULTS.openai_model),
          is_default: formIsDefault,
          enabled: formEnabled,
        })
        success = 'Provider added.'
      }
      providers = await providersApi.list()
      resetForm()
    } catch (e: any) {
      error = e.message
    } finally {
      saving = false
    }
  }

  async function deleteProvider(id: number) {
    error = ''
    success = ''
    try {
      await providersApi.delete(id)
      providers = await providersApi.list()
      success = 'Provider removed.'
      if (editingId === id) resetForm()
    } catch (e: any) {
      error = e.message
    }
  }

  async function setDefault(id: number) {
    error = ''
    try {
      await providersApi.update(id, { is_default: true })
      providers = await providersApi.list()
    } catch (e: any) {
      error = e.message
    }
  }

  async function toggleEnabled(p: AiProvider) {
    error = ''
    try {
      await providersApi.update(p.id, { enabled: !p.enabled })
      providers = await providersApi.list()
    } catch (e: any) {
      error = e.message
    }
  }
</script>

<div class="settings">
  <div class="settings-header">
    <h1>Settings</h1>
  </div>

  {#if loading}
    <div class="form-card">
      <div class="skeleton skeleton-heading" style="width: 40%"></div>
      <div class="field">
        <div class="skeleton skeleton-text-short"></div>
        <div class="skeleton skeleton-text"></div>
      </div>
    </div>
  {:else}
    {#if error}
      <p class="error">{error}</p>
    {/if}

    {#if success}
      <div class="notice notice-success">{success}</div>
    {/if}

    <div class="form-card">
      <div class="section-header">
        <h4>AI Providers</h4>
        {#if !showForm}
          <button class="btn btn-sm btn-primary" onclick={startAdd}>Add Provider</button>
        {/if}
      </div>

      {#if providers.length === 0 && !showForm}
        <p class="empty-hint">No AI providers configured. Add one to enable AI features.</p>
      {/if}

      {#if providers.length > 0}
        <div class="provider-list">
          {#each providers as p (p.id)}
            <div class="provider-row" class:disabled={!p.enabled}>
              <div class="provider-info">
                <div class="provider-name">
                  {p.name}
                  {#if p.is_default}
                    <span class="badge badge-default">Default</span>
                  {/if}
                </div>
                <div class="provider-meta">
                  {TYPE_LABELS[p.provider_type] ?? p.provider_type}
                  {#if p.model}
                    &middot; {p.model}
                  {/if}
                  {#if p.api_key_set}
                    &middot; Key set
                  {/if}
                </div>
              </div>
              <div class="provider-actions">
                {#if !p.is_default && p.enabled}
                  <button class="btn btn-xs btn-ghost" onclick={() => setDefault(p.id)}>Set Default</button>
                {/if}
                <button
                  class="btn btn-xs btn-ghost"
                  onclick={() => toggleEnabled(p)}
                >{p.enabled ? 'Disable' : 'Enable'}</button>
                <button class="btn btn-xs btn-ghost" onclick={() => startEdit(p)}>Edit</button>
                <button class="btn btn-xs btn-ghost btn-danger" onclick={() => deleteProvider(p.id)}>Delete</button>
              </div>
            </div>
          {/each}
        </div>
      {/if}

      {#if showForm}
        <form class="provider-form" onsubmit={(e) => { e.preventDefault(); saveProvider() }}>
          <h5>{editingId ? 'Edit Provider' : 'Add Provider'}</h5>

          <div class="field">
            <label for="provider-name">Name</label>
            <input id="provider-name" type="text" bind:value={formName} placeholder="e.g. Claude, Ollama Local" required />
          </div>

          <div class="field">
            <label for="provider-type">Type</label>
            <select id="provider-type" bind:value={formType}>
              <option value="claude">Claude (Anthropic)</option>
              <option value="openai_compat">OpenAI-Compatible (Ollama / LM Studio)</option>
            </select>
          </div>

          <div class="field">
            <label for="provider-api-key">
              API Key
              {#if formType === 'openai_compat'}
                <span class="optional">(optional)</span>
              {/if}
              {#if editingId}
                <span class="optional">(leave blank to keep existing)</span>
              {/if}
            </label>
            <input
              id="provider-api-key"
              type="password"
              bind:value={formApiKey}
              placeholder={formType === 'claude' ? 'sk-ant-...' : 'Not required for local servers'}
              autocomplete="off"
            />
          </div>

          {#if formType === 'openai_compat'}
            <div class="field">
              <label for="provider-api-base">API Base URL</label>
              <input
                id="provider-api-base"
                type="url"
                bind:value={formApiBase}
                placeholder={DEFAULTS.openai_api_base}
              />
            </div>
          {/if}

          <div class="field">
            <label for="provider-model">Model</label>
            {#if availableModels.length > 0}
              <select id="provider-model" bind:value={formModel}>
                {#each availableModels as model}
                  <option value={model.id}>{model.display_name || model.id}</option>
                {/each}
              </select>
            {:else}
              <input
                id="provider-model"
                type="text"
                bind:value={formModel}
                placeholder={formType === 'claude' ? DEFAULTS.claude_model : DEFAULTS.openai_model}
              />
            {/if}
            <button
              type="button"
              class="btn btn-sm btn-secondary"
              disabled={fetchingModels || (formType === 'claude' && !formApiKey)}
              onclick={fetchModels}
            >
              {fetchingModels ? 'Fetching...' : 'Fetch Models'}
            </button>
            {#if modelsError}
              <span class="field-error">{modelsError}</span>
            {/if}
          </div>

          <div class="field-row">
            <label class="checkbox-label">
              <input type="checkbox" bind:checked={formIsDefault} />
              Set as default provider
            </label>
            <label class="checkbox-label">
              <input type="checkbox" bind:checked={formEnabled} />
              Enabled
            </label>
          </div>

          <div class="form-actions">
            <button type="submit" class="btn btn-primary" disabled={saving || !formName.trim()}>
              {saving ? 'Saving...' : editingId ? 'Update' : 'Add'}
            </button>
            <button type="button" class="btn btn-secondary" onclick={resetForm}>Cancel</button>
          </div>
        </form>
      {/if}
    </div>
  {/if}
</div>

<style>
  .settings-header {
    margin-bottom: var(--sp-6);
  }

  .settings-header h1 {
    margin: 0;
  }

  .form-card {
    animation: fade-in-down var(--duration-base) var(--ease-out) both;
  }

  .section-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: var(--sp-4);
  }

  .section-header h4 {
    margin: 0;
  }

  .empty-hint {
    color: var(--text-muted);
    font-size: 0.85rem;
    text-align: center;
    padding: var(--sp-6) 0;
  }

  .provider-list {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    margin-bottom: var(--sp-4);
  }

  .provider-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--sp-3);
    padding: var(--sp-3) var(--sp-4);
    background: var(--surface);
    border-radius: var(--radius-md);
    border: 1px solid var(--border-subtle);
  }

  .provider-row.disabled {
    opacity: 0.5;
  }

  .provider-info {
    flex: 1;
    min-width: 0;
  }

  .provider-name {
    font-weight: 600;
    font-size: 0.9rem;
    display: flex;
    align-items: center;
    gap: var(--sp-2);
  }

  .provider-meta {
    font-size: 0.75rem;
    color: var(--text-muted);
    margin-top: 2px;
  }

  .badge-default {
    display: inline-block;
    padding: 1px 6px;
    font-size: 0.65rem;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    background: var(--primary);
    color: var(--primary-text);
    border-radius: var(--radius-sm);
  }

  .provider-actions {
    display: flex;
    gap: var(--sp-1);
    flex-shrink: 0;
  }

  .btn-xs {
    padding: 2px 8px;
    font-size: 0.7rem;
  }

  .btn-ghost {
    background: transparent;
    border: 1px solid var(--border-subtle);
    color: var(--text-secondary);
  }

  .btn-ghost:hover {
    background: var(--surface);
    border-color: var(--border);
  }

  .btn-ghost.btn-danger {
    color: var(--danger);
  }

  .btn-ghost.btn-danger:hover {
    background: var(--danger-bg);
    border-color: var(--danger);
  }

  .provider-form {
    margin-top: var(--sp-4);
    padding-top: var(--sp-4);
    border-top: 1px solid var(--border-subtle);
    animation: fade-in-down var(--duration-fast) var(--ease-out) both;
  }

  .provider-form h5 {
    margin: 0 0 var(--sp-3) 0;
    font-size: 0.9rem;
  }

  .field-row {
    display: flex;
    gap: var(--sp-6);
    margin-top: var(--sp-3);
  }

  .checkbox-label {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    font-size: 0.85rem;
    cursor: pointer;
  }

  .checkbox-label input[type="checkbox"] {
    width: auto;
    margin: 0;
  }

  .field-error {
    display: block;
    font-size: 0.75rem;
    color: var(--danger);
    margin-top: var(--sp-1);
  }

  .field .btn-sm {
    margin-top: var(--sp-2);
  }

  .optional {
    font-weight: 400;
    color: var(--text-muted);
    font-size: 0.75rem;
  }

  .form-actions {
    margin-top: var(--sp-4);
    padding-top: var(--sp-4);
    border-top: 1px solid var(--border-subtle);
    display: flex;
    gap: var(--sp-2);
  }

  .notice-success {
    padding: var(--sp-3) var(--sp-4);
    background: var(--success-bg);
    border: 1px solid var(--success-border);
    border-radius: var(--radius-md);
    color: var(--success);
    font-size: 0.85rem;
    margin-bottom: var(--sp-4);
    animation: fade-in-down var(--duration-fast) var(--ease-out) both;
  }

  .error {
    color: var(--danger);
    padding: var(--sp-3) var(--sp-4);
    background: var(--danger-bg);
    border: 1px solid var(--danger-border);
    border-radius: var(--radius-md);
    margin-bottom: var(--sp-4);
  }

  @media (max-width: 640px) {
    .settings-header {
      margin-bottom: var(--sp-4);
    }

    .provider-row {
      flex-direction: column;
      align-items: flex-start;
    }

    .provider-actions {
      flex-wrap: wrap;
    }

    .field-row {
      flex-direction: column;
      gap: var(--sp-2);
    }
  }
</style>
