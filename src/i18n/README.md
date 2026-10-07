# Translations

`screens/` holds one file per screen. Each text sits there once, with a
note on where it appears and what it means, the English source and every
translation under it, so a reviewer reads a whole screen with the languages
side by side. A new screen file is listed in `screens/index.ts`. A language
missing on a text falls back to English; `i18n.test.ts` fails on a
`{placeholder}` that differs from English and on plural forms a language
needs but lacks.

Translate by meaning, with the screen in view, never word for word. The
mock (`npm run dev`, `?mock=unlocked`) shows every screen; the language is
picked under Settings > General.

## Glossary

One word per idea, the same everywhere. Where English uses one word for two
ideas, the note on each text says which.

| English | Meaning here | ro | de | fr | es | it | pt-BR | pl |
|---|---|---|---|---|---|---|---|---|
| silo | the user's encrypted vault; the product's own word, kept | siloz | Silo | silo | silo | silo | silo | silos |
| security key | the hardware key (YubiKey); es "llave" for anything that unlocks, "clave" for the encryption key | cheie de securitate | Sicherheitsschlüssel | clé de sécurité | llave de seguridad | chiave di sicurezza | chave de segurança | klucz bezpieczeństwa |
| key (encryption) | the secret that encrypts | cheie de criptare | Verschlüsselungsschlüssel | clé de chiffrement | clave de cifrado | chiave di cifratura | chave de criptografia | klucz szyfrowania |
| recovery code | the code on paper | cod de recuperare | Wiederherstellungscode | code de récupération | código de recuperación | codice di recupero | código de recuperação | kod odzyskiwania |
| copy (backup) | a place the silo is backed up to | copie | Kopie | copie | copia | copia | cópia | kopia |
| copy (verb) | put on the clipboard | copiază | kopieren | copier | copiar | copia | copiar | kopiuj |
| lock (verb) | close the silo | blochează | sperren | verrouiller | bloquear | blocca | bloquear | zablokuj |
| unlock | open the silo with a key | deblochează | entsperren | déverrouiller | desbloquear | sblocca | desbloquear | odblokuj |
| backup storage | where copies live | stocare pentru backup | Sicherungsspeicher | stockage de sauvegarde | almacenamiento de copias | spazio di backup | armazenamento de backup | magazyn kopii |
| never-delete copy | a copy nothing is ever removed from | copie fără ștergere | Kopie ohne Löschen | copie sans suppression | copia sin borrado | copia senza eliminazione | cópia sem exclusão | kopia bez usuwania |
| entry | one item under Passwords | intrare | Eintrag | entrée | entrada | voce | item | wpis |
| trash | deleted files, still restorable | coș | Papierkorb | corbeille | papelera | cestino | lixeira | kosz |
| working copy | the decrypted silo on this computer, not a backup copy | copie de lucru | Arbeitskopie | copie de travail | copia de trabajo | copia di lavoro | cópia de trabalho | kopia robocza |
| main copy | the backup copy others sync from | copia principală | Hauptkopie | copie principale | copia principal | copia principale | cópia principal | kopia główna |
| save a copy | export a decrypted file to disk | salvează o copie | Kopie speichern | enregistrer une copie | guardar una copia | salva una copia | salvar uma cópia | zapisz kopię |
| sync (noun) | | sincronizare | Synchronisierung | synchronisation | sincronización | sincronizzazione | sincronização | synchronizacja |
| change | one synced record | modificare | Änderung | modification | cambio | modifica | alteração | zmiana |
| file list | the silo's index of names | lista de fișiere | Dateiliste | liste des fichiers | lista de archivos | elenco dei file | lista de arquivos | lista plików |
| enrol | add a key to a silo | înregistrezi | registrieren | enregistrer | registrar | registrare | registrar | zarejestrować |
| emergency kit | the printed recovery sheet | kit de urgență | Notfallblatt | kit d’urgence | kit de emergencia | kit di emergenza | kit de emergência | zestaw awaryjny |
| login | a saved sign-in for a site; counted form in brackets | date de conectare (cont) | Zugangsdaten (Login) | identifiant | inicio de sesión | login | login | dane logowania (login) |
| passkey | | cheie de acces | Passkey | clé d’accès | llave de acceso | passkey | chave de acesso | klucz dostępu |
| passphrase | protects an SSH key | frază de acces | Passphrase | phrase de passe | frase de contraseña | passphrase | frase secreta | hasło klucza |
| activity log | | jurnal de activitate | Aktivitätsprotokoll | journal d’activité | registro de actividad | registro attività | registro de atividade | dziennik aktywności |

Settings sections, written the same wherever a text points to them
("Settings, Backup"). The rail in `screens/settings-rail.ts` is the source:

| English | ro | de | fr | es | it | pt-BR | pl |
|---|---|---|---|---|---|---|---|
| Settings | Setări | Einstellungen | Paramètres | Ajustes | Impostazioni | Configurações | Ustawienia |
| Overview | Privire de ansamblu | Übersicht | Vue d’ensemble | Resumen | Panoramica | Visão geral | Przegląd |
| Backup | Backup | Sicherung | Sauvegarde | Copia de seguridad | Backup | Backup | Kopia zapasowa |
| Recovery code | Cod de recuperare | Wiederherstellungscode | Code de récupération | Código de recuperación | Codice di recupero | Código de recuperação | Kod odzyskiwania |
| Test backup | Testează backupul | Sicherung prüfen | Tester la sauvegarde | Probar la copia | Verifica backup | Testar backup | Test kopii |
| Unlocking | Deblocare | Entsperren | Déverrouillage | Desbloqueo | Sblocco | Desbloqueio | Odblokowywanie |
| Devices | Dispozitive | Geräte | Appareils | Dispositivos | Dispositivi | Dispositivos | Urządzenia |
| Advanced | Avansat | Erweitert | Avancé | Avanzado | Avanzate | Avançado | Zaawansowane |
| General | General | Allgemein | Général | General | Generali | Geral | Ogólne |
| Updates and about | Actualizări și despre | Updates und Info | Mises à jour et à propos | Actualizaciones e información | Aggiornamenti e informazioni | Atualizações e sobre | Aktualizacje i informacje |

The app's name, SilentSilo, is never translated.

## Before a language loses "beta"

A native speaker reads at least the critical screens: the recovery code,
a lost key, deleting for good, the update card. Then its `reviewed` in
`locales.ts` becomes true.
