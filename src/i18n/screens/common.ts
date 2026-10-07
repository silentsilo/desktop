import type { Screen } from "../types";

/** Words several screens share. */
export const common = {
  "common.cancel": {
    note: "Button that closes a dialog without doing anything.",
    en: "Cancel",
    ro: "Anulează",
    de: "Abbrechen",
    fr: "Annuler",
    es: "Cancelar",
    it: "Annulla",
    "pt-BR": "Cancelar",
    pl: "Anuluj",
  },
  "common.confirm": {
    note: "Default button of a confirmation dialog, used when the dialog names no action.",
    en: "Confirm",
    ro: "Confirmă",
    de: "Bestätigen",
    fr: "Confirmer",
    es: "Confirmar",
    it: "Conferma",
    "pt-BR": "Confirmar",
    pl: "Potwierdź",
  },
  "common.are_you_sure": {
    note: "Default title of a confirmation dialog.",
    en: "Are you sure?",
    ro: "Ești sigur?",
    de: "Bist du sicher?",
    fr: "Êtes-vous sûr ?",
    es: "¿Seguro?",
    it: "Sei sicuro?",
    "pt-BR": "Tem certeza?",
    pl: "Czy na pewno?",
  },
} satisfies Screen;
