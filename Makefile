COMPOSE = docker compose
SERVICE ?= proxy

.PHONY: dev-build dev-run dev-kill dev-shell

.env:
	cp .env.example .env

dev-build: .env
	$(COMPOSE) build

dev-run: .env
	$(COMPOSE) up -d --build
	@IP=$$(hostname -I | awk '{print $$1}'); \
	echo "Proxy : http://$$IP:$$(grep -m1 '^PROXY_PORT=' .env | cut -d= -f2)"; \
	echo "Admin : http://$$IP:$$(grep -m1 '^ADMIN_PORT=' .env | cut -d= -f2)"

dev-kill:
	$(COMPOSE) down

dev-shell: .env
	$(COMPOSE) exec $(SERVICE) sh
