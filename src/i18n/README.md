# Translations

`en.ts` is the source: every text with a note on where it appears and what
it means. `translations.ts` holds the other seven languages. A key missing
there falls back to English; `i18n.test.ts` fails on a key English does not
have, on a `{placeholder}` that differs from English, and on plural forms a
language needs but lacks.

Translate by meaning, with the screen in view, never word for word. The
mock (`npm run dev`, `?mock=unlocked`) shows every screen; the language is
picked under Settings > General.

## Glossary

One word per idea, the same everywhere. Where English uses one word for two
ideas, the note on each text says which.

| English | Meaning here | ro | de | fr | es | it | pt-BR | pl |
|---|---|---|---|---|---|---|---|---|
| silo | the user's encrypted vault; the product's own word, kept | siloz | Silo | silo | silo | silo | silo | silos |
| security key | the hardware key (YubiKey) | cheie de securitate | Sicherheitsschlüssel | clé de sécurité | llave de seguridad | chiave di sicurezza | chave de segurança | klucz bezpieczeństwa |
| key (encryption) | the secret that encrypts | cheie de criptare | Schlüssel | clé de chiffrement | clave de cifrado | chiave di cifratura | chave de criptografia | klucz szyfrowania |
| recovery code | the code on paper | cod de recuperare | Wiederherstellungscode | code de récupération | código de recuperación | codice di recupero | código de recuperação | kod odzyskiwania |
| copy (backup) | a place the silo is backed up to | copie | Kopie | copie | copia | copia | cópia | kopia |
| copy (verb) | put on the clipboard | copiază | kopieren | copier | copiar | copia | copiar | kopiuj |
| lock (verb) | close the silo | blochează | sperren | verrouiller | bloquear | blocca | bloquear | zablokuj |
| unlock | open the silo with a key | deblochează | entsperren | déverrouiller | desbloquear | sblocca | desbloquear | odblokuj |
| backup storage | where copies live | stocare pentru backup | Sicherungsspeicher | stockage de sauvegarde | almacenamiento de copias | spazio di backup | armazenamento de backup | magazyn kopii |
| never-delete copy | a copy nothing is ever removed from | copie fără ștergere | Kopie ohne Löschen | copie sans suppression | copia sin borrado | copia senza eliminazione | cópia sem exclusão | kopia bez usuwania |
| entry | one item under Passwords | intrare | Eintrag | entrée | entrada | voce | item | wpis |
| trash | deleted files, still restorable | coș | Papierkorb | corbeille | papelera | cestino | lixeira | kosz |

The app's name, SilentSilo, is never translated.

## Before a language loses "beta"

A native speaker reads at least the critical screens: the recovery code,
a lost key, deleting for good, the update card. Then its `reviewed` in
`locales.ts` becomes true.
