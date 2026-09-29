import { useEffect, useState } from "react";
import {
  api,
  type AgentReport,
  type AssistantReport,
  type Goal,
  type Project,
  type Snapshot,
  type ToolManifest,
  type VerificationOutcome,
  type VoiceSettings,
} from "../api";

const GOAL_STATUS_STYLES: Record<string, string> = {
  active: "bg-emerald-500/15 text-emerald-300 border-emerald-500/30",
  paused: "bg-amber-500/15 text-amber-300 border-amber-500/30",
  completed: "bg-sky-500/15 text-sky-300 border-sky-500/30",
  abandoned: "bg-zinc-500/15 text-zinc-400 border-zinc-500/30",
};

import { type Locale } from "../i18n";

interface Props {
  projects: Project[];
  selectedProject: Project | null;
  onSelectProject: (id: string) => void;
  onCreateProject: (name: string) => Promise<void>;
  snapshot: Snapshot | null;
  onCreateGoal: (objective: string) => Promise<void>;
  onAddFeedback: (content: string) => Promise<void>;
  onSendMessage: (text: string) => Promise<void>;
  sending: boolean;
  onRunAgent: (goalId: string) => Promise<void>;
  onVerifyProject: () => Promise<void>;
  agentReport: AgentReport | null;
  verification: VerificationOutcome | null;
  busy: string | null;
  settings: VoiceSettings;
  onSettingsChange: (s: VoiceSettings) => void;
  onClose: () => void;
  locale?: Locale;
  onLocaleChange?: (loc: Locale) => void;
}

type TabType = "goals" | "assistant" | "verification" | "voice";

function GoalCard({
  goal,
  onRunAgent,
  busy,
}: {
  goal: Goal;
  onRunAgent: (goalId: string) => Promise<void>;
  busy: string | null;
}) {
  const running = busy === goal.id;
  return (
    <div className="rounded-xl border border-amber-500/20 bg-black/40 hover:border-amber-500/40 transition-all p-3.5 shadow-md">
      <div className="flex items-start justify-between gap-3">
        <div>
          <p className="text-sm font-medium text-amber-50">{goal.objective}</p>
          <div className="mt-1 flex items-center gap-2 text-[10px] text-zinc-400">
            <span>要件: {goal.requirements.length} 件</span>
            <span>·</span>
            <span>AC: {goal.acceptance_criteria.length} 件</span>
          </div>
        </div>
        <div className="flex items-center gap-2 shrink-0">
          <span
            className={`rounded-full border px-2 py-0.5 text-[10px] font-medium ${
              GOAL_STATUS_STYLES[goal.status] ?? GOAL_STATUS_STYLES["abandoned"]
            }`}
          >
            {goal.status}
          </span>
          <button
            onClick={() => void onRunAgent(goal.id)}
            disabled={running || busy !== null}
            className="rounded-lg border border-amber-500/40 bg-gradient-to-r from-amber-500/25 to-amber-600/25 px-3 py-1 text-[11px] font-semibold text-amber-200 hover:from-amber-500/40 hover:to-amber-600/40 disabled:opacity-40 transition-all cursor-pointer shadow-sm active:scale-95"
          >
            {running ? "Agent実行中…" : "Agent実行"}
          </button>
        </div>
      </div>

      {goal.acceptance_criteria.length > 0 && (
        <div className="mt-2.5 pt-2 border-t border-amber-500/10 space-y-1">
          <span className="text-[10px] font-mono uppercase tracking-wider text-zinc-500">
            Acceptance Criteria:
          </span>
          <ul className="space-y-0.5 text-[11px] text-zinc-300">
            {goal.acceptance_criteria.map((ac) => (
              <li key={ac.id} className="flex items-center gap-1.5 pl-1">
                <span className="text-amber-400/80 text-[10px]">◇</span>
                <span>{ac.content}</span>
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}

function VerificationReport({ verification }: { verification: VerificationOutcome }) {
  return (
    <div
      className={`rounded-xl border p-3.5 text-sm shadow-lg ${
        verification.passed
          ? "border-emerald-500/30 bg-emerald-950/20 text-emerald-200"
          : "border-rose-500/30 bg-rose-950/20 text-rose-200"
      }`}
    >
      <div className="font-semibold text-xs flex items-center justify-between">
        <span className="flex items-center gap-1.5">
          <span>{verification.passed ? "✅ 検証パス" : "❌ 検証失敗"}</span>
          <span className="opacity-80">({verification.checks.length} チェック)</span>
        </span>
        <span className="text-[10px] font-mono opacity-60">Plan §19 Verification</span>
      </div>
      <div className="mt-1 text-xs opacity-90">{verification.summary}</div>
      <ul className="mt-2.5 space-y-1.5 text-xs">
        {verification.checks.map((c) => (
          <li key={c.name} className="flex items-start gap-2 bg-black/30 rounded-lg px-2.5 py-1 border border-white/5">
            <span className="shrink-0 text-xs">
              {c.skipped ? "⏭" : c.passed ? "✓" : "✗"}
            </span>
            <span className={c.skipped ? "opacity-50" : ""}>
              <strong className="font-mono text-zinc-200">{c.name}</strong> — {c.detail.slice(0, 240)}
            </span>
          </li>
        ))}
      </ul>
    </div>
  );
}

function AgentReportView({ report }: { report: AgentReport }) {
  return (
    <div className="rounded-xl border border-amber-500/30 bg-black/40 p-3.5 text-sm shadow-lg">
      <div className="flex items-center justify-between pb-2 border-b border-amber-500/15">
        <div className="flex items-center gap-2">
          <span
            className={`font-semibold text-xs ${
              report.completed ? "text-emerald-300" : "text-amber-300"
            }`}
          >
            {report.completed ? "Goal達成 ✓ (Agent Finished)" : "未達・要対応"}
          </span>
          <span className="text-[10px] text-zinc-400 font-mono">replan: {report.replans} 回</span>
        </div>
        <span className="text-[10px] text-zinc-500 font-mono">ステップ数: {report.steps.length}</span>
      </div>
      <ul className="mt-2.5 space-y-1.5 text-xs">
        {report.steps.map((s, i) => (
          <li key={i} className="flex items-start gap-2 bg-white/5 rounded-lg p-2 border border-white/5">
            <span className="shrink-0">{s.success ? "✅" : "❌"}</span>
            <div className="flex-1">
              <div className="flex items-center justify-between">
                <span className={s.success ? "text-zinc-200" : "text-rose-300"}>
                  {s.description}
                </span>
                <span className="text-amber-400 font-mono text-[10px]">[{s.kind}]</span>
              </div>
              <span className="text-zinc-500 text-[11px] block mt-0.5">{s.detail.slice(0, 200)}</span>
            </div>
          </li>
        ))}
      </ul>
      {report.verification && (
        <div className="mt-3">
          <VerificationReport verification={report.verification} />
        </div>
      )}
    </div>
  );
}

export default function ProjectView({
  projects,
  selectedProject,
  onSelectProject,
  onCreateProject,
  snapshot,
  onCreateGoal,
  onAddFeedback,
  onSendMessage,
  sending,
  onRunAgent,
  onVerifyProject,
  agentReport,
  verification,
  busy,
  settings,
  onSettingsChange,
  onClose,
  locale = "ja",
  onLocaleChange,
}: Props) {
  const [tab, setTab] = useState<TabType>("goals");
  const [newGoalObjective, setNewGoalObjective] = useState("");
  const [newFeedbackText, setNewFeedbackText] = useState("");
  const [newProjectName, setNewProjectName] = useState("");
  const [showNewProjectInput, setShowNewProjectInput] = useState(false);
  const [tools, setTools] = useState<ToolManifest[]>([]);
  const [assistantInput, setAssistantInput] = useState("");
  const [assistantReport, setAssistantReport] = useState<AssistantReport | null>(null);
  const [runningAssistant, setRunningAssistant] = useState(false);
  const [chatInput, setChatInput] = useState("");

  useEffect(() => {
    void api.listAvailableTools().then(setTools).catch(() => {});
  }, []);

  const handleSendMessageSubmit = async () => {
    const text = chatInput.trim();
    if (!text || sending) return;
    setChatInput("");
    await onSendMessage(text);
  };

  const handleCreateGoalSubmit = async () => {
    const text = newGoalObjective.trim();
    if (!text) return;
    setNewGoalObjective("");
    await onCreateGoal(text);
  };

  const handleAddFeedbackSubmit = async () => {
    const text = newFeedbackText.trim();
    if (!text) return;
    setNewFeedbackText("");
    await onAddFeedback(text);
  };

  const handleCreateProjectSubmit = async () => {
    const text = newProjectName.trim();
    if (!text) return;
    setNewProjectName("");
    setShowNewProjectInput(false);
    await onCreateProject(text);
  };

  const handleRunAssistantDirect = async (cmd: string) => {
    const text = cmd.trim();
    if (!text || runningAssistant) return;
    setRunningAssistant(true);
    setAssistantInput("");
    try {
      const rep = await api.runAssistantTask(text, selectedProject?.id);
      setAssistantReport(rep);
    } catch (e) {
      setAssistantReport({
        instruction: text,
        response: `エラー: ${String(e)}`,
        steps: [],
        success: false,
      });
    } finally {
      setRunningAssistant(false);
    }
  };

  return (
    <div className="luxury-card flex flex-col max-h-[72vh] overflow-hidden no-drag text-zinc-100 shadow-2xl border border-amber-500/25">
      {/* ------------------------------------------------------------- */}
      {/* Top Header: Project Selector & Close Button                   */}
      {/* ------------------------------------------------------------- */}
      <div className="flex items-center justify-between px-4 py-3 border-b border-amber-500/15 bg-black/40">
        <div className="flex items-center gap-2.5">
          <span className="text-amber-400 text-sm">📊</span>
          <span className="text-xs font-semibold uppercase tracking-wider text-amber-200">
            Project Dashboard
          </span>

          {/* Project Dropdown / Switcher */}
          {projects.length > 0 && (
            <select
              value={selectedProject?.id ?? ""}
              onChange={(e) => onSelectProject(e.target.value)}
              className="bg-zinc-900 border border-amber-500/25 text-amber-100 text-xs rounded-lg px-2 py-1 outline-none cursor-pointer hover:border-amber-400 transition-colors"
            >
              {projects.map((p) => (
                <option key={p.id} value={p.id} className="bg-zinc-900 text-zinc-100">
                  {p.name}
                </option>
              ))}
            </select>
          )}

          <button
            onClick={() => setShowNewProjectInput((v) => !v)}
            className="text-[11px] text-amber-400/80 hover:text-amber-300 px-1.5 py-0.5 rounded border border-amber-500/20 hover:border-amber-500/40 transition-colors"
            title={locale === "en" ? "Create New Project" : "新規プロジェクト作成"}
          >
            {locale === "en" ? "+ New" : "+ 新規"}
          </button>
        </div>

        <button
          onClick={onClose}
          className="flex h-6 w-6 items-center justify-center rounded-full text-zinc-500 hover:text-amber-200 hover:bg-white/10 transition-colors cursor-pointer text-xs"
          title={locale === "en" ? "Close" : "閉じる"}
        >
          ✕
        </button>
      </div>

      {/* Inline Create Project Input */}
      {showNewProjectInput && (
        <div className="flex items-center gap-2 px-4 py-2 bg-amber-500/10 border-b border-amber-500/20">
          <input
            value={newProjectName}
            onChange={(e) => setNewProjectName(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && void handleCreateProjectSubmit()}
            placeholder={locale === "en" ? "Enter new project name…" : "新規プロジェクト名を入力…"}
            className="flex-1 bg-black/50 border border-amber-500/30 rounded-lg px-3 py-1 text-xs text-amber-100 outline-none"
            autoFocus
          />
          <button
            onClick={() => void handleCreateProjectSubmit()}
            className="bg-amber-500 hover:bg-amber-400 text-zinc-950 font-semibold px-3 py-1 rounded-lg text-xs transition-colors"
          >
            {locale === "en" ? "Create" : "作成"}
          </button>
        </div>
      )}

      {/* ------------------------------------------------------------- */}
      {/* Navigation Tabs                                               */}
      {/* ------------------------------------------------------------- */}
      <div className="flex items-center gap-1 px-4 py-2 border-b border-amber-500/10 bg-black/20 text-xs">
        {[
          { key: "goals", label: "Goals & Agent", icon: "🎯" },
          { key: "assistant", label: "Tools & Automation", icon: "⚡" },
          { key: "verification", label: "Verification", icon: "📋" },
          { key: "voice", label: "Voice & Settings", icon: "⚙️" },
        ].map(({ key, label, icon }) => (
          <button
            key={key}
            onClick={() => setTab(key as TabType)}
            className={`flex items-center gap-1.5 px-3 py-1.5 rounded-lg font-medium transition-all cursor-pointer ${
              tab === key
                ? "bg-amber-500/20 text-amber-200 border border-amber-500/30 shadow-sm"
                : "text-zinc-400 hover:text-zinc-200 hover:bg-white/5"
            }`}
          >
            <span>{icon}</span>
            <span>{label}</span>
          </button>
        ))}
      </div>

      {/* ------------------------------------------------------------- */}
      {/* Tab Contents (Scrollable Area)                                */}
      {/* ------------------------------------------------------------- */}
      <div className="flex-1 overflow-y-auto p-4 space-y-4">
        {/* Tab 1: Goals & Agent Execution */}
        {tab === "goals" && (
          <div className="space-y-4">
            {/* New Goal Input */}
            <div className="flex gap-2">
              <input
                value={newGoalObjective}
                onChange={(e) => setNewGoalObjective(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && void handleCreateGoalSubmit()}
                placeholder="新しいGoalを作成: 例) Next.js のランディングページを構築する"
                className="flex-1 bg-black/40 border border-amber-500/25 rounded-xl px-3.5 py-2 text-xs text-amber-50 placeholder:text-zinc-500 outline-none focus:border-amber-400"
              />
              <button
                onClick={() => void handleCreateGoalSubmit()}
                className="bg-gradient-to-r from-amber-500 to-amber-600 hover:from-amber-400 hover:to-amber-500 text-zinc-950 font-semibold px-4 py-2 rounded-xl text-xs transition-all shadow-md active:scale-95"
              >
                Goal作成
              </button>
            </div>

            {/* Goals List */}
            <div className="space-y-2.5">
              <div className="flex items-center justify-between text-xs text-zinc-400">
                <span className="font-semibold uppercase tracking-wider text-amber-300/80">
                  Goals ({snapshot?.goals.length ?? 0})
                </span>
                <span className="text-[11px] text-zinc-500">Autonomous Execution Engine</span>
              </div>

              {snapshot?.goals.length === 0 ? (
                <div className="rounded-xl border border-white/5 bg-black/20 p-6 text-center text-xs text-zinc-500">
                  現在設定されているGoalはありません。上の入力欄からGoalを作成してください。
                </div>
              ) : (
                snapshot?.goals.map((g) => (
                  <GoalCard
                    key={g.id}
                    goal={g}
                    onRunAgent={onRunAgent}
                    busy={busy}
                  />
                ))
              )}
            </div>

            {/* Latest Agent Report */}
            {agentReport && (
              <div className="pt-2">
                <div className="text-xs font-semibold uppercase tracking-wider text-amber-300 mb-2">
                  直近のAgent実行レポート
                </div>
                <AgentReportView report={agentReport} />
              </div>
            )}

            {/* Direct Project Chat & Instruction */}
            <div className="pt-2 border-t border-amber-500/10 space-y-2">
              <div className="flex items-center justify-between text-xs text-zinc-400">
                <span className="font-semibold uppercase tracking-wider text-amber-300/80">
                  Project Chat & Guidance
                </span>
                {sending && <span className="text-amber-400 animate-pulse text-[11px]">思考中…</span>}
              </div>

              {snapshot?.messages && snapshot.messages.length > 0 && (
                <div className="max-h-36 overflow-y-auto space-y-1.5 p-2 rounded-xl bg-black/30 border border-white/5 text-xs">
                  {snapshot.messages.slice(-6).map((m, idx) => (
                    <div
                      key={idx}
                      className={`p-2 rounded-lg ${
                        m.role === "user"
                          ? "bg-amber-500/15 text-amber-100 ml-4"
                          : "bg-white/5 text-zinc-300 mr-4"
                      }`}
                    >
                      <span className="text-[10px] text-zinc-500 block">{m.role === "user" ? "You" : "Veyra"}</span>
                      <p className="mt-0.5 leading-relaxed">{m.content}</p>
                    </div>
                  ))}
                </div>
              )}

              <div className="flex gap-2">
                <input
                  value={chatInput}
                  onChange={(e) => setChatInput(e.target.value)}
                  onKeyDown={(e) => e.key === "Enter" && void handleSendMessageSubmit()}
                  placeholder="プロジェクトに関する指示や質問を入力…"
                  className="flex-1 bg-black/40 border border-amber-500/20 rounded-xl px-3 py-1.5 text-xs text-amber-50 outline-none"
                />
                <button
                  onClick={() => void handleSendMessageSubmit()}
                  disabled={sending}
                  className="bg-amber-500/25 border border-amber-500/40 text-amber-200 px-3 py-1.5 rounded-xl text-xs font-semibold hover:bg-amber-500/40 transition-colors disabled:opacity-40"
                >
                  {sending ? "送信中…" : "送信"}
                </button>
              </div>
            </div>
          </div>
        )}

        {/* Tab 2: Tools & Automation */}
        {tab === "assistant" && (
          <div className="space-y-4">
            {/* Quick Task Input */}
            <div className="rounded-xl border border-amber-500/25 bg-black/30 p-3.5">
              <div className="text-xs font-semibold text-amber-300 uppercase tracking-wider mb-2">
                Windows Desktop Task Automation (M6)
              </div>
              <div className="flex gap-2">
                <input
                  value={assistantInput}
                  onChange={(e) => setAssistantInput(e.target.value)}
                  onKeyDown={(e) => e.key === "Enter" && void handleRunAssistantDirect(assistantInput)}
                  placeholder="例: 「YouTube開いて」「アクティブなウィンドウを調べて」「メモ帳を開いて」"
                  className="flex-1 bg-black/50 border border-amber-500/20 rounded-xl px-3 py-2 text-xs text-amber-50 placeholder:text-zinc-500 outline-none focus:border-amber-400"
                />
                <button
                  onClick={() => void handleRunAssistantDirect(assistantInput)}
                  disabled={runningAssistant}
                  className="bg-amber-500 hover:bg-amber-400 text-zinc-950 font-semibold px-4 py-2 rounded-xl text-xs transition-all disabled:opacity-40"
                >
                  {runningAssistant ? "実行中…" : "実行"}
                </button>
              </div>

              {/* Preset buttons */}
              <div className="mt-3 flex flex-wrap gap-1.5 text-[11px]">
                <span className="text-zinc-500 py-0.5">クイック実行:</span>
                {[
                  { label: "YouTube起動", cmd: "YouTubeを開いて" },
                  { label: "ウィンドウ確認", cmd: "アクティブなウィンドウを調べて" },
                  { label: "メモ帳起動", cmd: "メモ帳を開いて (notepad)" },
                  { label: "Agent進捗確認", cmd: "Agentがゴールに向けて動いているか確認して" },
                ].map(({ label, cmd }) => (
                  <button
                    key={label}
                    onClick={() => void handleRunAssistantDirect(cmd)}
                    className="rounded-lg border border-amber-500/20 bg-white/5 px-2.5 py-1 text-zinc-300 hover:text-amber-200 hover:bg-amber-500/10 transition-colors"
                  >
                    {label}
                  </button>
                ))}
              </div>
            </div>

            {/* Assistant execution result */}
            {assistantReport && (
              <div className="rounded-xl border border-amber-500/25 bg-black/40 p-3.5 text-xs">
                <div className="flex items-center justify-between pb-2 border-b border-amber-500/15">
                  <span className={`font-semibold ${assistantReport.success ? "text-emerald-400" : "text-rose-400"}`}>
                    {assistantReport.success ? "✓ 実行成功" : "✕ 失敗"}
                  </span>
                  <span className="text-[10px] text-zinc-500 font-mono">
                    ステップ: {assistantReport.steps.length}
                  </span>
                </div>
                <p className="mt-2 text-amber-100 font-medium whitespace-pre-wrap leading-relaxed">
                  {assistantReport.response}
                </p>
                {assistantReport.steps.length > 0 && (
                  <div className="mt-2.5 pt-2 border-t border-white/5 space-y-1">
                    {assistantReport.steps.map((s, idx) => (
                      <div key={idx} className="flex items-center justify-between bg-black/40 px-2 py-1 rounded text-[10px]">
                        <span className="font-mono text-amber-300">{s.tool_name ?? "action"}</span>
                        <span className="text-zinc-400 truncate max-w-[280px]">{s.detail}</span>
                      </div>
                    ))}
                  </div>
                )}
              </div>
            )}

            {/* Available tools grid */}
            <div>
              <div className="text-xs font-semibold text-zinc-400 uppercase tracking-wider mb-2 flex items-center justify-between">
                <span>利用可能な登録ツール ({tools.length})</span>
                <span className="text-[10px] text-amber-400/80 font-mono">Multi-Step Execution Ready</span>
              </div>
              <div className="grid grid-cols-2 gap-2">
                {tools.map((t) => (
                  <div
                    key={t.name}
                    className="rounded-lg border border-white/5 bg-black/30 p-2.5 hover:border-amber-500/30 transition-colors"
                  >
                    <div className="flex items-center justify-between">
                      <span className="font-mono text-xs font-medium text-amber-300">{t.name}</span>
                      <span className="text-[9px] px-1.5 py-0.5 rounded bg-amber-500/10 text-amber-400 border border-amber-500/20 font-mono">
                        {t.permission}
                      </span>
                    </div>
                    <p className="mt-1 text-[11px] text-zinc-400 line-clamp-2 leading-relaxed">
                      {t.description}
                    </p>
                  </div>
                ))}
              </div>
            </div>
          </div>
        )}

        {/* Tab 3: Verification & Decisions */}
        {tab === "verification" && (
          <div className="space-y-4">
            {/* Run Verification Button */}
            <div className="flex items-center justify-between rounded-xl border border-amber-500/20 bg-black/30 p-3.5">
              <div>
                <div className="text-xs font-semibold text-amber-200">
                  プロジェクト整合性・品質検証 (Plan §19)
                </div>
                <div className="text-[11px] text-zinc-400 mt-0.5">
                  ビルド、テスト、AC準拠、デッドロックを自動検証します。
                </div>
              </div>
              <button
                onClick={() => void onVerifyProject()}
                disabled={busy !== null}
                className="bg-amber-500/25 border border-amber-500/40 hover:bg-amber-500/40 text-amber-200 font-semibold px-4 py-1.5 rounded-lg text-xs transition-all disabled:opacity-40"
              >
                {busy === "verify" ? "検証実行中…" : "今すぐ検証"}
              </button>
            </div>

            {/* Verification Result */}
            {verification && <VerificationReport verification={verification} />}

            {/* Recorded Decisions */}
            <div>
              <div className="text-xs font-semibold text-zinc-400 uppercase tracking-wider mb-2">
                Decisions ({snapshot?.decisions.length ?? 0})
              </div>
              {snapshot?.decisions.length === 0 ? (
                <div className="rounded-xl border border-white/5 bg-black/20 p-4 text-center text-xs text-zinc-500">
                  記録された決定事項はありません。
                </div>
              ) : (
                <div className="space-y-2">
                  {snapshot?.decisions.map((d) => (
                    <div key={d.id} className="rounded-xl border border-white/5 bg-black/30 p-3">
                      <p className="text-xs font-medium text-amber-100">{d.decision}</p>
                      {d.rationale && (
                        <p className="text-[11px] text-zinc-400 mt-1">{d.rationale}</p>
                      )}
                    </div>
                  ))}
                </div>
              )}
            </div>

            {/* User Feedback */}
            <div>
              <div className="text-xs font-semibold text-zinc-400 uppercase tracking-wider mb-2">
                User Feedback ({snapshot?.user_feedback.length ?? 0})
              </div>
              <div className="flex gap-2 mb-2">
                <input
                  value={newFeedbackText}
                  onChange={(e) => setNewFeedbackText(e.target.value)}
                  onKeyDown={(e) => e.key === "Enter" && void handleAddFeedbackSubmit()}
                  placeholder="フィードバックを追加 (例: ボタンの位置を右に寄せて)"
                  className="flex-1 bg-black/40 border border-amber-500/20 rounded-xl px-3 py-1.5 text-xs text-amber-50 outline-none"
                />
                <button
                  onClick={() => void handleAddFeedbackSubmit()}
                  className="bg-white/10 hover:bg-white/15 text-zinc-200 px-3 py-1.5 rounded-xl text-xs transition-colors"
                >
                  記録
                </button>
              </div>
              <ul className="space-y-1.5">
                {snapshot?.user_feedback.map((f, i) => (
                  <li key={i} className="rounded-lg bg-black/30 border border-white/5 px-3 py-2 text-xs text-zinc-300">
                    「{f}」
                  </li>
                ))}
              </ul>
            </div>
          </div>
        )}

        {/* Tab 4: Voice & System Settings */}
        {tab === "voice" && (
          <div className="space-y-4 text-xs">
            <div className="rounded-xl border border-amber-500/20 bg-black/30 p-4 space-y-3">
              <div className="text-xs font-semibold text-amber-300 uppercase tracking-wider pb-2 border-b border-white/5">
                音声合成・認識エンジン設定 (Plan §7-8)
              </div>

              <div className="grid grid-cols-2 gap-3">
                <label className="flex flex-col gap-1">
                  <span className="text-zinc-400">Style-Bert-VITS2 サーバーURL</span>
                  <input
                    value={settings.sbv2Url}
                    onChange={(e) => onSettingsChange({ ...settings, sbv2Url: e.target.value })}
                    className="bg-black/50 border border-amber-500/20 rounded-lg px-2.5 py-1.5 text-xs text-amber-50 outline-none focus:border-amber-400"
                  />
                </label>

                <label className="flex flex-col gap-1">
                  <span className="text-zinc-400">TTS モデル識別子</span>
                  <input
                    value={settings.ttsModel}
                    onChange={(e) => onSettingsChange({ ...settings, ttsModel: e.target.value })}
                    placeholder="jvnv-M1-jp など"
                    className="bg-black/50 border border-amber-500/20 rounded-lg px-2.5 py-1.5 text-xs text-amber-50 outline-none focus:border-amber-400"
                  />
                </label>

                <label className="flex flex-col gap-1">
                  <span className="text-zinc-400">Whisper モデル (ローカルSTT)</span>
                  <select
                    value={settings.whisperModel}
                    onChange={(e) => onSettingsChange({ ...settings, whisperModel: e.target.value })}
                    className="bg-black/50 border border-amber-500/20 rounded-lg px-2.5 py-1.5 text-xs text-amber-50 outline-none cursor-pointer"
                  >
                    <option value="tiny">tiny (推奨・超高速 75MB)</option>
                    <option value="base">base (軽量 145MB)</option>
                    <option value="small">small (標準 466MB)</option>
                    <option value="medium">medium (高精度 1.5GB)</option>
                  </select>
                </label>

                <label className="flex flex-col gap-1">
                  <span className="text-zinc-400">{locale === "en" ? "UI Language" : "UI 表示言語 (Language)"}</span>
                  <select
                    value={locale}
                    onChange={(e) => onLocaleChange?.(e.target.value as Locale)}
                    className="bg-black/50 border border-amber-500/20 rounded-lg px-2.5 py-1.5 text-xs text-amber-50 outline-none cursor-pointer"
                  >
                    <option value="ja">日本語 (Japanese)</option>
                    <option value="en">English (英語)</option>
                  </select>
                </label>

                <div className="flex items-center pt-5">
                  <label className="flex items-center gap-2 cursor-pointer">
                    <input
                      type="checkbox"
                      checked={settings.autoSpeak}
                      onChange={(e) => onSettingsChange({ ...settings, autoSpeak: e.target.checked })}
                      className="accent-amber-500 w-4 h-4 cursor-pointer"
                    />
                    <span className="text-zinc-300 font-medium">回答を自動音声読み上げ</span>
                  </label>
                </div>
              </div>
            </div>

            <div className="rounded-xl border border-white/5 bg-black/20 p-3 text-[11px] text-zinc-500 space-y-1">
              <div>💡 <strong>Smart Tiering:</strong> コーディング・検証は <code>qwen2.5-coder:7b</code>、日常会話・意図判定は <code>qwen2.5:1.5b</code> で自動振り分けされます。</div>
              <div>💡 <strong>Context Packaging:</strong> アクティブなGoalや直近のWindows実行履歴が自動的にAIプロンプトに統合されます。</div>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
