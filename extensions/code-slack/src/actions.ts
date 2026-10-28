// Policy-controlled OCP actions the Frontal Code agent may request. Each action validates its
// untrusted input before the runtime records it, then calls the Frontal Code control plane.
import { type ActionDefinition, action } from "ocp-sdk";
import { z } from "zod";
import type {
  FrontalCodeCreateTaskRequest,
  FrontalCodeCreateTaskResponse,
  FrontalCodeResolveApprovalRequest,
  FrontalCodeTask,
} from "./types";

export const CREATE_TASK_ACTION = "frontal-code.task.create";
export const RESOLVE_APPROVAL_ACTION = "frontal-code.task.approval.resolve";

export interface FrontalCodeActionsApi {
  createTask(request: FrontalCodeCreateTaskRequest): Promise<FrontalCodeCreateTaskResponse>;
  resolveTaskApproval(request: FrontalCodeResolveApprovalRequest): Promise<FrontalCodeTask>;
}

const createTaskInput = z.object({
  prompt: z.string().trim().min(1),
  user_id: z.string().min(1),
  channel_id: z.string().min(1),
  thread_ts: z.string().min(1).optional(),
  source: z.literal("slack"),
});

const resolveApprovalInput = z.object({
  taskId: z.string().min(1),
  approvalKind: z.enum(["orphaned_hosted_agent", "github_review_followup"]),
  action: z.enum(["cancel", "retry", "ack"]),
  resolvedBy: z.string().min(1),
  reason: z.string().min(1).optional(),
});

export type CreateTaskActionInput = z.infer<typeof createTaskInput>;
export type ResolveApprovalActionInput = z.infer<typeof resolveApprovalInput>;

export function createTaskAction(
  api: FrontalCodeActionsApi,
): ActionDefinition<CreateTaskActionInput, FrontalCodeCreateTaskResponse> {
  return action({
    type: CREATE_TASK_ACTION,
    validate: (input) => createTaskInput.parse(input),
    execute: (input) => api.createTask(input),
  });
}

export function resolveApprovalAction(
  api: FrontalCodeActionsApi,
): ActionDefinition<ResolveApprovalActionInput, FrontalCodeTask> {
  return action({
    type: RESOLVE_APPROVAL_ACTION,
    validate: (input) => resolveApprovalInput.parse(input),
    execute: (input) => api.resolveTaskApproval(input),
  });
}

export function frontalCodeActions(
  api: FrontalCodeActionsApi,
): ActionDefinition<unknown, unknown>[] {
  return [createTaskAction(api), resolveApprovalAction(api)] as ActionDefinition<
    unknown,
    unknown
  >[];
}
