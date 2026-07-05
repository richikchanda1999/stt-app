// Sarvam-supported languages and models.

export const LANGUAGES: { code: string; label: string }[] = [
  { code: "unknown", label: "Auto-detect" },
  { code: "bn-IN", label: "Bengali" },
  { code: "en-IN", label: "English (India)" },
  { code: "hi-IN", label: "Hindi" },
  { code: "kn-IN", label: "Kannada" },
  { code: "ml-IN", label: "Malayalam" },
  { code: "mr-IN", label: "Marathi" },
  { code: "od-IN", label: "Odia" },
  { code: "pa-IN", label: "Punjabi" },
  { code: "ta-IN", label: "Tamil" },
  { code: "te-IN", label: "Telugu" },
  { code: "gu-IN", label: "Gujarati" },
  { code: "as-IN", label: "Assamese" },
  { code: "ur-IN", label: "Urdu" },
];

export function langLabel(code: string): string {
  return LANGUAGES.find((l) => l.code === code)?.label ?? code;
}

export const MODELS = [
  { value: "saaras:v3", label: "saaras:v3 (latest, supports modes)" },
  { value: "saarika:v2.5", label: "saarika:v2.5" },
];

export const MODES = [
  { value: "transcribe", label: "Transcribe (original language)" },
  { value: "translate", label: "Translate → English" },
  { value: "verbatim", label: "Verbatim (fillers/repetitions)" },
  { value: "translit", label: "Transliterate (Latin script)" },
  { value: "codemix", label: "Code-mix (mixed languages)" },
];

// Per-file dropdown adds an "inherit batch default" sentinel.
export const INHERIT = "__inherit__";
