import { defineConfig } from 'vite';

// Рулсеты лежат в общей папке assets/ (вне web/) и раздаются как статика:
//   assets/rulesets/core.yaml  ->  http://localhost:5173/rulesets/core.yaml
export default defineConfig({
  root: '.',
  publicDir: '../assets',
  server: { port: 5173 },
  worker: { format: 'es' },
});
