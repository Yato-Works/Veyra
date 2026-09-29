import { invoke } from "@tauri-apps/api/core";

// --- Types mirroring the Rust authority layer (veyra-core / veyra-store) ---

export interface Project {
  id: string;
  name: string;
  root_path: string | null;
  created_at: string;
  updated_at: string;
}

export interface Requirement {
  id: string;
  goal_id: string;
  content: string;
  status: string;
}

export interface AcceptanceCriterion {
  id: string;
  goal_id: string;
  content: string;
  status: string;
}

export interface Goal {
  id: string;
  project_id: string;
  objective: string;
  requirements: Requirement[];
  constraints: { content: string }[];
  preferences: { content: string }[];
  acceptance_criteria: AcceptanceCriterion[];
  status: string;
  created_at: string;
  updated_at: string;
}

export interface Decision {
  id: string;
  project_id: string;
  decision: string;
  rationale: string | null;
  created_at: string;
}

export interface ChatMessage {
  role: string;
  content: string;
  created_at: string;
}

export interface Snapshot {
  goals: Goal[];
  decisions: Decision[];
  user_feedback: string[];
  last_verification: {
    id: string;
    passed: boolean;
    summary: string;
    verified_at: string;
  } | null;
  messages: ChatMessage[];
}

export interface LoopOutcome {
  intent: string;
  intent_method: string;
  response: string;
  goal_id: string | null;
}

export interface CheckOutcome {
  name: string;
  passed: boolean;
  skipped: boolean;
  detail: string;
}

export interface VerificationOutcome {
  passed: boolean;
  summary: string;
  checks: CheckOutcome[];
}

export interface StepReport {
  description: string;
  kind: string;
  success: boolean;
  detail: string;
}

export interface AgentReport {
  goal_id: string;
  completed: boolean;
  replans: number;
  steps: StepReport[];
  verification: VerificationOutcome | null;
}

export interface ToolManifest {
  name: string;
  description: string;
  input_schema: unknown;
  output_schema: unknown;
  permission: string;
  risk: string;
}

export interface AssistantStep {
  tool_name: string | null;
  input: unknown;
  output: unknown | null;
  success: boolean;
  detail: string;
}

export interface AssistantReport {
  instruction: string;
  response: string;
  steps: AssistantStep[];
  success: boolean;
}

export interface TranscriptPayload {
  raw: string;
  cleaned: string;
}

export interface VoiceSettings {
  sbv2Url: string;
  ttsModel: string;
  whisperModel: string;
  autoSpeak: boolean;
}

export const DEFAULT_VOICE_SETTINGS: VoiceSettings = {
  sbv2Url: "http://127.0.0.1:5000",
  ttsModel: "jvnv-M1-jp",
  whisperModel: "tiny",
  autoSpeak: true,
};

export function loadVoiceSettings(): VoiceSettings {
  try {
    const raw = localStorage.getItem("veyra-voice-settings");
    if (raw) {
      const parsed = JSON.parse(raw);
      return { ...DEFAULT_VOICE_SETTINGS, ...parsed };
    }
  } catch {
    // ignore malformed settings
  }
  return DEFAULT_VOICE_SETTINGS;
}

export function saveVoiceSettings(s: VoiceSettings) {
  localStorage.setItem("veyra-voice-settings", JSON.stringify(s));
}

function isTauriEnvironment(): boolean {
  return typeof window !== "undefined" && ("__TAURI_INTERNALS__" in window || "__TAURI__" in window);
}

async function safeInvoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauriEnvironment()) {
    throw new Error(
      "Tauriバックエンドに接続できません。通常のWebブラウザではなく、'npm run tauri dev' で立ち上がるデスクトップウィンドウをご利用ください。"
    );
  }
  return invoke<T>(cmd, args);
}

// --- Typed wrappers over Tauri IPC commands ---

export const api = {
  listProjects: () => safeInvoke<Project[]>("list_projects"),

  createProject: (name: string, rootPath: string | null) =>
    safeInvoke<Project>("create_project", { name, rootPath }),

  projectSnapshot: (projectId: string) =>
    safeInvoke<Snapshot>("project_snapshot", { projectId }),

  createGoal: (projectId: string, objective: string) =>
    safeInvoke<Goal>("create_goal", { projectId, objective }),

  addUserFeedback: (projectId: string, content: string) =>
    safeInvoke<void>("add_user_feedback", { projectId, content }),

  recordDecision: (projectId: string, decision: string, rationale: string | null) =>
    safeInvoke<Decision>("record_decision", { projectId, decision, rationale }),

  sendMessage: (projectId: string, text: string) =>
    safeInvoke<LoopOutcome>("send_message", { projectId, text }),

  runAgent: (goalId: string) => safeInvoke<AgentReport>("run_agent", { goalId }),

  verifyProject: (projectId: string) =>
    safeInvoke<VerificationOutcome>("verify_project", { projectId }),

  // Assistant Mode (M6)
  listAvailableTools: () =>
    safeInvoke<ToolManifest[]>("list_available_tools"),

  runAssistantTask: (
    instruction: string,
    projectId?: string | null,
    permissionLevel?: "read" | "low_risk" | "medium_risk" | "high_risk" | "critical"
  ) =>
    safeInvoke<AssistantReport>("run_assistant_task", {
      projectId: projectId ?? null,
      instruction,
      permissionLevel: permissionLevel ?? null,
    }),

  // Voice (M5): Synthesizes and actually PLAYS the speech audio
  voiceStart: () => safeInvoke<void>("voice_start"),
  voiceStop: (model: string) =>
    safeInvoke<TranscriptPayload>("voice_stop", { model }),
  speak: async (text: string, settings: VoiceSettings): Promise<void> => {
    // 1. Try Style-Bert-VITS2 backend via Tauri IPC
    try {
      const base64Wav = await safeInvoke<string>("speak", {
        text,
        sbv2Url: settings.sbv2Url,
        modelName: settings.ttsModel,
      });

      if (base64Wav) {
        return new Promise<void>((resolve) => {
          const audio = new Audio("data:audio/wav;base64," + base64Wav);
          audio.onended = () => resolve();
          audio.onerror = () => {
            // Audio tag error, fallback to Web Speech
            speakViaWebSpeech(text).then(resolve);
          };
          audio.play().catch(() => {
            speakViaWebSpeech(text).then(resolve);
          });
        });
      }
    } catch {
      // Style-Bert-VITS2 is not running - fallback to native Windows Japanese TTS
    }

    // 2. Fallback to Windows native speech synthesis (Web Speech API)
    return speakViaWebSpeech(text);
  },

  // Overlay toggle
  toggleOverlay: () => safeInvoke<boolean>("toggle_overlay"),

  // AI backend liveness (Ollama daemon reachable?)
  aiHealth: () => safeInvoke<boolean>("ai_health"),
};

/**
 * Windows / Chromium native Japanese text-to-speech fallback.
 * Uses Microsoft Ayumi, Haruka, Sayaka, Ichiro, or standard Japanese voice.
 */
function speakViaWebSpeech(text: string): Promise<void> {
  return new Promise<void>((resolve) => {
    if (typeof window === "undefined" || !("speechSynthesis" in window)) {
      resolve();
      return;
    }

    window.speechSynthesis.cancel(); // Stop any previous utterance

    const cleanText = text
      .replace(/[#*`_~]/g, "")
      .replace(/https?:\/\/\S+/g, "リンク")
      .trim();

    if (!cleanText) {
      resolve();
      return;
    }

    const utterance = new SpeechSynthesisUtterance(cleanText);
    utterance.lang = "ja-JP";
    utterance.rate = 1.05;
    utterance.pitch = 1.0;

    // Pick best Japanese voice available on Windows
    const voices = window.speechSynthesis.getVoices();
    const jaVoice = voices.find((v) => v.lang === "ja-JP" || v.lang === "ja_JP")
      || voices.find((v) => v.name.includes("Japanese") || v.name.includes("Haruka") || v.name.includes("Ayumi"));

    if (jaVoice) {
      utterance.voice = jaVoice;
    }

    utterance.onend = () => resolve();
    utterance.onerror = () => resolve();

    window.speechSynthesis.speak(utterance);
  });
}
