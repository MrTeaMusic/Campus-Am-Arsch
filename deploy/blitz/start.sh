#!/bin/bash
# Startet die drei Prozesse. Endet einer, wird der ganze Container beendet, damit die Plattform
# ihn neu startet.
set -u

# Die Plattform setzt PORT=8080 für die App, das gehört aber der Campus-API. Das Frontend
# nimmt Port 3000 (siehe EXPOSE im Dockerfile).
unset PORT

: "${JWT_SECRET:?JWT_SECRET ist nicht gesetzt}"
: "${AES_KEY:?AES_KEY ist nicht gesetzt}"

# Hinter dem Proxy braucht SvelteKit die öffentliche Adresse (ORIGIN=https://...), sonst wird der
# Login als Cross-Site-Anfrage abgelehnt. Ohne ORIGIN nehmen wir die Header des Proxys.
if [ -z "${ORIGIN:-}" ]; then
  export PROTOCOL_HEADER=x-forwarded-proto
  export HOST_HEADER=x-forwarded-host
fi

# Das kostenlose Kontingent hat 512 MB für alle Apps zusammen
export NODE_OPTIONS="${NODE_OPTIONS:---max-old-space-size=160}"

export RUST_LOG="${RUST_LOG:-info}"

/app/campus-api &
api_pid=$!
/app/mensa-api &
mensa_pid=$!
(
  cd /app/frontend || exit 1
  export PORT=3000 HOST=0.0.0.0
  export CD_API_URL=http://127.0.0.1:8080
  export MENSA_API_URL=http://127.0.0.1:9090
  exec node build
) &
frontend_pid=$!

# Endet einer der Prozesse, wird alles beendet (Schleife statt `wait -n`, damit sie überall läuft)
while true; do
  for pid in "$api_pid" "$mensa_pid" "$frontend_pid"; do
    if ! kill -0 "$pid" 2>/dev/null; then
      echo "Prozess $pid wurde beendet, stoppe den Container." >&2
      kill 0
      exit 1
    fi
  done
  sleep 2
done
