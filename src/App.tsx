import { useCallback, useEffect, useState } from "react";
import { api, type Project, type Snapshot } from "./api";
import ProjectView from "./components/ProjectView";

export default function App() {
  const [projects, setProjects] = useState<Project[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [newProjectName, setNewProjectName] = useState("");
  const [error, setError] = useState<string | null>(null);

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
          />
        )}
      </main>
    </div>
  );
}
