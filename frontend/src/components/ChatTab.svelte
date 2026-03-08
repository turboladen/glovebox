<script lang="ts">
  import { onMount } from 'svelte'
  import { ai } from '../lib/api'
  import type { ChatMessage, AiStatus } from '../lib/types'

  let { vehicleId }: { vehicleId: number } = $props()

  let messages: ChatMessage[] = $state([])
  let input = $state('')
  let sending = $state(false)
  let aiStatus: AiStatus | null = $state(null)
  let loading = $state(true)
  let messagesContainer: HTMLElement

  onMount(async () => {
    try {
      aiStatus = await ai.status()
      if (aiStatus.configured) {
        messages = await ai.chatHistory(vehicleId)
      }
    } catch (e) {
      console.error('Failed to load AI status:', e)
    } finally {
      loading = false
    }
  })

  function scrollToBottom() {
    if (messagesContainer) {
      setTimeout(() => {
        messagesContainer.scrollTop = messagesContainer.scrollHeight
      }, 0)
    }
  }

  async function send() {
    if (!input.trim() || sending) return
    const msg = input.trim()
    input = ''
    sending = true

    // Optimistically add user message
    messages = [...messages, {
      id: 0,
      vehicle_id: vehicleId,
      role: 'user',
      content: msg,
      created_at: new Date().toISOString(),
    }]
    scrollToBottom()

    try {
      const resp = await ai.chat(vehicleId, msg)
      // Replace optimistic messages with real data
      messages = [...messages.slice(0, -1), {
        id: resp.message.id - 1,
        vehicle_id: vehicleId,
        role: 'user',
        content: msg,
        created_at: resp.message.created_at,
      }, resp.message]
      scrollToBottom()
    } catch (e: any) {
      // Remove optimistic message and show error
      messages = messages.slice(0, -1)
      messages = [...messages, {
        id: 0,
        vehicle_id: vehicleId,
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
</script>

<div class="chat-tab">
  {#if loading}
    <p>Loading...</p>
  {:else if !aiStatus?.configured}
    <div class="not-configured">
      <p>AI is not configured.</p>
      <p class="hint">Set an AI provider in Settings to enable chat.</p>
    </div>
  {:else}
    <div class="chat-messages" bind:this={messagesContainer}>
      {#if messages.length === 0}
        <p class="empty">No messages yet. Ask about your vehicle!</p>
      {/if}
      {#each messages as msg (msg.id + msg.created_at)}
        <div class="message {msg.role}">
          <div class="message-bubble">
            <div class="message-content">{msg.content}</div>
          </div>
        </div>
      {/each}
      {#if sending}
        <div class="message assistant">
          <div class="message-bubble">
            <div class="message-content thinking">Thinking...</div>
          </div>
        </div>
      {/if}
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

  .chat-messages {
    flex: 1;
    overflow-y: auto;
    padding: var(--sp-4) 0;
    display: flex;
    flex-direction: column;
    gap: var(--sp-3);
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
  }

  .chat-input {
    display: flex;
    gap: var(--sp-2);
    padding-top: var(--sp-3);
    border-top: 1px solid var(--border-subtle);
  }

  .chat-input textarea {
    flex: 1;
    resize: none;
  }

  .chat-input button {
    align-self: flex-end;
  }
</style>
