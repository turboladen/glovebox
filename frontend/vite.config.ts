import { defineConfig } from 'vite'
import { svelte } from '@sveltejs/vite-plugin-svelte'

const apiTarget = process.env.VITE_API_TARGET ?? 'http://localhost:3003'

export default defineConfig({
  plugins: [svelte()],
  server: {
    port: 5373,
    strictPort: true,
    proxy: {
      '/api': apiTarget,
      '/files': apiTarget,
    },
  },
})
