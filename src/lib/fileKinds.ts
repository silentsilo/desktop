import {
  File,
  FileArchive,
  FileAudio,
  FileCode,
  FileImage,
  FileSpreadsheet,
  FileText,
  FileType,
  FileVideo,
  Presentation,
} from "lucide-react";

/**
 * A file's type, from its extension.
 *
 * Drawn from the app's own icon set rather than read from the Windows shell.
 * Shell icons are full-colour raster art whose style is set by whatever is
 * installed on that machine; dropped into a UI built entirely from 1.6-stroke
 * line icons they read as pasted in, they differ between two people looking
 * at the same silo, and they would need per-platform code to exist at all.
 * The tradeoff is that an obscure extension gets the generic mark instead of
 * its application's logo, which costs nothing a filename does not already say.
 */
export type FileKind =
  | "text"
  | "pdf"
  | "image"
  | "audio"
  | "video"
  | "archive"
  | "code"
  | "sheet"
  | "slides"
  | "generic";

const BY_EXTENSION: Record<string, FileKind> = {};

function register(kind: FileKind, extensions: string) {
  for (const ext of extensions.split(" ")) BY_EXTENSION[ext] = kind;
}

register("pdf", "pdf");
register("text", "txt md rtf log rst adoc tex doc docx odt pages");
register("image", "png jpg jpeg gif webp avif bmp tif tiff svg ico heic raw psd");
register("audio", "mp3 wav flac aac ogg oga m4a opus wma aiff mid");
register("video", "mp4 mkv mov avi webm wmv flv m4v mpg mpeg");
register("archive", "zip 7z rar tar gz bz2 xz zst iso dmg cab tgz");
register("sheet", "csv tsv xls xlsx ods numbers");
register("slides", "ppt pptx odp key");
register(
  "code",
  "js ts jsx tsx json yaml yml toml xml html css scss rs go py rb java c h cpp hpp cs php sh ps1 sql ini conf env lock",
);

export function fileKindOf(name: string): FileKind {
  const dot = name.lastIndexOf(".");
  // A leading dot is a bare dotfile (".gitignore"), not an extension.
  if (dot <= 0 || dot === name.length - 1) return "generic";
  return BY_EXTENSION[name.slice(dot + 1).toLowerCase()] ?? "generic";
}

const ICONS: Record<FileKind, typeof File> = {
  text: FileText,
  pdf: FileType,
  image: FileImage,
  audio: FileAudio,
  video: FileVideo,
  archive: FileArchive,
  code: FileCode,
  sheet: FileSpreadsheet,
  slides: Presentation,
  generic: File,
};

export function fileIconFor(name: string) {
  return ICONS[fileKindOf(name)];
}
