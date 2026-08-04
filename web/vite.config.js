import { defineConfig } from 'vite';

// Рулсеты лежат в общей папке assets/ (вне web/) и раздаются как статика:
//   assets/rulesets/core.yaml  ->  http://localhost:5173/rulesets/core.yaml
export default defineConfig({
  root: '.',
  publicDir: '../assets',
  // Порт можно занять снаружи (PORT): 5173 бывает занят соседним запуском, а
  // адрес превью должен совпадать с тем, что вправду слушает Vite.
  server: { port: Number(process.env.PORT) || 5173 },
  worker: { format: 'es' },
});
