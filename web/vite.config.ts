import { defineConfig } from 'vite';
import solid from 'vite-plugin-solid';

export default defineConfig({
  plugins: [solid()],
  worker: { format: 'es' },
  build: { target: 'es2023', outDir: 'dist', emptyOutDir: true },
  server: { proxy: { '/git': 'http://localhost:8787' } },
});
