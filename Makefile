# SP / Эсперы — задачи разработки.
# Требуется: rust + wasm-pack, node/npm. Первый раз: `make setup`.

WASM_OUT := web/src/wasm
PORT     := 5173

.PHONY: dev build wasm run lint web-install setup clean help

## dev: собрать WASM и запустить dev-сервер (http://localhost:5173)
dev: wasm web-install
	npm --prefix web run dev

## build: продакшн-сборка (WASM + статика в web/dist)
build: wasm web-install
	npm --prefix web run build

## wasm: собрать ядро симуляции (Rust -> WASM) в web/src/wasm
wasm:
	wasm-pack build --target web --out-dir $(WASM_OUT)

## run: запустить dev-сервер без пересборки WASM
run: web-install
	npm --prefix web run dev

## lint: clippy по всему крейту, любое замечание — ошибка
# --all-targets обязателен: без него тесты не линтуются вовсе, а `src/tests/`
# это треть кода — там уже жило одно из замечаний, схлопнутых в 99cfb3a.
lint:
	cargo clippy --all-targets -- -D warnings

## setup: одноразовая подготовка тулчейна (wasm-таргет + веб-зависимости)
setup:
	rustup target add wasm32-unknown-unknown
	rustup component add clippy
	npm --prefix web install

## clean: удалить сборочные артефакты
clean:
	rm -rf target $(WASM_OUT) web/node_modules web/dist

# Ставим веб-зависимости, только если их ещё нет или изменился package.json.
web-install: web/node_modules
web/node_modules: web/package.json
	npm --prefix web install
	@touch web/node_modules

## help: показать список целей
help:
	@grep -E '^## ' $(MAKEFILE_LIST) | sed 's/^## //'

.DEFAULT_GOAL := help
