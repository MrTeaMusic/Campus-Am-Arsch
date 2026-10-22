# Auf blitz.cloud betreiben (kostenlos, ein Container)

Campus-API, Mensa-API und Frontend laufen in **einer** App. Das passt in den kostenlosen Tarif
von [blitz.cloud](https://blitz.cloud/de/pricing/) (5 Apps, 512 MB für alle Apps zusammen, kein
Kreditkarte nötig). Auf dem Handy öffnen die Nutzer die Adresse im Browser und wählen
„Zum Home-Bildschirm“.

> **Stand:** Das Dockerfile ist **noch nie auf blitz.cloud gebaut worden** (auf dem
> Entwicklungsrechner gibt es kein Docker). Der erste Bau kann scheitern. Das Build-Log zeigt dann,
> woran es liegt. Die Teile einzeln gemessen: Campus-API etwa 17 MB, Mensa-API etwa 30 MB,
> Frontend etwa 78 MB Arbeitsspeicher im Leerlauf, zusammen rund 125 MB.

## Pflichten als Betreiber

Du nimmst die Campus-Dual-Passwörter anderer Studierender entgegen. Sie liegen verschlüsselt im
Cookie des Nutzers, laufen aber über deine App.

- **Impressum:** Name, Anschrift, Telefon, E-Mail werden öffentlich angezeigt.
- **Datenschutz:** Die Seite bringt die Datenschutzerklärung des ursprünglichen Betreibers mit.
  Passe sie an deinen Betrieb an (`src/routes/datenschutz/+page.svelte` im Frontend).
- **Hochschule:** Kläre, ob automatisierte Logins bei Campus Dual / DHSN erlaubt sind.
- Das ist keine Rechtsberatung.

## Einrichten

1. Auf [dashboard.blitz.cloud](https://dashboard.blitz.cloud/) ein kostenloses Konto anlegen und
   eine Adresse wählen, zum Beispiel `meinname`. Damit werden deine Apps unter
   `<app>.meinname.blitz.cloud` erreichbar.
2. **Host something new → My own code.** Den Link einfügen:
   `https://github.com/MrTeaMusic/Campus-Unbloat`
3. Der App einen Namen geben, zum Beispiel `campus`. Die Adresse lautet dann
   `https://campus.meinname.blitz.cloud`.
4. Unter **Advanced settings** das Dockerfile **`deploy/blitz/Dockerfile`** auswählen. Das
   Dockerfile im Hauptverzeichnis baut nur die Campus-API und ist hier das falsche.
5. **Einstellungen (Environment)** eintragen, bevor oder gleich nachdem du „Build it“ klickst:

   | Name | Wert |
   |---|---|
   | `JWT_SECRET` | lange Zufallszeichenkette, zum Beispiel `openssl rand -hex 32` |
   | `AES_KEY` | genau 32 Zeichen, zum Beispiel `openssl rand -hex 16` |
   | `ORIGIN` | `https://campus.meinname.blitz.cloud` (deine echte Adresse) |
   | `PUBLIC_IMPRESS_FULLNAME` | Vorname Nachname |
   | `PUBLIC_IMPRESS_STRASSE_HAUSNR` | Straße und Hausnummer |
   | `PUBLIC_IMPRESS_PLZ_ORT` | Postleitzahl und Ort |
   | `PUBLIC_IMPRESS_TELEFON` | Telefonnummer |
   | `PUBLIC_IMPRESS_EMAIL` | E-Mail-Adresse |

   `JWT_SECRET` und `AES_KEY` nie ändern oder weitergeben. Ändern sie sich, sind alle Sitzungen
   ungültig, und wer sie kennt, kann gespeicherte Anmeldedaten entschlüsseln.
6. Bauen lassen und im Build-Log zuschauen. Der erste Bau dauert mehrere Minuten, vor allem die
   beiden Rust-Programme.

Das Frontend lauscht im Container auf Port 3000 (`EXPOSE 3000`), die Plattform liest ihn von dort.
Die Campus-API (8080) und die Mensa-API (9090) sind nur intern erreichbar.

## Aktualisieren

Bei jedem Push auf `main` des Backend-Repos baut blitz.cloud neu. Das Frontend wird dabei in dem
Stand gebaut, der in `deploy/blitz/frontend-ref.txt` steht (ein Commit-Hash des Frontend-Repos). Nach
einer Änderung im Frontend den neuen Hash dort eintragen (`git rev-parse origin/main` im
Frontend-Repo) und im Backend-Repo pushen. Das löst den Bau aus und zwingt den Docker-Cache, das
Frontend neu zu holen. Die Mensa-API wird weiter in ihrem jeweils neuesten Stand geholt.

## Grenzen des kostenlosen Tarifs

- **Schlafmodus:** Die App schläft nach 30 Minuten ohne Besuch ein. Der erste Aufruf danach braucht
  einen Moment. Soll sie immer wach sein, kostet eine einzelne Pro-App 2 € im Monat.
- Auf den Seiten steht ein kleiner Link „Made with blitz.cloud“, und Suchmaschinen sollen sie
  nicht aufnehmen.
- Ein Bau nach dem anderen, Pushes kurz hintereinander werden zusammengefasst.
- Die Plattform ist jung. Prüfe ihre aktuellen Bedingungen.

## Gemeinsamer Kalender (Speicher)

Die Einträge liegen in der SQLite-Datei `shared_calendar.db` im Arbeitsverzeichnis `/app/data`
(anderer Ort: Umgebungsvariable `SHARED_DB_PATH`). Damit sie ein Update überleben, muss der Ordner
`/app/data` unter Einstellungen → Dateien als zu behaltender Ordner eingetragen sein. Im
kostenlosen Tarif gibt es dafür kein automatisches Backup.

## Wenn etwas nicht klappt

- **Build scheitert:** Das Build-Log in der Übersicht der App oder im Tab Activity lesen.
- **App startet nicht:** Meist fehlt eine Einstellung. `start.sh` bricht mit einer Meldung ab, wenn
  `JWT_SECRET` oder `AES_KEY` fehlen, und das Frontend beendet sich im Produktionsmodus, wenn die
  `PUBLIC_IMPRESS_*`-Werte fehlen.
- **Login-Seite meldet „Cross-site POST form submissions are forbidden“:** `ORIGIN` fehlt oder
  stimmt nicht mit der Adresse überein.
- **„CampusDual ist gerade nicht erreichbar oder hat sich geändert“:** Campus Dual selbst hat sich
  geändert oder ist nicht erreichbar (HTTP 502).
