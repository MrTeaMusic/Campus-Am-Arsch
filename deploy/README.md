# Selbst betreiben (Oracle Cloud „Always Free“)

> **Einfacher und ohne Server:** [blitz.cloud](blitz/README.md) (kostenlos, ein Container).
> Diese Anleitung hier beschreibt den Weg über einen eigenen Server bei Oracle.

Diese Anleitung bringt CampusUnbloat (Frontend, Campus-API, Mensa-API) auf einen kostenlosen
Oracle-Server. Alles wird dort aus GitHub gebaut, es braucht keine Container-Registry und keine
Zugangsdaten außer deinen eigenen. Danach öffnen Android- und iOS-Nutzer die Adresse im Browser
und wählen „Zum Home-Bildschirm“, dann verhält sich die Seite wie eine App.

> **Stand:** Die Dateien in diesem Ordner sind **nicht** auf einem echten Server getestet. Die
> Menünamen von Oracle ändern sich öfter, die Schritte unten sind die Richtung, nicht Wort für Wort.

## Bevor du anfängst: Pflichten als Betreiber

Du nimmst die Campus-Dual-Passwörter anderer Studierender entgegen. Sie liegen verschlüsselt im
Cookie des Nutzers, laufen aber über deinen Server.

- **Impressum:** Name, Anschrift, Telefon und E-Mail werden öffentlich angezeigt (Datei `.env`).
- **Datenschutz:** Die Seite bringt eine Datenschutzerklärung des ursprünglichen Betreibers mit.
  Lies sie und passe sie an deinen Betrieb an (Frontend: `src/routes/datenschutz/+page.svelte`).
- **Hochschule:** Kläre, ob automatisierte Logins bei Campus Dual / DHSN erlaubt sind.
- Das ist keine Rechtsberatung.

## 1. Oracle-Konto und Server

1. Konto auf cloud.oracle.com anlegen. Dafür verlangt Oracle meines Wissens eine Kreditkarte zur
   Prüfung. Wähle die „Always Free“-Ressourcen.
2. Eine Instanz erstellen: Image **Ubuntu 24.04** (Architektur **aarch64/ARM**), Shape
   **VM.Standard.A1.Flex** mit höchstens **2 OCPU und 12 GB RAM** (das kostenlose Kontingent wurde
   2026 gekürzt, bleib darunter). Gibt es „Out of capacity“, später oder in einer anderen
   Verfügbarkeitsdomäne erneut versuchen.
3. Den SSH-Schlüssel herunterladen, den Oracle anbietet, und die öffentliche IP-Adresse notieren.
4. **Ports freigeben:** Im Oracle-Netzwerk (VCN, Security List der Subnetz-Einstellungen) eingehende
   Regeln für TCP **80** und **443** von `0.0.0.0/0` ergänzen.
5. Per SSH verbinden: `ssh -i <schluessel> ubuntu@<IP>`. Dort die Ports auch in der Firewall des
   Servers öffnen. Die Oracle-Ubuntu-Images blockieren sie standardmäßig:

   ```bash
   sudo iptables -I INPUT 6 -m state --state NEW -p tcp --dport 80 -j ACCEPT
   sudo iptables -I INPUT 6 -m state --state NEW -p tcp --dport 443 -j ACCEPT
   sudo apt-get update && sudo apt-get install -y iptables-persistent
   sudo netfilter-persistent save
   ```

## 2. Kostenlose Adresse (Domain)

Für HTTPS braucht Caddy einen Namen. Eine kostenlose Möglichkeit ist DuckDNS:

1. Auf duckdns.org anmelden und eine Subdomain anlegen, zum Beispiel `meine-mensa.duckdns.org`.
2. Als IP die öffentliche IP deines Oracle-Servers eintragen.

Alternativ geht jede eigene Domain, deren A-Eintrag auf die Server-IP zeigt.

## 3. Docker installieren (auf dem Server)

```bash
curl -fsSL https://get.docker.com | sudo sh
sudo usermod -aG docker $USER
```

Danach einmal ab- und wieder anmelden.

## 4. Starten

```bash
git clone https://github.com/MrTeaMusic/Campus-Unbloat.git
cd Campus-Unbloat/deploy
./setup.sh          # erzeugt .env mit zufälligen Schlüsseln
nano .env           # DOMAIN und die IMPRESS_*-Angaben ausfüllen
docker compose up -d --build
```

Der erste Bau dauert mehrere Minuten, vor allem das Rust-Backend. Danach läuft alles. Prüfen:

```bash
docker compose ps
docker compose logs -f api-campus
```

Öffne dann `https://<deine-domain>`. Das Zertifikat holt sich Caddy beim ersten Aufruf selbst.

## Aktualisieren

```bash
cd Campus-Unbloat/deploy
docker compose up -d --build --pull always
```

Die Dienste werden dabei neu aus GitHub gebaut. Die Schlüssel in `.env` bleiben, Sitzungen
bleiben gültig.

## Hinweise

- `JWT_SECRET` und `AES_KEY` in `.env` nie ändern oder weitergeben. Ändern sie sich, sind alle
  Sitzungen ungültig, und wer sie kennt, kann Anmeldedaten entschlüsseln.
- Oracle kann ungenutzte kostenlose Instanzen nach meiner Kenntnis stilllegen, wenn sie dauerhaft
  kaum ausgelastet sind. Prüfe die aktuellen Bedingungen von Oracle.
- Die Leipziger Mensen holt das Frontend direkt von OpenMensa. Der Mensa-Dienst ist nur für die
  Mensaria am Botanischen Garten und als Rückfall nötig.
- Geht Campus Dual wieder kaputt, zeigt der Login „CampusDual ist gerade nicht erreichbar oder hat
  sich geändert“ (HTTP 502).
