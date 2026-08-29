# Публикация `cats-of-base` на GitHub Pages

В `cats-of-base` можно повторить схему из [`chains-of-purr-future`](https://github.com/Islands-of-Eternal-Cats/chains-of-purr-future): собирать проект при каждом push в `main`, складывать результат в ветку `gh-pages`, а GitHub Pages публиковать из этой ветки.

В отличие от обычного Vite-проекта, здесь перед веб-сборкой необходимо скомпилировать Rust в WASM.

## 1. Добавить базовый путь Vite

Обновите `web/vite.config.js`:

```js
import { defineConfig } from "vite";

export default defineConfig(({ command }) => ({
  root: ".",
  publicDir: "../assets",

  // Локально: /
  // GitHub Pages: /cats-of-base/
  base: command === "build" ? "/cats-of-base/" : "/",

  server: { port: Number(process.env.PORT) || 5173 },
  worker: { format: "es" },
  build: { target: "es2022" },
}));
```

## 2. Исправить абсолютные адреса ресурсов

В `web/src/main.js` портреты сейчас загружаются от корня домена:

```js
src="/portraits/..."
```

Замените этот адрес на:

```js
src="${import.meta.env.BASE_URL}portraits/${encodeURIComponent(sprite)}.png"
```

В `web/src/worker.js` замените:

```js
fetch("/rulesets/core.yaml")
```

на:

```js
fetch(`${import.meta.env.BASE_URL}rulesets/core.yaml`)
```

Без этих изменений браузер будет искать файлы по адресу:

```text
https://islands-of-eternal-cats.github.io/portraits/...
```

вместо правильного:

```text
https://islands-of-eternal-cats.github.io/cats-of-base/portraits/...
```

## 3. Добавить GitHub Actions workflow

Создайте `.github/workflows/deploy.yml`:

```yaml
name: Deploy to GitHub Pages

on:
  push:
    branches:
      - main
  workflow_dispatch:

permissions:
  contents: write

jobs:
  build-and-deploy:
    runs-on: ubuntu-latest

    steps:
      - name: Checkout
        uses: actions/checkout@v4

      - name: Set up Rust
        uses: dtolnay/rust-toolchain@stable
        with:
          targets: wasm32-unknown-unknown

      - name: Install wasm-pack
        run: cargo install wasm-pack --locked

      - name: Build WASM
        run: wasm-pack build --target web --out-dir web/src/wasm

      - name: Set up Node.js
        uses: actions/setup-node@v4
        with:
          node-version: 22
          cache: npm
          cache-dependency-path: web/package-lock.json

      - name: Install web dependencies
        run: npm ci
        working-directory: web

      - name: Build website
        run: npm run build
        working-directory: web

      - name: Deploy to GitHub Pages
        uses: JamesIves/github-pages-deploy-action@v4
        with:
          branch: gh-pages
          folder: web/dist
```

Это почти тот же workflow, что используется в `chains-of-purr-future`, но с дополнительными шагами сборки Rust/WASM и путями внутри каталога `web`.

## 4. Включить GitHub Pages

После отправки изменений в `main`:

1. Откройте вкладку **Actions** и дождитесь успешного выполнения workflow `Deploy to GitHub Pages`.
2. Workflow создаст ветку `gh-pages`.
3. Откройте **Settings → Pages**.
4. В поле **Source** выберите **Deploy from a branch**.
5. В качестве ветки выберите `gh-pages`.
6. В качестве папки выберите `/(root)`.
7. Нажмите **Save**.

Итоговый адрес сайта:

<https://islands-of-eternal-cats.github.io/cats-of-base/>

## Возможная проблема с правами

Если Action не сможет создать или обновить ветку `gh-pages`, откройте:

**Settings → Actions → General → Workflow permissions**

и проверьте, что workflow разрешена запись в репозиторий. В самом `deploy.yml` уже указано минимально необходимое разрешение:

```yaml
permissions:
  contents: write
```

## Справка

- [Настройка источника публикации GitHub Pages](https://docs.github.com/en/pages/getting-started-with-github-pages/configuring-a-publishing-source-for-your-github-pages-site)
- [`cats-of-base`](https://github.com/Islands-of-Eternal-Cats/cats-of-base)
- [`chains-of-purr-future`](https://github.com/Islands-of-Eternal-Cats/chains-of-purr-future)
