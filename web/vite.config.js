import { defineConfig } from 'vite';

// Рулсеты лежат в общей папке assets/ (вне web/) и раздаются как статика:
//   assets/rulesets/core.yaml  ->  http://localhost:5173/rulesets/core.yaml
export default defineConfig(({ command }) => ({
  root: '.',
  publicDir: '../assets',
  // Dev работает от корня, а GitHub Pages публикует project site
  // под именем репозитория.
  base: command === 'build' ? '/cats-of-base/' : '/',
  // Порт можно занять снаружи (PORT): 5173 бывает занят соседним запуском, а
  // адрес превью должен совпадать с тем, что вправду слушает Vite.
  server: { port: Number(process.env.PORT) || 5173 },
  worker: { format: 'es' },
  // PixiJS поднимается через `await app.init(...)` на верхнем уровне модуля, и
  // это не случайность: инициализация асинхронна, а весь модуль ниже опирается
  // на готовый `app`. Целевой набор Vite по умолчанию (`modules` = es2020 и
  // браузеры 2020 года) top-level await не умеет, поэтому dev-сервер собирался,
  // а продакшн-сборка падала. es2022 — первый уровень, где он есть.
  build: { target: 'es2022' },
}));
