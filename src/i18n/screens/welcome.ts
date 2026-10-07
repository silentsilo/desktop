import type { Screen } from "../types";

/** The first screen. The emergency kit quotes these labels, so they are
 * shared rather than copied. */
export const welcome = {
  "welcome.already_have": {
    note: "Above two buttons on the first screen, when there is no silo yet. 'One': a silo. The emergency kit quotes this text, so it must match.",
    en: "Already have one?",
    ro: "Ai deja unul?",
    de: "Hast du schon eins?",
    fr: "Vous en avez déjà un ?",
    es: "¿Ya tienes uno?",
    it: "Ne hai già uno?",
    "pt-BR": "Já tem um?",
    pl: "Masz już silos?",
  },
  "welcome.add_folder": {
    note: "Button: adds an existing silo folder found on this computer or a drive. Quoted by the emergency kit.",
    en: "Add a folder from this computer",
    ro: "Adaugă un folder de pe acest calculator",
    de: "Ordner von diesem Computer hinzufügen",
    fr: "Ajouter un dossier de cet ordinateur",
    es: "Añadir una carpeta de este ordenador",
    it: "Aggiungi una cartella da questo computer",
    "pt-BR": "Adicionar uma pasta deste computador",
    pl: "Dodaj folder z tego komputera",
  },
  "welcome.join": {
    note: "Button and screen title: sets up a silo on this computer from its backup storage. Quoted by the emergency kit.",
    en: "Set up from backup storage",
    ro: "Configurează din stocarea pentru backup",
    de: "Aus Sicherungsspeicher einrichten",
    fr: "Configurer depuis le stockage de sauvegarde",
    es: "Configurar desde el almacenamiento de copias",
    it: "Configura dallo spazio di backup",
    "pt-BR": "Configurar a partir do armazenamento de backup",
    pl: "Skonfiguruj z magazynu kopii",
  },
} satisfies Screen;
