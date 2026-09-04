import { useState } from "react";
import type {
  AgentReport,
  Goal,
  Project,
  Snapshot,
  VerificationOutcome,
} from "../api";

const GOAL_STATUS_STYLES: Record<string, string> = {
  active: "bg-emerald-500/15 text-emerald-300 border-emerald-500/30",
  paused: "bg-amber-500/15 text-amber-300 border-amber-500/30",
  completed: "bg-sky-500/15 text-sky-300 border-sky-500/30",
  abandoned: "bg-zinc-500/15 text-zinc-400 border-zinc-500/30",
};

interface Props {
  project: Project;
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
}

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
    <div className="rounded-xl border border-zinc-800 bg-zinc-900/70 p-4">
      <div className="flex items-start justify-between gap-3">
        <p className="text-sm text-zinc-100">{goal.objective}</p>
        <span
          className={`shrink-0 rounded-full border px-2 py-0.5 text-[11px] ${
            GOAL_STATUS_STYLES[goal.status] ?? GOAL_STATUS_STYLES["abandoned"]
          }`}
        >
          {goal.status}
        </span>
      </div>
      <div className="mt-2 flex items-center gap-4 text-[11px] text-zinc-500">
        <span>要件: {goal.requirements.length}</span>
        <span>AC: {goal.acceptance_criteria.length}</span>
        <button
          onClick={() => void onRunAgent(goal.id)}
          disabled={running || busy !== null}
          className="ml-auto rounded-lg border border-violet-500/40 bg-violet-600/20 px-3 py-1 text-[11px] font-medium text-violet-200 hover:bg-violet-600/35 disabled:opacity-40"
        >
          {running ? "実行中…" : "Agent実行"}
        </button>
      </div>
    </div>
  );
}

function VerificationReport({ verification }: { verification: VerificationOutcome }) {
  return (
    <div
      className={`mt-3 rounded-xl border px-4 py-3 text-sm ${
        verification.passed
          ? "border-emerald-500/30 bg-emerald-500/10 text-emerald-200"
          : "border-red-500/30 bg-red-500/10 text-red-200"
      }`}
    >
      <div className="font-medium">{verification.passed ? "検証パス ✓" : "検証失敗 ✗"}</div>
      <div className="mt-1 text-xs opacity-80">{verification.summary}</div>
      <ul className="mt-2 space-y-1 text-xs">
        {verification.checks.map((c) => (
          <li key={c.name} className="flex items-start gap-2">
            <span className="shrink-0">
              {c.skipped ? "⏭" : c.passed ? "✅" : "❌"}
            </span>
            <span className={c.skipped ? "opacity-50" : ""}>
              {c.name} — {c.detail.slice(0, 240)}
            </span>
          </li>
        ))}
      </ul>
    </div>
  );
}

function AgentReportView({ report }: { report: AgentReport }) {
  return (
    <div className="mt-3 rounded-xl border border-zinc-700 bg-zinc-900/80 px-4 py-3 text-sm">
      <div className="flex items-center gap-3">
        <span
          className={`font-medium ${
            report.completed ? "text-emerald-300" : "text-amber-300"
          }`}
        >
          {report.completed ? "Goal達成 ✓" : "未達（要対応）"}
        </span>
        <span className="text-xs text-zinc-500">replan: {report.replans} 回</span>
      </div>
      <ul className="mt-2 space-y-1 text-xs">
        {report.steps.map((s, i) => (
          <li key={i} className="flex items-start gap-2">
            <span className="shrink-0">{s.success ? "✅" : "❌"}</span>
            <span className={s.success ? "text-zinc-300" : "text-red-300"}>
              {s.description} <span className="text-zinc-600">[{s.kind}]</span>
              <br />
              <span className="text-zinc-500">{s.detail.slice(0, 200)}</span>
            </span>
          </li>
        ))}
      </ul>
      {report.verification && <VerificationReport verification={report.verification} />}
    </div>
  );
}

export default function ProjectView({
  project,
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
}: Props) {
  const [objective, setObjective] = useState("");
  const [feedback, setFeedback] = useState("");
  const [message, setMessage] = useState("");

  const submitMessage = async () => {
    const text = message.trim();
    if (!text || sending) return;
    setMessage("");
    await onSendMessage(text);
  };

  const submitGoal = async () => {
    const text = objective.trim();
    if (!text) return;
    setObjective("");
    await onCreateGoal(text);
  };

  const submitFeedback = async () => {
    const text = feedback.trim();
    if (!text) return;
    setFeedback("");
    await onAddFeedback(text);
  };

  return (
    <div className="mx-auto max-w-3xl px-8 py-8">
      <h1 className="text-2xl font-semibold text-white">{project.name}</h1>
      <div className="mt-1 flex items-center gap-3 text-xs text-zinc-500">
        <span>Current state is a projection of the append-only event log.</span>
        <button
          onClick={() => void onVerifyProject()}
          disabled={busy !== null}
          className="ml-auto rounded-lg border border-zinc-700 px-3 py-1 text-[11px] text-zinc-300 hover:bg-zinc-800 disabled:opacity-40"
        >
          {busy === "verify" ? "検証中…" : "プロジェクト検証"}
        </button>
      </div>

      {verification && <VerificationReport verification={verification} />}
      {agentReport && <AgentReportView report={agentReport} />}

      {/* Chat (M3 text loop) */}
      <section className="mt-6 rounded-xl border border-zinc-800 bg-zinc-900/70">
        <div className="flex items-center justify-between border-b border-zinc-800 px-4 py-2">
          <h2 className="text-xs font-semibold uppercase tracking-widest text-zinc-500">
            Veyra Chat
          </h2>
          {sending && <span className="text-[11px] text-violet-300">思考中…</span>}
        </div>
        <div className="max-h-72 space-y-2 overflow-y-auto p-4">
          {snapshot?.messages.length === 0 && (
            <p className="text-sm text-zinc-500">
              「〜を作って」「ここ違う」「質問…」のように話しかけてください。
            </p>
          )}
          {snapshot?.messages.map((m, i) => (
            <div
              key={i}
              className={`rounded-lg px-3 py-2 text-sm ${
                m.role === "user"
                  ? "ml-8 bg-violet-600/20 text-violet-100"
                  : "mr-8 bg-zinc-800/70 text-zinc-200"
              }`}
            >
              {m.content}
            </div>
          ))}
        </div>
        <div className="flex gap-2 border-t border-zinc-800 p-3">
          <input
            value={message}
            onChange={(e) => setMessage(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && void submitMessage()}
            placeholder="Veyraに指示・質問…"
            className="flex-1 rounded-lg border border-zinc-700 bg-zinc-800/60 px-3 py-2 text-sm outline-none placeholder:text-zinc-500 focus:border-violet-500"
          />
          <button
            onClick={() => void submitMessage()}
            disabled={sending}
            className="rounded-lg bg-violet-600 px-4 py-2 text-sm font-medium text-white hover:bg-violet-500 disabled:opacity-50"
          >
            送信
          </button>
        </div>
      </section>

      {/* New goal */}
      <div className="mt-6 flex gap-2">
        <input
          value={objective}
          onChange={(e) => setObjective(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && void submitGoal()}
          placeholder="新しいGoal: 例) Build a Windows AI assistant."
          className="flex-1 rounded-lg border border-zinc-700 bg-zinc-800/60 px-3 py-2 text-sm outline-none placeholder:text-zinc-500 focus:border-violet-500"
        />
        <button
          onClick={() => void submitGoal()}
          className="rounded-lg bg-violet-600 px-4 py-2 text-sm font-medium text-white hover:bg-violet-500"
        >
          Goal作成
        </button>
      </div>

      {/* Goals */}
      <section className="mt-8">
        <h2 className="mb-3 text-xs font-semibold uppercase tracking-widest text-zinc-500">
          Goals
        </h2>
        {snapshot?.goals.length === 0 && (
          <p className="text-sm text-zinc-500">まだGoalはありません。</p>
        )}
        <div className="space-y-3">
          {snapshot?.goals.map((goal) => (
            <GoalCard
              key={goal.id}
              goal={goal}
              onRunAgent={onRunAgent}
              busy={busy}
            />
          ))}
        </div>
      </section>

      {/* Decisions */}
      <section className="mt-8">
        <h2 className="mb-3 text-xs font-semibold uppercase tracking-widest text-zinc-500">
          Decisions
        </h2>
        {snapshot?.decisions.length === 0 && (
          <p className="text-sm text-zinc-500">まだDecisionは記録されていません。</p>
        )}
        <div className="space-y-2">
          {snapshot?.decisions.map((d) => (
            <div
              key={d.id}
              className="rounded-xl border border-zinc-800 bg-zinc-900/70 px-4 py-3"
            >
              <p className="text-sm text-zinc-100">{d.decision}</p>
              {d.rationale && <p className="mt-1 text-xs text-zinc-500">{d.rationale}</p>}
            </div>
          ))}
        </div>
      </section>

      {/* Feedback */}
      <section className="mt-8">
        <h2 className="mb-3 text-xs font-semibold uppercase tracking-widest text-zinc-500">
          User Feedback
        </h2>
        {snapshot?.user_feedback.length === 0 && (
          <p className="text-sm text-zinc-500">まだFeedbackはありません。</p>
        )}
        <ul className="space-y-1">
          {snapshot?.user_feedback.map((f, i) => (
            <li key={i} className="rounded-lg bg-zinc-900/70 px-4 py-2 text-sm text-zinc-300">
              「{f}」
            </li>
          ))}
        </ul>
        <div className="mt-3 flex gap-2">
          <input
            value={feedback}
            onChange={(e) => setFeedback(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && void submitFeedback()}
            placeholder="「ここ違う」などのFeedbackを記録"
            className="flex-1 rounded-lg border border-zinc-700 bg-zinc-800/60 px-3 py-2 text-sm outline-none placeholder:text-zinc-500 focus:border-violet-500"
          />
          <button
            onClick={() => void submitFeedback()}
            className="rounded-lg border border-zinc-700 px-4 py-2 text-sm text-zinc-300 hover:bg-zinc-800"
          >
            記録
          </button>
        </div>
      </section>
    </div>
  );
}
