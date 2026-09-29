import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  api,
  loadVoiceSettings,
  saveVoiceSettings,
  type AgentReport,
  type AssistantReport,
  type Project,
  type Snapshot,
  type TranscriptPayload,
  type VerificationOutcome,
  type VoiceSettings,
} from "./api";
import SiriOrb, { type AssistantState } from "./components/SiriOrb";
import ProjectView from "./components/ProjectView";
import { getInitialLocale, saveLocale, translations, type Locale } from "./i18n";

export default function App() {
  // Overlay visibility state (controlled by backend events)
  const [animState, setAnimState] = useState<"entering" | "exiting" | "visible">("visible");

  const [locale, setLocale] = useState<Locale>(getInitialLocale);
  const t = translations[locale];
  const handleLocaleChange = (loc: Locale) => {
    setLocale(loc);
    saveLocale(loc);
  };

  const [projects, setProjects] = useState<Project[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [inputText, setInputText] = useState("");
  const [lastResponse, setLastResponse] = useState<string | null>(null);
  const [lastReport, setLastReport] = useState<AssistantReport | null>(null);
  const [isProcessing, setIsProcessing] = useState(false);
  const [speaking, setSpeaking] = useState(false);
  const [recording, setRecording] = useState(false);
  const [voiceError, setVoiceError] = useState<string | null>(null);
  const [incomingTranscript, setIncomingTranscript] = useState<string | null>(null);
  const [voiceSettings, setVoiceSettings] = useState<VoiceSettings>(loadVoiceSettings);
  const [aiOnline, setAiOnline] = useState<boolean | null>(null);
  const [copied, setCopied] = useState(false);
  const [inputFocused, setInputFocused] = useState(false);

  // Dashboard Drawer State (M4 Smart Integration of ProjectView)
  const [showDashboard, setShowDashboard] = useState(false);
  const [agentReport, setAgentReport] = useState<AgentReport | null>(null);
  const [verification, setVerification] = useState<VerificationOutcome | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  const inputRef = useRef<HTMLInputElement>(null);
  const settingsRef = useRef(voiceSettings);
  settingsRef.current = voiceSettings;

  const updateVoiceSettings = (s: VoiceSettings) => {
    setVoiceSettings(s);
    saveVoiceSettings(s);
  };

  // ---------------------------------------------------------------
  // Backend events
  // ---------------------------------------------------------------
  useEffect(() => {
    const unlisteners: Promise<() => void>[] = [
      listen<TranscriptPayload>("voice-transcript", (e) => {
        setIncomingTranscript(e.payload.cleaned || e.payload.raw);
      }),
      listen<boolean>("voice-recording", (e) => setRecording(e.payload)),
      listen<string>("voice-error", (e) => setVoiceError(e.payload)),
      listen<{ received: number; total: number | null }>("voice-model-progress", (e) => {
        const { received, total } = e.payload;
        setVoiceError(
          total
            ? `whisperモデルダウンロード中… ${(received / 1e6).toFixed(0)}MB / ${(total / 1e6).toFixed(0)}MB`
            : `whisperモデルダウンロード中… ${(received / 1e6).toFixed(0)}MB`,
        );
      }),
      // Overlay visibility events from Rust (tray click, hotkey, etc.)
      listen("overlay-show", () => {
        setAnimState("entering");
        setTimeout(() => inputRef.current?.focus(), 60);
      }),
      listen("overlay-hide", () => {
        setAnimState("exiting");
      }),
    ];
    return () => {
      void Promise.all(unlisteners).then((uns) => uns.forEach((un) => un()));
    };
  }, []);

  // Auto-focus input & keyboard handling
  useEffect(() => {
    const handleFocus = () => inputRef.current?.focus();
    const handleKeyDown = (e: KeyboardEvent) => {
      // Ctrl+D / Cmd+D: Toggle Project Dashboard drawer
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "d") {
        e.preventDefault();
        setShowDashboard((v) => !v);
        return;
      }
      if (e.key === "Escape") {
        if (showDashboard) {
          setShowDashboard(false);
          return;
        }
        setAnimState("exiting");
        void api.toggleOverlay();
      }
    };
    window.addEventListener("focus", handleFocus);
    window.addEventListener("keydown", handleKeyDown);
    return () => {
      window.removeEventListener("focus", handleFocus);
      window.removeEventListener("keydown", handleKeyDown);
    };
  }, [showDashboard]);

  // Animation transition timing
  useEffect(() => {
    if (animState === "entering") {
      const timer = setTimeout(() => {
        setAnimState("visible");
        inputRef.current?.focus();
      }, 420);
      return () => clearTimeout(timer);
    }
  }, [animState]);

  // Auto-fill input when voice transcript arrives
  useEffect(() => {
    if (incomingTranscript) {
      setInputText(incomingTranscript);
    }
  }, [incomingTranscript]);

  // AI backend liveness
  useEffect(() => {
    let cancelled = false;
    const check = async () => {
      try {
        const ok = await api.aiHealth();
        if (!cancelled) setAiOnline(ok);
      } catch {
        if (!cancelled) setAiOnline(false);
      }
    };
    void check();
    const timer = setInterval(check, 10_000);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, []);

  // Data loading
  const refreshProjects = useCallback(async () => {
    try {
      const list = await api.listProjects();
      setProjects(list);
      return list;
    } catch {
      return [];
    }
  }, []);

  const refreshSnapshot = useCallback(async (projectId: string) => {
    try {
      setSnapshot(await api.projectSnapshot(projectId));
    } catch {
      // ignore
    }
  }, []);

  useEffect(() => {
    void (async () => {
      const list = await refreshProjects();
      if (list.length > 0) {
        setSelectedId(list[0].id);
        await refreshSnapshot(list[0].id);
      } else {
        const defaultProject = await api.createProject("Veyra Assistant", null).catch(() => null);
        if (defaultProject) {
          await refreshProjects();
          setSelectedId(defaultProject.id);
          await refreshSnapshot(defaultProject.id);
        }
      }
    })();
  }, [refreshProjects, refreshSnapshot]);

  // ---------------------------------------------------------------
  // Project & Dashboard Handlers
  // ---------------------------------------------------------------
  const handleSelectProject = (id: string) => {
    setSelectedId(id);
    void refreshSnapshot(id);
  };

  const handleCreateProject = async (name: string) => {
    try {
      const p = await api.createProject(name, null);
      await refreshProjects();
      setSelectedId(p.id);
      await refreshSnapshot(p.id);
    } catch (e) {
      alert(`プロジェクト作成エラー: ${String(e)}`);
    }
  };

  const handleCreateGoal = async (objective: string) => {
    if (!selectedId) return;
    try {
      await api.createGoal(selectedId, objective);
      await refreshSnapshot(selectedId);
    } catch (e) {
      alert(`Goal作成エラー: ${String(e)}`);
    }
  };

  const handleAddFeedback = async (content: string) => {
    if (!selectedId) return;
    try {
      await api.addUserFeedback(selectedId, content);
      await refreshSnapshot(selectedId);
    } catch (e) {
      alert(`Feedback記録エラー: ${String(e)}`);
    }
  };

  const handleSendMessage = async (text: string) => {
    if (!selectedId) return;
    try {
      await api.sendMessage(selectedId, text);
      await refreshSnapshot(selectedId);
    } catch (e) {
      alert(`メッセージ送信エラー: ${String(e)}`);
    }
  };

  const handleRunAgent = async (goalId: string) => {
    setBusy(goalId);
    try {
      const rep = await api.runAgent(goalId);
      setAgentReport(rep);
      if (selectedId) await refreshSnapshot(selectedId);
    } catch (e) {
      alert(`Agent実行エラー: ${String(e)}`);
    } finally {
      setBusy(null);
    }
  };

  const handleVerifyProject = async () => {
    if (!selectedId) return;
    setBusy("verify");
    try {
      const outcome = await api.verifyProject(selectedId);
      setVerification(outcome);
      await refreshSnapshot(selectedId);
    } catch (e) {
      alert(`検証エラー: ${String(e)}`);
    } finally {
      setBusy(null);
    }
  };

  // ---------------------------------------------------------------
  // Pill HUD Handlers
  // ---------------------------------------------------------------
  const assistantState: AssistantState = recording
    ? "listening"
    : isProcessing
    ? "thinking"
    : speaking
    ? "speaking"
    : "idle";

  const handleSubmit = async (customText?: string) => {
    const text = (customText ?? inputText).trim();
    if (!text || isProcessing) return;
    setInputText("");
    setIsProcessing(true);
    setLastResponse(null);
    setLastReport(null);

    try {
      const report = await api.runAssistantTask(text, selectedId);
      setLastReport(report);
      setLastResponse(report.response);
      if (settingsRef.current.autoSpeak && report.response) {
        setSpeaking(true);
        await api.speak(report.response, settingsRef.current).catch(() => {});
        setSpeaking(false);
      }
    } catch (e) {
      setLastResponse(`エラー: ${String(e)}`);
    } finally {
      setIsProcessing(false);
      if (selectedId) await refreshSnapshot(selectedId);
    }
  };

  const handleDismiss = () => {
    if (showDashboard) {
      setShowDashboard(false);
      return;
    }
    setAnimState("exiting");
    void api.toggleOverlay();
  };

  const handleCopyResponse = async () => {
    if (!lastResponse) return;
    try {
      await navigator.clipboard.writeText(lastResponse);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      // clipboard unavailable
    }
  };

  // Web Speech API recognition ref for instantaneous real-time transcription
  const recognitionRef = useRef<any>(null);

  const handleVoiceStart = async () => {
    setVoiceError(null);
    setRecording(true);

    try {
      const SpeechRec = (window as any).SpeechRecognition || (window as any).webkitSpeechRecognition;
      if (SpeechRec) {
        const rec = new SpeechRec();
        rec.lang = "ja-JP";
        rec.continuous = true;
        rec.interimResults = true;
        rec.onresult = (event: any) => {
          let text = "";
          for (let i = 0; i < event.results.length; i++) {
            text += event.results[i][0].transcript;
          }
          if (text.trim()) {
            setInputText(text.trim());
          }
        };
        rec.onerror = () => {};
        rec.start();
        recognitionRef.current = rec;
      }
    } catch {
      // Web speech not available - whisper will handle it
    }

    await api.voiceStart().catch((e) => {
      if (!recognitionRef.current) {
        setRecording(false);
        setVoiceError(String(e));
      }
    });
  };

  const handleVoiceStop = async () => {
    setRecording(false);

    if (recognitionRef.current) {
      try {
        recognitionRef.current.stop();
      } catch {}
      recognitionRef.current = null;
    }

    const modelToUse = voiceSettings.whisperModel || "tiny";
    setVoiceError("音声認識・文字起こし中…");
    const payload = await api.voiceStop(modelToUse).catch((e) => {
      if (!inputText) {
        setVoiceError(String(e));
      } else {
        setVoiceError(null);
      }
      return null;
    });

    if (payload && (payload.cleaned || payload.raw)) {
      setVoiceError(null);
      setInputText(payload.cleaned || payload.raw);
    } else if (inputText) {
      setVoiceError(null);
    }
  };

  const activeGoal = snapshot?.goals.find((g) => g.status === "active");
  const selectedProject = projects.find((p) => p.id === selectedId) ?? projects[0] ?? null;
  const overlayAnimClass = animState === "entering" ? "overlay-enter" : animState === "exiting" ? "overlay-exit" : "";

  return (
    <div className="w-full h-full relative overflow-visible select-none" style={{ background: "transparent" }}>
      {/* Click-away dismiss backdrop */}
      <div className="absolute inset-0 z-0" onClick={handleDismiss} />

      {/* Main Center Floating HUD */}
      <div className={`relative z-10 mx-auto max-w-[680px] pt-3 px-3 transition-all duration-300 ${overlayAnimClass}`}>
        {/* Subtle Ambient Aura behind the Capsule (Amber & Gold Glow) */}
        <div className="absolute top-2 left-1/2 -translate-x-1/2 w-3/4 h-24 rounded-full bg-amber-500/18 blur-3xl pointer-events-none -z-10" />

        {/* ------------------------------------------------------------- */}
        {/* Luxury Capsule Bar (Inspired by User Reference Image 2)        */}
        {/* ------------------------------------------------------------- */}
        <div className="luxury-pill relative flex items-center h-[58px] px-3.5 gap-2.5 shadow-2xl drag-region">
          {/* Left: Siri/Veyra Orb indicator & glowing gold caret */}
          <div className="flex items-center gap-2 shrink-0 no-drag pl-0.5">
            <div
              className="cursor-pointer transition-transform hover:scale-105 active:scale-95"
              onClick={() => {
                if (recording) void handleVoiceStop();
                else void handleVoiceStart();
              }}
              title={recording ? "録音停止" : "音声入力開始"}
            >
              <SiriOrb state={assistantState} size="sm" />
            </div>

            {/* Glowing Amber Caret from reference image (visible when input is idle or empty) */}
            {!inputText && !inputFocused && (
              <div className="luxury-cursor ml-0.5" />
            )}
          </div>

          {/* Center: Search / Command Input Field */}
          <div className="flex-1 relative flex items-center no-drag">
            <input
              ref={inputRef}
              type="text"
              value={inputText}
              onFocus={() => setInputFocused(true)}
              onBlur={() => setInputFocused(false)}
              onChange={(e) => setInputText(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") void handleSubmit();
                if (e.key === "Escape") handleDismiss();
              }}
              placeholder={t.inputPlaceholder}
              className="w-full bg-transparent px-1 py-1 text-[15px] font-normal text-amber-50 placeholder:text-zinc-500 placeholder:font-light outline-none tracking-wide"
            />
          </div>

          {/* Right Action Controls: Mic, Submit, Dashboard Drawer, Config & Dismiss */}
          <div className="flex items-center gap-1.5 shrink-0 no-drag">
            {/* Minimalist Gold Submit Arrow Button (Appears when text is entered) */}
            {inputText.trim().length > 0 && (
              <button
                onClick={() => void handleSubmit()}
                disabled={isProcessing}
                className="flex h-9 w-9 items-center justify-center rounded-full bg-gradient-to-br from-amber-400 to-amber-600 text-zinc-950 font-semibold hover:from-amber-300 hover:to-amber-500 shadow-md shadow-amber-500/30 transition-all duration-200 cursor-pointer active:scale-95 disabled:opacity-40"
                title={t.sendTooltip}
              >
                {isProcessing ? (
                  <span className="w-3.5 h-3.5 rounded-full border-2 border-zinc-950/40 border-t-zinc-950 animate-spin" />
                ) : (
                  <svg className="w-4 h-4 translate-x-0.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2.5}>
                    <path strokeLinecap="round" strokeLinejoin="round" d="M14 5l7 7m0 0l-7 7m7-7H3" />
                  </svg>
                )}
              </button>
            )}

            {/* Reference Image 2: Minimalist Mic Icon */}
            <button
              onPointerDown={() => void handleVoiceStart()}
              onPointerUp={() => void handleVoiceStop()}
              className={`flex h-9 w-9 items-center justify-center rounded-full transition-all duration-300 cursor-pointer ${
                recording
                  ? "bg-rose-500 text-white shadow-lg shadow-rose-500/50 scale-105 pulse-ring"
                  : "text-amber-100/70 hover:text-amber-100 hover:bg-white/8 active:scale-95"
              }`}
              title={t.micTooltip}
            >
              <svg className="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={1.8}>
                <path strokeLinecap="round" strokeLinejoin="round" d="M19 11a7 7 0 01-7 7m0 0a7 7 0 01-7-7m7 7v4m-4 0h8m-4-18a3 3 0 00-3 3v8a3 3 0 006 0V6a3 3 0 00-3-3z" />
              </svg>
            </button>

            {/* Dashboard Drawer Toggle Button (M4 Feature: ProjectView Smart Integration) */}
            <button
              onClick={() => setShowDashboard((v) => !v)}
              className={`flex h-8 w-8 items-center justify-center rounded-full transition-all duration-200 cursor-pointer active:scale-95 ${
                showDashboard
                  ? "bg-amber-500/30 text-amber-300 border border-amber-500/50 shadow-sm shadow-amber-500/40 scale-105"
                  : "text-amber-200/60 hover:text-amber-200 hover:bg-white/8"
              }`}
              title={t.dashboardTooltip}
            >
              <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
                <path strokeLinecap="round" strokeLinejoin="round" d="M4 6h16M4 12h16m-7 6h7" />
              </svg>
            </button>

            {/* Quick Project selector (Subtle Dropdown) */}
            {projects.length > 0 && (
              <select
                value={selectedId ?? ""}
                onChange={(e) => {
                  const newId = e.target.value;
                  setSelectedId(newId);
                  void refreshSnapshot(newId);
                }}
                className="bg-transparent text-amber-200/50 hover:text-amber-100 text-[11px] rounded-md px-1.5 py-1 outline-none transition-colors cursor-pointer border border-transparent hover:border-amber-500/20 max-w-[90px] truncate"
                title={t.projectSelectorTitle}
              >
                {projects.map((p) => (
                  <option key={p.id} value={p.id} className="bg-zinc-900 text-zinc-200">
                    {p.name}
                  </option>
                ))}
              </select>
            )}

            {/* Auto-speak Toggle */}
            <button
              onClick={() => updateVoiceSettings({ ...voiceSettings, autoSpeak: !voiceSettings.autoSpeak })}
              className={`flex h-7 w-7 items-center justify-center rounded-full text-xs transition-colors cursor-pointer ${
                voiceSettings.autoSpeak
                  ? "text-amber-300 hover:bg-amber-500/15"
                  : "text-zinc-600 hover:text-zinc-400 hover:bg-white/5"
              }`}
              title={voiceSettings.autoSpeak ? t.autoSpeakOn : t.autoSpeakOff}
            >
              {voiceSettings.autoSpeak ? "🔊" : "🔇"}
            </button>

            {/* Close button */}
            <button
              onClick={handleDismiss}
              className="flex h-7 w-7 items-center justify-center rounded-full text-zinc-500 hover:text-amber-200 hover:bg-white/8 transition-colors cursor-pointer text-xs"
              title={t.closeTooltip}
            >
              ✕
            </button>
          </div>
        </div>

        {/* Voice error / progress notification */}
        {voiceError && (
          <div className="mt-2.5 mx-auto max-w-[620px] rounded-full border border-amber-500/30 bg-amber-500/10 px-4 py-1.5 text-[11px] text-amber-200 text-center backdrop-blur-xl animate-fade-in">
            {voiceError}
          </div>
        )}

        {/* ------------------------------------------------------------- */}
        {/* Smart Integrated Drawer: ProjectView (When showDashboard is true) */}
        {/* ------------------------------------------------------------- */}
        {showDashboard && (
          <div className="mt-3.5 response-enter">
            <ProjectView
              projects={projects}
              selectedProject={selectedProject}
              onSelectProject={handleSelectProject}
              onCreateProject={handleCreateProject}
              snapshot={snapshot}
              onCreateGoal={handleCreateGoal}
              onAddFeedback={handleAddFeedback}
              onSendMessage={handleSendMessage}
              sending={isProcessing}
              onRunAgent={handleRunAgent}
              onVerifyProject={handleVerifyProject}
              agentReport={agentReport}
              verification={verification}
              busy={busy}
              settings={voiceSettings}
              onSettingsChange={updateVoiceSettings}
              onClose={() => setShowDashboard(false)}
              locale={locale}
              onLocaleChange={handleLocaleChange}
            />
          </div>
        )}

        {/* ------------------------------------------------------------- */}
        {/* Quick Action Chips (Floating Amber Glass Badges)              */}
        {/* ------------------------------------------------------------- */}
        {!showDashboard && (
          <div className="mt-3 flex flex-wrap items-center justify-center gap-1.5 no-drag">
            {[
              { emoji: "▶", label: t.chipYoutube, cmd: t.chipYoutubeCmd },
              { emoji: "🔍", label: t.chipGoogle, cmd: t.chipGoogleCmd },
              { emoji: "📝", label: t.chipNotepad, cmd: t.chipNotepadCmd },
              { emoji: "🪟", label: t.chipWindow, cmd: t.chipWindowCmd },
              { emoji: "🎯", label: t.chipAgent, cmd: t.chipAgentCmd },
            ].map(({ emoji, label, cmd }) => (
              <button
                key={label}
                onClick={() => void handleSubmit(cmd)}
                className="luxury-chip flex items-center gap-1.5 px-3 py-1 text-[11px] cursor-pointer"
              >
                <span className="text-amber-400 text-xs">{emoji}</span>
                <span className="font-medium">{label}</span>
              </button>
            ))}
          </div>
        )}

        {/* ------------------------------------------------------------- */}
        {/* Liquid Expansion Result Card (When Processing or Responded)    */}
        {/* ------------------------------------------------------------- */}
        {!showDashboard && (lastResponse || lastReport || isProcessing) && (
          <div className="mt-3.5 luxury-card p-4.5 response-enter no-drag relative overflow-hidden">
            {/* Left Amber Accent Bar */}
            <span className="absolute left-0 top-3 bottom-3 w-[3px] rounded-full bg-gradient-to-b from-amber-300 via-amber-500 to-amber-700 shadow-sm shadow-amber-500/50" />

            <div className="flex items-center justify-between pb-2 border-b border-amber-500/15">
              <div className="flex items-center gap-2 pl-2">
                {isProcessing ? (
                  <span className="flex items-center gap-2 text-xs font-semibold text-amber-300">
                    <span className="w-3.5 h-3.5 rounded-full border-2 border-amber-400/30 border-t-amber-400 animate-spin" />
                    思考＆アクション実行中…
                  </span>
                ) : (
                  <>
                    <span className={`text-xs font-semibold tracking-wider ${
                      lastReport?.success ? "text-amber-400" : "text-amber-200"
                    }`}>
                      {lastReport?.success ? "✓ 実行完了" : "応答"}
                    </span>
                    {lastReport?.steps && lastReport.steps.length > 0 && (
                      <span className="rounded-full bg-amber-500/15 border border-amber-500/25 px-2.5 py-0.5 text-[10px] text-amber-300 font-mono">
                        {lastReport.steps.map((s) => s.tool_name ?? "direct").join(" → ")}
                      </span>
                    )}
                  </>
                )}
              </div>

              {lastResponse && !isProcessing && (
                <div className="flex items-center gap-1">
                  <button
                    onClick={() => void handleCopyResponse()}
                    className="text-[11px] text-zinc-400 hover:text-amber-300 transition-colors cursor-pointer flex items-center gap-1 px-2 py-0.5 rounded-md hover:bg-white/5"
                    title="コピー"
                  >
                    {copied ? "✓ コピー完了" : "⧉ コピー"}
                  </button>
                  <button
                    onClick={() => {
                      setSpeaking(true);
                      void api.speak(lastResponse, settingsRef.current).finally(() => setSpeaking(false));
                    }}
                    className="text-[11px] text-zinc-400 hover:text-amber-300 transition-colors cursor-pointer flex items-center gap-1 px-2 py-0.5 rounded-md hover:bg-white/5"
                    title="読み上げ"
                  >
                    🔊 再生
                  </button>
                </div>
              )}
            </div>

            {/* Main content response */}
            {isProcessing ? (
              <p className="mt-3 pl-2 text-sm text-zinc-400 leading-relaxed italic">
                リクエストを解析し、Windowsデスクトップ環境でアクションを処理しています…
              </p>
            ) : (
              <p className="mt-3 pl-2 text-[14px] text-amber-50/95 leading-relaxed whitespace-pre-wrap font-normal">
                {lastResponse}
              </p>
            )}

            {/* Tool execution steps details */}
            {lastReport?.steps && lastReport.steps.length > 0 && (
              <div className="mt-3.5 pt-2.5 border-t border-amber-500/10 space-y-1.5 pl-2">
                {lastReport.steps.map((s, idx) => (
                  <div
                    key={idx}
                    className="flex items-center justify-between text-[11px] text-zinc-400 bg-black/30 border border-amber-500/10 rounded-lg px-2.5 py-1.5"
                  >
                    <span className="font-mono text-amber-300 flex items-center gap-1.5">
                      <span className={s.success ? "text-emerald-400" : "text-rose-400"}>
                        {s.success ? "●" : "✕"}
                      </span>
                      {s.tool_name ?? "action"}
                    </span>
                    <span className="text-[10px] text-zinc-400 truncate max-w-[260px]">{s.detail}</span>
                  </div>
                ))}
              </div>
            )}
          </div>
        )}

        {/* Active Goal Indicator (Click to open ProjectView Goals tab) */}
        {!showDashboard && activeGoal && (
          <div
            onClick={() => setShowDashboard(true)}
            className="mt-2.5 flex items-center justify-center gap-2 text-[11px] text-zinc-400 hover:text-amber-300 transition-colors cursor-pointer no-drag group"
            title="クリックでGoal詳細・Agent実行画面を開く"
          >
            <span className="text-amber-400 group-hover:scale-110 transition-transform">🎯</span>
            <span className="text-zinc-300 truncate max-w-[380px] group-hover:underline">{activeGoal.objective}</span>
            <span className="text-emerald-400 font-mono">({activeGoal.acceptance_criteria.length} AC)</span>
          </div>
        )}

        {/* Bottom Minimalist Footer */}
        <div className="mt-3 flex items-center justify-between px-2 text-[10px] text-zinc-500 no-drag">
          <span>Ctrl+Shift+Space 呼出 · Ctrl+D ダッシュボード · Esc 閉じる</span>
          <div className="flex items-center gap-1.5">
            <span
              className={`h-1.5 w-1.5 rounded-full ${
                aiOnline === null
                  ? "bg-zinc-500"
                  : aiOnline
                  ? "bg-amber-400 shadow-sm shadow-amber-400/80 animate-pulse"
                  : "bg-rose-500"
              }`}
            />
            <span className="text-zinc-400">
              {aiOnline === null
                ? "AI接続中…"
                : aiOnline
                ? "Ollama Ready"
                : "Ollama Offline"}
            </span>
          </div>
        </div>
      </div>
    </div>
  );
}
