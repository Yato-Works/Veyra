import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  api,
  loadVoiceSettings,
  saveVoiceSettings,
  type AgentReport,
  type Project,
  type Snapshot,
  type TranscriptPayload,
  type VerificationOutcome,
  type VoiceSettings,
} from "./api";
import ProjectView from "./components/ProjectView";

export default function App() {
  const [projects, setProjects] = useState<Project[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [newProjectName, setNewProjectName] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [sending, setSending] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [agentReport, setAgentReport] = useState<AgentReport | null>(null);
  const [verification, setVerification] = useState<VerificationOutcome | null>(null);
  const [voiceSettings, setVoiceSettings] = useState<VoiceSettings>(loadVoiceSettings);
  const [incomingTranscript, setIncomingTranscript] = useState<string | null>(null);
  const [recording, setRecording] = useState(false);
  const [voiceError, setVoiceError] = useState<string | null>(null);
  const settingsRef = useRef(voiceSettings);
  settingsRef.current = voiceSettings;

  const updateVoiceSettings = (s: VoiceSettings) => {
    setVoiceSettings(s);
    saveVoiceSettings(s);
  };

  // Backend voice events: hotkey transcript, recording state, errors,
  // model download progress (Plan.txt §7-9).
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
            ? `whisperモデルをダウンロード中… ${(received / 1e6).toFixed(0)}MB / ${(total / 1e6).toFixed(0)}MB`
            : `whisperモデルをダウンロード中… ${(received / 1e6).toFixed(0)}MB`,
        );
      }),
    ];
    return () => {
      void Promise.all(unlisteners).then((uns) => uns.forEach((un) => un()));
    };
  }, []);

  const refreshProjects = useCallback(async () => {
    try {
      const list = await api.listProjects();
      setProjects(list);
      return list;
    } catch (e) {
      setError(String(e));
      return [];
    }
  }, []);

  const refreshSnapshot = useCallback(async (projectId: string) => {
    try {
      setSnapshot(await api.projectSnapshot(projectId));
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    void (async () => {
      const list = await refreshProjects();
      if (list.length > 0) {
        setSelectedId(list[0].id);
        await refreshSnapshot(list[0].id);
      }
    })();
  }, [refreshProjects, refreshSnapshot]);

  const selectProject = async (id: string) => {
    setSelectedId(id);
    setSnapshot(null);
    setAgentReport(null);
    setVerification(null);
    await refreshSnapshot(id);
  };

  const handleCreateProject = async () => {
    const name = newProjectName.trim();
    if (!name) return;
    setError(null);
    const project = await api.createProject(name, null).catch((e) => {
      setError(String(e));
      return null;
    });
    if (project) {
      setNewProjectName("");
      await refreshProjects();
      await selectProject(project.id);
    }
  };

  const handleCreateGoal = async (objective: string) => {
    if (!selectedId) return;
    setError(null);
    await api.createGoal(selectedId, objective).catch((e) => setError(String(e)));
    await refreshSnapshot(selectedId);
  };

  const handleAddFeedback = async (content: string) => {
    if (!selectedId) return;
    setError(null);
    await api.addUserFeedback(selectedId, content).catch((e) => setError(String(e)));
    await refreshSnapshot(selectedId);
  };

  const handleSendMessage = async (text: string) => {
    if (!selectedId) return;
    setError(null);
    setSending(true);
    const outcome = await api.sendMessage(selectedId, text).catch((e) => {
      setError(String(e));
      return null;
    });
    await refreshSnapshot(selectedId);
    setSending(false);

    // Auto-speak the assistant response (Plan.txt §7 Assistant Mode).
    if (outcome && settingsRef.current.autoSpeak) {
      await speakText(outcome.response);
    }
  };

  const speakText = async (text: string) => {
    await api.speak(text, settingsRef.current).catch((e) => {
      setVoiceError(
        `読み上げに失敗しました。Style-Bert-VITS2サーバー(Server.bat)が起動しているか確認してください。 (${e})`,
      );
    });
  };

  const handleVoiceStart = async () => {
    setVoiceError(null);
    await api.voiceStart().catch((e) => setVoiceError(String(e)));
  };

  const handleVoiceStop = async () => {
    if (!recording) return;
    const payload = await api.voiceStop(settingsRef.current.whisperModel).catch((e) => {
      setVoiceError(String(e));
      return null;
    });
    if (payload) {
      setIncomingTranscript(payload.cleaned || payload.raw);
    }
  };

  const handleRunAgent = async (goalId: string) => {
    setError(null);
    setBusy(goalId);
    const report = await api.runAgent(goalId).catch((e) => {
      setError(String(e));
      return null;
    });
    if (report) setAgentReport(report);
    setBusy(null);
    if (selectedId) await refreshSnapshot(selectedId);
  };

  const handleVerifyProject = async () => {
    if (!selectedId) return;
    setError(null);
    setBusy("verify");
    const outcome = await api.verifyProject(selectedId).catch((e) => {
      setError(String(e));
      return null;
    });
    if (outcome) setVerification(outcome);
    setBusy(null);
    await refreshSnapshot(selectedId);
  };

  const selectedProject = projects.find((p) => p.id === selectedId) ?? null;

  return (
    <div className="flex h-screen bg-zinc-950 text-zinc-200 font-sans">
      {/* Sidebar */}
      <aside className="flex w-72 shrink-0 flex-col border-r border-zinc-800 bg-zinc-900/60">
        <div className="flex items-center gap-2 border-b border-zinc-800 px-4 py-4">
          <div className="flex h-8 w-8 items-center justify-center rounded-lg bg-violet-600 font-bold text-white">
            V
          </div>
          <div>
            <div className="text-sm font-semibold tracking-wide">Veyra</div>
            <div className="text-[11px] text-zinc-500">AI operating layer</div>
          </div>
        </div>

        <div className="flex-1 overflow-y-auto px-2 py-2">
          {projects.length === 0 && (
            <p className="px-2 py-4 text-xs text-zinc-500">
              プロジェクトがありません。下から作成してください。
            </p>
          )}
          {projects.map((p) => (
            <button
              key={p.id}
              onClick={() => void selectProject(p.id)}
              className={`mb-1 w-full rounded-lg px-3 py-2 text-left text-sm transition-colors ${
                p.id === selectedId
                  ? "bg-violet-600/20 text-violet-200"
                  : "text-zinc-300 hover:bg-zinc-800/70"
              }`}
            >
              {p.name}
            </button>
          ))}
        </div>

        <div className="border-t border-zinc-800 p-3">
          <input
            value={newProjectName}
            onChange={(e) => setNewProjectName(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && void handleCreateProject()}
            placeholder="新しいプロジェクト名"
            className="mb-2 w-full rounded-lg border border-zinc-700 bg-zinc-800/60 px-3 py-2 text-sm outline-none placeholder:text-zinc-500 focus:border-violet-500"
          />
          <button
            onClick={() => void handleCreateProject()}
            className="w-full rounded-lg bg-violet-600 px-3 py-2 text-sm font-medium text-white hover:bg-violet-500"
          >
            プロジェクト作成
          </button>
        </div>
      </aside>

      {/* Main */}
      <main className="flex-1 overflow-y-auto">
        {error && (
          <div className="m-4 rounded-lg border border-red-500/40 bg-red-500/10 px-4 py-3 text-sm text-red-300">
            {error}
          </div>
        )}

        {!selectedProject ? (
          <div className="flex h-full items-center justify-center text-zinc-500">
            プロジェクトを選択、または新規作成してください。
          </div>
        ) : (
          <ProjectView
            project={selectedProject}
            snapshot={snapshot}
            onCreateGoal={handleCreateGoal}
            onAddFeedback={handleAddFeedback}
            onSendMessage={handleSendMessage}
            sending={sending}
            onRunAgent={handleRunAgent}
            onVerifyProject={handleVerifyProject}
            agentReport={agentReport}
            verification={verification}
            busy={busy}
            onVoiceStart={handleVoiceStart}
            onVoiceStop={handleVoiceStop}
            onSpeak={speakText}
            settings={voiceSettings}
            onSettingsChange={updateVoiceSettings}
            incomingTranscript={incomingTranscript}
            voiceError={voiceError}
            recording={recording}
          />
        )}
      </main>
    </div>
  );
}
