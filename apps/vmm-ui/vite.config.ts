import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'

// Use VITE_API_TARGET env var to switch between vmm-server (standalone) and vmm-cluster.
// Default: http://localhost:8443 (vmm-server)
// Cluster: VITE_API_TARGET=http://localhost:9443 npx vite
const apiTarget = process.env.VITE_API_TARGET || 'http://localhost:8443'

export default defineConfig({
  plugins: [react(), tailwindcss()],
  server: {
    port: 5173,
    proxy: {
      '/api': {
        target: apiTarget,
        changeOrigin: true,
      },
      '/ws': {
        target: apiTarget,
        ws: true,
        changeOrigin: true,
      },
    },
  },
})
