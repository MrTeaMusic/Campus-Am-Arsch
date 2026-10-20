#!/usr/bin/env bash
# Legt .env an und erzeugt die geheimen Schlüssel. Mehrfaches Ausführen überschreibt nichts.
set -euo pipefail
cd "$(dirname "$0")"

if [ -f .env ]; then
  echo ".env existiert bereits, nichts geändert."
  exit 0
fi

cp .env.example .env
jwt=$(openssl rand -hex 32)
aes=$(openssl rand -hex 16) # 32 Zeichen = 32 Bytes
sed -i.bak "s|^JWT_SECRET=.*|JWT_SECRET=${jwt}|; s|^AES_KEY=.*|AES_KEY=${aes}|" .env
rm -f .env.bak
chmod 600 .env
echo ".env angelegt. Jetzt DOMAIN und die IMPRESS_*-Angaben darin ausfüllen: nano .env"
