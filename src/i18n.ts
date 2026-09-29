export type Locale = "ja" | "en";

export interface Translations {
  // HUD
  inputPlaceholder: string;
  closeTooltip: string;
  autoSpeakOn: string;
  autoSpeakOff: string;
  sendTooltip: string;
  micTooltip: string;
  dashboardTooltip: string;
  projectSelectorTitle: string;
  noProjects: string;
  newProjectButton: string;
  newProjectTitle: string;
  newProjectPlaceholder: string;
  createButton: string;
  // Quick chips
  chipYoutube: string;
  chipYoutubeCmd: string;
  chipGoogle: string;
  chipGoogleCmd: string;
  chipNotepad: string;
  chipNotepadCmd: string;
  chipWindow: string;
  chipWindowCmd: string;
  chipAgent: string;
  chipAgentCmd: string;
  // Dashboard Tabs
  tabGoals: string;
  tabTools: string;
  tabVerification: string;
  tabSettings: string;
  // Common
  languageLabel: string;
}

export const translations: Record<Locale, Translations> = {
  ja: {
    inputPlaceholder: "Veyra に何でも指示してください… (例: メモ帳を開いて, Agent進捗確認)",
    closeTooltip: "閉じる (Esc)",
    autoSpeakOn: "自動読み上げ: オン",
    autoSpeakOff: "自動読み上げ: オフ",
    sendTooltip: "送信 (Enter)",
    micTooltip: "音声入力 (長押し: Push-to-Talk)",
    dashboardTooltip: "プロジェクトダッシュボード (Ctrl+D)",
    projectSelectorTitle: "プロジェクト選択",
    noProjects: "プロジェクトなし",
    newProjectButton: "+ 新規",
    newProjectTitle: "新規プロジェクト作成",
    newProjectPlaceholder: "新規プロジェクト名を入力…",
    createButton: "作成",
    chipYoutube: "YouTube",
    chipYoutubeCmd: "YouTubeを開いて",
    chipGoogle: "Google検索",
    chipGoogleCmd: "Googleで最新のAIニュースを検索して",
    chipNotepad: "メモ帳",
    chipNotepadCmd: "メモ帳を開いて (notepad)",
    chipWindow: "ウィンドウ",
    chipWindowCmd: "アクティブなウィンドウを調べて",
    chipAgent: "Agent確認",
    chipAgentCmd: "Agentがゴールに向けて動いているか確認して",
    tabGoals: "🎯 Goals & Agent",
    tabTools: "⚡ Tools & Automation",
    tabVerification: "📋 Verification & Decisions",
    tabSettings: "⚙️ Voice & Settings",
    languageLabel: "表示言語 (Language)",
  },
  en: {
    inputPlaceholder: "Ask Veyra anything… (e.g. Open Notepad, Check agent progress)",
    closeTooltip: "Close (Esc)",
    autoSpeakOn: "Auto-Speak: ON",
    autoSpeakOff: "Auto-Speak: OFF",
    sendTooltip: "Send (Enter)",
    micTooltip: "Voice Input (Hold: Push-to-Talk)",
    dashboardTooltip: "Project Dashboard (Ctrl+D)",
    projectSelectorTitle: "Select Project",
    noProjects: "No Projects",
    newProjectButton: "+ New",
    newProjectTitle: "Create New Project",
    newProjectPlaceholder: "Enter new project name…",
    createButton: "Create",
    chipYoutube: "YouTube",
    chipYoutubeCmd: "Open YouTube in browser",
    chipGoogle: "Search Google",
    chipGoogleCmd: "Search Google for latest AI news",
    chipNotepad: "Notepad",
    chipNotepadCmd: "Open Notepad",
    chipWindow: "Active Window",
    chipWindowCmd: "Check current active window",
    chipAgent: "Agent Progress",
    chipAgentCmd: "Check autonomous agent progress toward active goal",
    tabGoals: "🎯 Goals & Agent",
    tabTools: "⚡ Tools & Automation",
    tabVerification: "📋 Verification & Decisions",
    tabSettings: "⚙️ Voice & Settings",
    languageLabel: "Language",
  },
};

const LANG_KEY = "veyra_ui_locale";

export function getInitialLocale(): Locale {
  try {
    const saved = localStorage.getItem(LANG_KEY);
    if (saved === "ja" || saved === "en") return saved;
    return typeof navigator !== "undefined" && navigator.language.startsWith("ja") ? "ja" : "en";
  } catch {
    return "ja";
  }
}

export function saveLocale(loc: Locale): void {
  try {
    localStorage.setItem(LANG_KEY, loc);
  } catch {}
}
