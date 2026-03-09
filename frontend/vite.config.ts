import { defineConfig } from 'vite'
import { svelte } from '@sveltejs/vite-plugin-svelte'

const apiTarget = process.env.VITE_API_TARGET ?? 'http://localhost:8000'

export default defineConfig({
  plugins: [svelte()],
  server: {
    proxy: {
      '/api': apiTarget,
      '/files': apiTarget,
    },
  },
})
