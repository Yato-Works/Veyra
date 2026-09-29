import { useState, useRef, useEffect } from "react";
import SiriOrb, { type AssistantState } from "./SiriOrb";
import type {
  AssistantReport,
  Project,
  Snapshot,
  VoiceSettings,
} from "../api";

interface WhisperHUDProps {
  project: Project | null;
  snapshot: Snapshot | null;
  recording: boolean;
  sending: boolean;
  voiceError: string | null;
  incomingTranscript: string | null;
  settings: VoiceSettings;
  onVoiceStart: () => Promise<void>;
  onVoiceStop: () => Promise<void>;
  onSendMessage: (text: string) => Promise<void>;
  onRunAssistantTask: (instruction: string) => Promise<AssistantReport>;
  onSpeak: (text: string) => Promise<void>;
  onToggleFullWorkspace: () => void;
}

export default function WhisperHUD({
  project,
  snapshot,
  recording,
  sending,
  voiceError,
  incomingTranscript,
  settings,
  onVoiceStart,
  onVoiceStop,
  onSendMessage,
  onRunAssistantTask,
  onSpeak,
  onToggleFullWorkspace,
}: WhisperHUDProps) {
  const [inputText, setInputText] = useState("");
  const [lastResponse, setLastResponse] = useState<string | null>(null);
  const [lastReport, setLastReport] = useState<AssistantReport | null>(null);
  const [isProcessing, setIsProcessing] = useState(false);
  const [speaking, setSpeaking] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);

  // Auto-focus input on mount
  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  // Update input when voice transcript arrives
  useEffect(() => {
    if (incomingTranscript) {
      setInputText(incomingTranscript);
    }
  }, [incomingTranscript]);

  // Determine current assistant state
  const assistantState: AssistantState = recording
    ? "listening"
    : sending || isProcessing
    ? "thinking"
    : speaking
    ? "speaking"
    : "idle";

  const handleSubmit = async (customText?: string) => {
    const text = (customText ?? inputText).trim();
    if (!text || isProcessing || sending) return;
    setInputText("");
    setIsProcessing(true);
    setLastResponse(null);

    try {
      // Run as assistant task if project is active, or send message
      if (project) {
        const report = await onRunAssistantTask(text);
        setLastReport(report);
        setLastResponse(report.response);
        if (settings.autoSpeak && report.response) {
          setSpeaking(true);
          await onSpeak(report.response);
          setSpeaking(false);
        }
      } else {
        await onSendMessage(text);
      }
    } catch (e) {
      setLastResponse(`エラーが発生しました: ${String(e)}`);
    } finally {
      setIsProcessing(false);
    }
  };

  const activeGoal = snapshot?.goals.find((g) => g.status === "active");

  return (
    <div className="relative min-h-screen w-full flex flex-col items-center justify-between p-6 select-none overflow-hidden">
      {/* Dynamic ambient background orbs blending into desktop */}
      <div className="absolute top-1/4 left-1/3 -translate-x-1/2 -translate-y-1/2 w-96 h-96 rounded-full bg-violet-600/15 blur-3xl pointer-events-none animate-aura-float" />
      <div className="absolute top-1/3 right-1/4 -translate-y-1/2 w-80 h-80 rounded-full bg-cyan-500/10 blur-3xl pointer-events-none animate-aura-float [animation-delay:3s]" />
      <div className="absolute bottom-1/4 left-1/2 -translate-x-1/2 w-96 h-72 rounded-full bg-pink-600/10 blur-3xl pointer-events-none" />

      {/* Top Header Bar */}
      <header className="relative z-10 w-full max-w-4xl flex items-center justify-between">
        <div className="flex items-center gap-3">
          <div className="flex items-center gap-2 rounded-full glass-pill px-3.5 py-1.5 shadow-lg">
            <span className="h-2 w-2 rounded-full bg-emerald-400 animate-pulse" />
            <span className="text-xs font-semibold tracking-wide text-white/90">
              Veyra <span className="text-violet-400 font-normal">Whisper HUD</span>
            </span>
          </div>
          {project && (
            <div className="hidden sm:flex items-center gap-2 text-xs text-zinc-400 glass-pill px-3 py-1">
              <span className="text-zinc-500">Project:</span>
              <span className="text-zinc-200 font-medium truncate max-w-[140px]">
                {project.name}
              </span>
            </div>
          )}
        </div>

        <div className="flex items-center gap-2">
          <button
            onClick={onToggleFullWorkspace}
            className="flex items-center gap-1.5 rounded-full glass-pill px-4 py-1.5 text-xs font-medium text-zinc-300 hover:text-white hover:bg-white/10 transition-all cursor-pointer shadow-md"
            title="ワークスペース詳細画面へ切り替え"
          >
            <span>ワークスペース</span>
            <span className="text-violet-400">↗</span>
          </button>
        </div>
      </header>

      {/* Main Center Area: Siri / Whisper Orb & Voice Floating Hub */}
      <main className="relative z-10 w-full max-w-2xl flex flex-col items-center my-auto py-8">
        {/* Glow status title */}
        <div className="mb-6 flex flex-col items-center text-center">
          <div className="mb-4">
            <SiriOrb
              state={assistantState}
              size="lg"
              onClick={() => {
                if (recording) void onVoiceStop();
                else void onVoiceStart();
              }}
            />
          </div>

          <h2 className="text-xl sm:text-2xl font-semibold tracking-tight text-white drop-shadow-sm flex items-center gap-2">
            {recording ? (
              <span className="text-rose-300 animate-pulse">
                お話しください…
              </span>
            ) : isProcessing || sending ? (
              <span className="text-violet-300">
                思考中・操作中…
              </span>
            ) : speaking ? (
              <span className="text-cyan-300">
                お答えしています…
              </span>
            ) : (
              <span>Veyraに話しかけてください</span>
            )}
          </h2>

          <p className="mt-1.5 text-xs text-zinc-400 max-w-sm">
            {recording
              ? "マイクから音声を入力中 (離すと送信されます)"
              : "「YouTubeつけて」「Agentの進捗を教えて」「メモ帳開いて」など"}
          </p>

          {voiceError && (
            <div className="mt-3 rounded-full border border-amber-500/30 bg-amber-500/15 px-4 py-1 text-xs text-amber-200 backdrop-blur-md">
              {voiceError}
            </div>
          )}
        </div>

        {/* Floating Glass Pill Input Bar */}
        <div className="w-full glass-pill rounded-2xl p-2 shadow-2xl transition-all duration-300 border border-white/15">
          <div className="flex items-center gap-2">
            {/* Push-to-talk mic button */}
            <button
              onPointerDown={() => void onVoiceStart()}
              onPointerUp={() => void onVoiceStop()}
              className={`flex h-11 w-11 shrink-0 items-center justify-center rounded-xl transition-all duration-300 cursor-pointer ${
                recording
                  ? "bg-rose-500 text-white shadow-lg shadow-rose-500/40 scale-105"
                  : "bg-white/5 text-zinc-300 hover:bg-white/10 hover:text-white"
              }`}
              title="押している間録音 (Ctrl+Space)"
            >
              <span className="text-base">{recording ? "⏹" : "🎙"}</span>
            </button>

            <input
              ref={inputRef}
              type="text"
              value={inputText}
              onChange={(e) => setInputText(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && void handleSubmit()}
              placeholder="指示や質問を入力、またはマイクで話しかける…"
              className="flex-1 bg-transparent px-3 py-2 text-sm sm:text-base text-zinc-100 placeholder:text-zinc-500 outline-none"
            />

            <button
              onClick={() => void handleSubmit()}
              disabled={isProcessing || sending || !inputText.trim()}
              className="flex h-10 px-4 items-center justify-center rounded-xl bg-gradient-to-r from-violet-600 to-indigo-600 font-medium text-xs sm:text-sm text-white hover:from-violet-500 hover:to-indigo-500 disabled:opacity-40 transition-all cursor-pointer shadow-md"
            >
              {isProcessing || sending ? "実行中…" : "送信"}
            </button>
          </div>
        </div>

        {/* Quick Suggestion Chips */}
        <div className="mt-4 flex flex-wrap items-center justify-center gap-2">
          <button
            onClick={() => void handleSubmit("Browserを開いてYouTubeをつけて")}
            className="flex items-center gap-1.5 rounded-full glass-pill px-3.5 py-1 text-xs text-zinc-300 hover:text-white hover:bg-white/10 transition-all cursor-pointer"
          >
            <span>▶</span>
            <span>YouTube起動</span>
          </button>
          <button
            onClick={() => void handleSubmit("Agentがゴールに向けて動いているか確認して")}
            className="flex items-center gap-1.5 rounded-full glass-pill px-3.5 py-1 text-xs text-zinc-300 hover:text-white hover:bg-white/10 transition-all cursor-pointer"
          >
            <span>🎯</span>
            <span>Agent進捗確認</span>
          </button>
          <button
            onClick={() => void handleSubmit("メモ帳を開いて (notepad)")}
            className="flex items-center gap-1.5 rounded-full glass-pill px-3.5 py-1 text-xs text-zinc-300 hover:text-white hover:bg-white/10 transition-all cursor-pointer"
          >
            <span>📝</span>
            <span>メモ帳起動</span>
          </button>
          <button
            onClick={() => void handleSubmit("アクティブなウィンドウを調べて")}
            className="flex items-center gap-1.5 rounded-full glass-pill px-3.5 py-1 text-xs text-zinc-300 hover:text-white hover:bg-white/10 transition-all cursor-pointer"
          >
            <span>🪟</span>
            <span>ウィンドウ確認</span>
          </button>
        </div>

        {/* Response Float Card (MacBook Siri / Whisper Card) */}
        {(lastResponse || lastReport) && (
          <div className="mt-6 w-full glass-card rounded-2xl p-5 border border-white/12 shadow-2xl animate-in fade-in slide-in-from-bottom-3 duration-300">
            <div className="flex items-center justify-between pb-3 border-b border-white/10">
              <div className="flex items-center gap-2">
                <span className="text-xs font-semibold tracking-wider uppercase text-violet-400">
                  {lastReport?.success ? "完了" : "応答"}
                </span>
                {lastReport?.steps && lastReport.steps.length > 0 && (
                  <span className="rounded-full bg-violet-500/20 px-2 py-0.5 text-[10px] text-violet-300 font-mono">
                    {lastReport.steps.map((s) => s.tool_name ?? "direct").join(" ➔ ")}
                  </span>
                )}
              </div>
              {lastResponse && (
                <button
                  onClick={() => {
                    setSpeaking(true);
                    void onSpeak(lastResponse).finally(() => setSpeaking(false));
                  }}
                  className="text-xs text-zinc-400 hover:text-violet-300 transition-colors cursor-pointer flex items-center gap-1"
                  title="Style-Bert-VITS2で読み上げ"
                >
                  <span>🔊</span>
                  <span>読み上げ</span>
                </button>
              )}
            </div>

            <p className="mt-3 text-sm sm:text-base text-zinc-100 leading-relaxed whitespace-pre-wrap font-normal">
              {lastResponse}
            </p>

            {lastReport?.steps && lastReport.steps.length > 0 && (
              <div className="mt-4 pt-3 border-t border-white/5 space-y-1.5">
                {lastReport.steps.map((s, idx) => (
                  <div
                    key={idx}
                    className="flex items-center justify-between text-xs text-zinc-400 bg-white/5 rounded-lg px-2.5 py-1.5"
                  >
                    <span className="font-mono text-violet-300">
                      🔧 {s.tool_name ?? "action"}
                    </span>
                    <span className="text-[11px] text-zinc-500">{s.detail}</span>
                  </div>
                ))}
              </div>
            )}
          </div>
        )}
      </main>

      {/* Bottom Footer: Minimal Status Capsule */}
      <footer className="relative z-10 w-full max-w-4xl flex items-center justify-between text-xs text-zinc-500">
        <div className="flex items-center gap-3">
          {activeGoal ? (
            <div className="flex items-center gap-2 glass-pill rounded-full px-3 py-1">
              <span className="text-violet-400">🎯</span>
              <span className="text-zinc-300 max-w-[280px] truncate">
                {activeGoal.objective}
              </span>
              <span className="text-[11px] text-emerald-400">
                ({activeGoal.acceptance_criteria.length} AC)
              </span>
            </div>
          ) : (
            <span className="text-zinc-500">
              ホットキー: <kbd className="font-mono bg-white/10 px-1.5 py-0.5 rounded text-zinc-300">Ctrl+Space</kbd> でいつでも音声入力
            </span>
          )}
        </div>

        <div className="flex items-center gap-2">
          <span className="text-[11px] text-zinc-500">
            {settings.autoSpeak ? "🔊 自動読み上げON" : "🔇 読み上げOFF"}
          </span>
        </div>
      </footer>
    </div>
  );
}
