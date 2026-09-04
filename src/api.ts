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

// --- Typed wrappers over Tauri IPC commands ---

export const api = {
  listProjects: () => invoke<Project[]>("list_projects"),

  createProject: (name: string, rootPath: string | null) =>
    invoke<Project>("create_project", { name, rootPath }),

  projectSnapshot: (projectId: string) =>
    invoke<Snapshot>("project_snapshot", { projectId }),

  createGoal: (projectId: string, objective: string) =>
    invoke<Goal>("create_goal", { projectId, objective }),

  addUserFeedback: (projectId: string, content: string) =>
    invoke<void>("add_user_feedback", { projectId, content }),

  recordDecision: (projectId: string, decision: string, rationale: string | null) =>
    invoke<Decision>("record_decision", { projectId, decision, rationale }),

  sendMessage: (projectId: string, text: string) =>
    invoke<LoopOutcome>("send_message", { projectId, text }),

  runAgent: (goalId: string) => invoke<AgentReport>("run_agent", { goalId }),

  verifyProject: (projectId: string) =>
    invoke<VerificationOutcome>("verify_project", { projectId }),
};
