# Alles in einem Container für blitz.cloud (eine App, ein Slot im kostenlosen Tarif):
#   Campus-API (Rust, Port 8080), Mensa-API (Rust, Port 9090), Frontend (Node, Port 3000)
# Das Frontend und die Mensa-API werden beim Bauen aus GitHub geholt, der Campus-API-Code
# kommt aus diesem Repository. Bau-Kontext ist das Wurzelverzeichnis dieses Repos.

FROM rust:bookworm AS build-api
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release

FROM rust:bookworm AS build-mensa
# schlägt den Docker-Cache nieder, sobald sich der Branch ändert
ADD https://api.github.com/repos/fnschmidt/mensa-api/git/refs/heads/main /tmp/mensa-ref.json
RUN git clone --depth 1 https://github.com/fnschmidt/mensa-api.git /src
WORKDIR /src
RUN cargo build --release

FROM node:22-bookworm-slim AS build-frontend
RUN apt-get update && apt-get install -y --no-install-recommends git ca-certificates \
  && rm -rf /var/lib/apt/lists/*
# Der Frontend-Stand steht in frontend-ref.txt. Ändert sich die Datei, baut der Docker-Cache dieses
# Stufe neu, auch wenn der Builder die ADD-Prüfung auf Remote-URLs nicht wiederholt.
COPY deploy/blitz/frontend-ref.txt /tmp/frontend-ref
RUN git clone https://github.com/MrTeaMusic/Campus-Am-Arsch-Frontend.git /app \
  && git -C /app checkout "$(cat /tmp/frontend-ref)"
WORKDIR /app
RUN npm ci && npm run build && npm prune --omit-dev

FROM node:22-bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends libsqlite3-0 ca-certificates \
  && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=build-api /src/target/release/campus-api /app/campus-api
COPY --from=build-mensa /src/target/release/mensa-api /app/mensa-api
COPY --from=build-frontend /app/build /app/frontend/build
COPY --from=build-frontend /app/node_modules /app/frontend/node_modules
COPY --from=build-frontend /app/package.json /app/frontend/package.json
COPY deploy/blitz/start.sh /app/start.sh
RUN chmod +x /app/start.sh && mkdir -p /app/data

ENV NODE_ENV=production
# blitz.cloud liest den Port der App aus EXPOSE (die Plattform setzt PORT=8080, das braucht
# aber die Campus-API intern)
EXPOSE 3000
WORKDIR /app/data
CMD ["/app/start.sh"]
