<script lang="ts">
  import { onMount } from 'svelte'
  import { marked } from 'marked'
  import { ai, conversations as convosApi } from '../lib/api'
  import type { ChatMessage, AiStatus, Conversation } from '../lib/types'
  import AiProviderSelect from './AiProviderSelect.svelte'
  import ProposedActionsCard from './ProposedActionsCard.svelte'

  /** Extract glovebox_actions JSON from an assistant message.
   *  Returns the parsed actions object and the message content with the JSON block removed. */
  function extractActions(content: string): { text: string; actions: any | null } {
    const re = /```glovebox_actions\s*\n([\s\S]*?)```/
    const match = content.match(re)
    if (!match) return { text: content, actions: null }
    try {
      const parsed = JSON.parse(match[1])
      const actions = parsed.glovebox_actions ?? parsed
      const text = content.replace(re, '').trim()
      return { text, actions }
    } catch {
      return { text: content, actions: null }
    }
  }

  // Configure marked for safe, synchronous rendering
  marked.setOptions({ async: false, breaks: true })

  let { vehicleId, initialDocumentId, initialMessage }: {
    vehicleId: number
    initialDocumentId?: number
    initialMessage?: string
  } = $props()

  let convos: Conversation[] = $state([])
  let activeConvoId: number | null = $state(null)
  let messages: ChatMessage[] = $state([])
  let input = $state('')
  let sending = $state(false)
  let nextOptimisticId = $state(-1)
  let aiStatus: AiStatus | null = $state(null)
  let loading = $state(true)
  let loadingMessages = $state(false)
  let messagesContainer: HTMLElement | undefined = $state(undefined)
  let selectedProviderId: number | undefined = $state(undefined)
  let sidebarCollapsed = $state(false)
  let editingConvoId: number | null = $state(null)
  let editingTitle = $state('')
  let pendingDocumentId: number | undefined = $state(undefined)

  onMount(async () => {
    try {
      aiStatus = await ai.status()
      if (aiStatus.providers.some(p => p.enabled)) {
        convos = await convosApi.list(vehicleId)

        // If launched with a document to analyze, create a new conversation and auto-send
        if (initialDocumentId && initialMessage) {
          pendingDocumentId = initialDocumentId
          const convo = await convosApi.create(vehicleId, 'Document Analysis')
          convos = [convo, ...convos]
          await selectConversation(convo.id)
          input = initialMessage
          // Auto-send after mount
          setTimeout(() => send(), 0)
        } else if (convos.length > 0) {
          // Auto-select most recent conversation
          await selectConversation(convos[0].id)
        }
      }
    } catch (e) {
      console.error('Failed to load AI status:', e)
    } finally {
      loading = false
    }
  })

  function scrollToBottom() {
    const el = messagesContainer
    if (el) {
      setTimeout(() => {
        el.scrollTop = el.scrollHeight
      }, 0)
    }
  }

  async function selectConversation(id: number) {
    activeConvoId = id
    loadingMessages = true
    try {
      messages = await convosApi.messages(vehicleId, id)
      scrollToBottom()
    } catch (e) {
      console.error('Failed to load messages:', e)
      messages = []
    } finally {
      loadingMessages = false
    }
  }

  async function newChat() {
    try {
      const convo = await convosApi.create(vehicleId)
      convos = [convo, ...convos]
      await selectConversation(convo.id)
    } catch (e: any) {
      console.error('Failed to create conversation:', e)
    }
  }

  async function deleteConversation(id: number) {
    try {
      await convosApi.delete(vehicleId, id)
      convos = convos.filter(c => c.id !== id)
      if (activeConvoId === id) {
        if (convos.length > 0) {
          await selectConversation(convos[0].id)
        } else {
          activeConvoId = null
          messages = []
        }
      }
    } catch (e: any) {
      console.error('Failed to delete conversation:', e)
    }
  }

  function startRename(convo: Conversation) {
    editingConvoId = convo.id
    editingTitle = convo.title
  }

  async function finishRename() {
    if (editingConvoId == null || !editingTitle.trim()) {
      editingConvoId = null
      return
    }
    try {
      const updated = await convosApi.rename(vehicleId, editingConvoId, editingTitle.trim())
      convos = convos.map(c => c.id === updated.id ? updated : c)
    } catch (e: any) {
      console.error('Failed to rename:', e)
    } finally {
      editingConvoId = null
    }
  }

  async function send() {
    if (!input.trim() || sending || activeConvoId == null) return
    const msg = input.trim()
    input = ''
    sending = true

    // Optimistically add user message
    const tempId = nextOptimisticId--
    messages = [...messages, {
      id: tempId,
      vehicle_id: vehicleId,
      conversation_id: activeConvoId,
      role: 'user',
      content: msg,
      created_at: new Date().toISOString(),
    }]
    scrollToBottom()

    try {
      const docId = pendingDocumentId
      pendingDocumentId = undefined
      const resp = await ai.chat(vehicleId, activeConvoId, msg, selectedProviderId, docId)
      // Replace optimistic user message and append assistant
      messages = [...messages.slice(0, -1), {
        ...messages[messages.length - 1],
        created_at: resp.message.created_at,
      }, resp.message]
      scrollToBottom()

      // Update conversation in sidebar (title may have auto-changed, updated_at changed)
      const refreshed = await convosApi.list(vehicleId)
      convos = refreshed
    } catch (e: any) {
      messages = messages.slice(0, -1)
      messages = [...messages, {
        id: 0,
        vehicle_id: vehicleId,
        conversation_id: activeConvoId,
        role: 'assistant',
        content: `Error: ${e.message}`,
        created_at: new Date().toISOString(),
      }]
      scrollToBottom()
    } finally {
      sending = false
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault()
      send()
    }
  }

  function handleRenameKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      e.preventDefault()
      finishRename()
    } else if (e.key === 'Escape') {
      editingConvoId = null
    }
  }
</script>

<div class="chat-tab">
  {#if loading}
    <p>Loading...</p>
  {:else if !aiStatus?.providers.some(p => p.enabled)}
    <div class="not-configured">
      <p>AI is not configured.</p>
      <p class="hint">Set an AI provider in Settings to enable chat.</p>
    </div>
  {:else}
    <div class="chat-layout">
      <!-- Sidebar -->
      <div class="chat-sidebar" class:collapsed={sidebarCollapsed}>
        <div class="sidebar-header">
          {#if !sidebarCollapsed}
            <button class="btn btn-primary btn-sm" onclick={newChat}>+ New Chat</button>
          {/if}
          <button class="btn btn-ghost btn-sm toggle-btn" onclick={() => (sidebarCollapsed = !sidebarCollapsed)}
            title={sidebarCollapsed ? 'Expand sidebar' : 'Collapse sidebar'}>
            {sidebarCollapsed ? '▶' : '◀'}
          </button>
        </div>
        {#if !sidebarCollapsed}
          <div class="convo-list">
            {#each convos as convo (convo.id)}
              <div
                class="convo-item"
                class:active={convo.id === activeConvoId}
                onclick={() => selectConversation(convo.id)}
                role="button"
                tabindex="0"
                onkeydown={(e) => { if (e.key === 'Enter') selectConversation(convo.id) }}
              >
                {#if editingConvoId === convo.id}
                  <input
                    class="rename-input"
                    bind:value={editingTitle}
                    onblur={finishRename}
                    onkeydown={handleRenameKeydown}
                  />
                {:else}
                  <span class="convo-title">{convo.title}</span>
                  <span class="convo-actions">
                    <button
                      class="convo-action-btn"
                      title="Rename"
                      onclick={(e: MouseEvent) => { e.stopPropagation(); startRename(convo) }}
                    >✎</button>
                    <button
                      class="convo-action-btn convo-action-delete"
                      title="Delete"
                      onclick={(e: MouseEvent) => { e.stopPropagation(); deleteConversation(convo.id) }}
                    >×</button>
                  </span>
                {/if}
              </div>
            {/each}
            {#if convos.length === 0}
              <p class="empty-sidebar">No conversations yet</p>
            {/if}
          </div>
        {/if}
      </div>

      <!-- Main chat area -->
      <div class="chat-main">
        {#if activeConvoId == null}
          <div class="no-convo">
            <p>Start a new conversation to chat about your vehicle.</p>
            <button class="btn btn-primary" onclick={newChat}>+ New Chat</button>
          </div>
        {:else if loadingMessages}
          <p class="loading-messages">Loading messages...</p>
        {:else}
          <div class="chat-messages" bind:this={messagesContainer}>
            {#if messages.length === 0}
              <p class="empty">No messages yet. Ask about your vehicle!</p>
            {/if}
            {#each messages as msg (msg.id + msg.created_at)}
              {@const extracted = msg.role === 'assistant' ? extractActions(msg.content) : null}
              <div class="message {msg.role}">
                <div class="message-bubble">
                  {#if msg.role === 'assistant'}
                    <div class="message-content markdown">{@html marked.parse(extracted?.text ?? msg.content)}</div>
                  {:else}
                    <div class="message-content">{msg.content}</div>
                  {/if}
                </div>
              </div>
              {#if extracted?.actions}
                <ProposedActionsCard {vehicleId} actionsJson={extracted.actions} />
              {/if}
            {/each}
            {#if sending}
              <div class="message assistant">
                <div class="message-bubble">
                  <div class="message-content thinking">Thinking...</div>
                </div>
              </div>
            {/if}
          </div>

          <div class="chat-controls">
            <AiProviderSelect bind:selectedProviderId />
          </div>
          <div class="chat-input">
            <textarea
              bind:value={input}
              onkeydown={handleKeydown}
              placeholder="Ask about your vehicle..."
              rows="2"
              disabled={sending}
            ></textarea>
            <button class="btn btn-primary" onclick={send} disabled={sending || !input.trim()}>
              {sending ? '...' : 'Send'}
            </button>
          </div>
        {/if}
      </div>
    </div>
  {/if}
</div>

<style>
  .chat-tab {
    display: flex;
    flex-direction: column;
    height: 500px;
  }

  .not-configured {
    text-align: center;
    padding: var(--sp-12) var(--sp-4);
    color: var(--text-muted);
  }

  .not-configured p:first-child {
    font-family: var(--font-display);
    font-size: 1.1rem;
    font-weight: 600;
  }

  .hint {
    font-size: 0.85rem;
  }

  /* Layout */
  .chat-layout {
    display: flex;
    flex: 1;
    min-height: 0;
  }

  /* Sidebar */
  .chat-sidebar {
    width: 220px;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    background: var(--bg-raised);
    border-right: 1px solid var(--border);
    transition: width 0.2s ease;
  }

  .chat-sidebar.collapsed {
    width: 36px;
  }

  .sidebar-header {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    padding: var(--sp-2);
    border-bottom: 1px solid var(--border);
  }

  .sidebar-header .btn-sm {
    font-size: 0.75rem;
    padding: var(--sp-1) var(--sp-2);
  }

  .toggle-btn {
    margin-left: auto;
    font-size: 0.7rem;
    opacity: 0.6;
  }

  .convo-list {
    flex: 1;
    overflow-y: auto;
    padding: var(--sp-1);
  }

  .convo-item {
    display: flex;
    align-items: center;
    gap: var(--sp-1);
    padding: var(--sp-2);
    border-radius: var(--radius-sm);
    cursor: pointer;
    font-size: 0.8rem;
    transition: background 0.15s ease;
  }

  .convo-item:hover {
    background: var(--surface);
  }

  .convo-item.active {
    background: var(--primary-muted);
    font-weight: 500;
  }

  .convo-title {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .convo-actions {
    display: flex;
    gap: 2px;
    opacity: 0;
    transition: opacity 0.15s ease;
  }

  .convo-item.active .convo-actions {
    opacity: 1;
  }

  .convo-item:hover .convo-actions {
    opacity: 1;
  }

  @media (hover: none) {
    .convo-actions {
      opacity: 1;
    }
  }

  .convo-action-btn {
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    font-size: 0.85rem;
    line-height: 1;
    padding: 2px 4px;
    border-radius: var(--radius-sm);
    transition: background 0.15s ease, color 0.15s ease;
  }

  .convo-action-btn:hover {
    background: var(--surface-hover);
    color: var(--text);
  }

  .convo-action-delete:hover {
    color: var(--danger);
    background: var(--danger-bg);
  }

  .rename-input {
    flex: 1;
    font-size: 0.8rem;
    padding: var(--sp-1);
    border: 1px solid var(--primary);
    border-radius: var(--radius-sm);
    background: var(--surface);
  }

  .empty-sidebar {
    text-align: center;
    color: var(--text-muted);
    font-size: 0.75rem;
    padding: var(--sp-4) var(--sp-2);
  }

  /* Main chat area */
  .chat-main {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
    background: var(--bg-base);
  }

  .no-convo {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    flex: 1;
    gap: var(--sp-4);
    color: var(--text-muted);
  }

  .loading-messages {
    text-align: center;
    color: var(--text-muted);
    padding: var(--sp-8) 0;
  }

  .chat-messages {
    flex: 1;
    overflow-y: auto;
    padding: var(--sp-4);
    display: flex;
    flex-direction: column;
    gap: var(--sp-3);
    user-select: text;
    -webkit-user-select: text;
  }

  .empty {
    text-align: center;
    color: var(--text-muted);
    padding: var(--sp-8) 0;
  }

  .message {
    display: flex;
  }

  .message.user {
    justify-content: flex-end;
  }

  .message.assistant {
    justify-content: flex-start;
  }

  .message-bubble {
    max-width: 75%;
    padding: var(--sp-3) var(--sp-4);
    border-radius: var(--radius-lg);
    font-size: 0.9rem;
    line-height: 1.4;
    box-shadow: var(--shadow-sm);
  }

  .message.user .message-bubble {
    background: var(--primary);
    color: var(--primary-text);
    border-bottom-right-radius: var(--radius-sm);
  }

  .message.assistant .message-bubble {
    background: var(--bg-raised);
    border: 1px solid var(--border-subtle);
    border-bottom-left-radius: var(--radius-sm);
  }

  .thinking {
    color: var(--text-muted);
    font-style: italic;
  }

  .message-content {
    white-space: pre-wrap;
    word-break: break-word;
    user-select: text;
    -webkit-user-select: text;
    cursor: text;
  }

  .message-content.markdown {
    white-space: normal;
  }

  .message-content.markdown :global(p) {
    margin: 0 0 0.5em;
  }

  .message-content.markdown :global(p:last-child) {
    margin-bottom: 0;
  }

  .message-content.markdown :global(ul),
  .message-content.markdown :global(ol) {
    margin: 0.25em 0;
    padding-left: 1.5em;
  }

  .message-content.markdown :global(li) {
    margin: 0.15em 0;
  }

  .message-content.markdown :global(code) {
    background: var(--bg-sunken, rgba(0, 0, 0, 0.08));
    padding: 0.1em 0.35em;
    border-radius: var(--radius-sm);
    font-size: 0.85em;
  }

  .message-content.markdown :global(pre) {
    background: var(--bg-sunken, rgba(0, 0, 0, 0.08));
    padding: var(--sp-2) var(--sp-3);
    border-radius: var(--radius-md);
    overflow-x: auto;
    margin: 0.5em 0;
  }

  .message-content.markdown :global(pre code) {
    background: none;
    padding: 0;
  }

  .message-content.markdown :global(h1),
  .message-content.markdown :global(h2),
  .message-content.markdown :global(h3) {
    font-size: 1em;
    font-weight: 700;
    margin: 0.75em 0 0.25em;
  }

  .message-content.markdown :global(h1:first-child),
  .message-content.markdown :global(h2:first-child),
  .message-content.markdown :global(h3:first-child) {
    margin-top: 0;
  }

  .message-content.markdown :global(strong) {
    font-weight: 600;
  }

  .chat-controls {
    display: flex;
    justify-content: flex-end;
    padding: var(--sp-2) var(--sp-4);
    border-top: 1px solid var(--border-subtle);
  }

  .chat-input {
    display: flex;
    gap: var(--sp-2);
    padding: 0 var(--sp-4) var(--sp-2);
  }

  .chat-input textarea {
    flex: 1;
    resize: none;
  }

  .chat-input button {
    align-self: flex-end;
  }

  /* Responsive: collapse sidebar on narrow screens */
  @media (max-width: 640px) {
    .chat-sidebar {
      width: 36px;
    }
    .chat-sidebar .convo-list,
    .chat-sidebar .sidebar-header .btn-primary {
      display: none;
    }
    .chat-sidebar.collapsed {
      width: 36px;
    }
  }
</style>
