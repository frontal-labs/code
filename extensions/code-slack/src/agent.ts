// The Frontal Code OCP agent. Every Slack request reaches it as a `message.received` event:
// - plain messages are task prompts and become a `frontal-code.task.create` action;
// - messages whose metadata `kind` is APPROVAL_DECISION_KIND are approval button clicks and become a
//   `frontal-code.task.approval.resolve` action.
// Both go through the OCP policy engine and are recorded in the event store.
import type { ActionResult, AgentContext, AgentDefinition } from "ocp-sdk";
import { stripBotMention } from "./access";
import { CREATE_TASK_ACTION, type CreateTaskActionInput, RESOLVE_APPROVAL_ACTION } from "./actions";
import { DEFAULT_AGENT_ID, slackUserIdOf } from "./actors";
import type {
  FrontalCodeCreateTaskResponse,
  FrontalCodeTask,
  FrontalCodeTrackedTask,
} from "./types";

export const APPROVAL_DECISION_KIND = "frontal-code.approval.decision";

export type AgentOutcome =
  | { kind: "task"; result: ActionResult<FrontalCodeCreateTaskResponse> }
  | { kind: "approval"; result: ActionResult<FrontalCodeTask> }
  | { kind: "ignored"; reason: string };

export interface FrontalCodeAgentOptions {
  id?: string;
  // Called after the control plane accepts a task, to post the confirmation and track the task.
  onTaskCreated(
    task: FrontalCodeCreateTaskResponse,
    routing: FrontalCodeTrackedTask,
    prompt: string,
  ): Promise<void>;
  // Receives the outcome for each handled event, keyed by the inbound event ID.
  onOutcome?(eventId: string, outcome: AgentOutcome): void;
}

export function createFrontalCodeAgent(options: FrontalCodeAgentOptions): AgentDefinition {
  return {
    id: options.id ?? DEFAULT_AGENT_ID,
    name: "Frontal Code",
    description: "Starts Frontal Code tasks from Slack and resolves their approvals.",
    version: "1.0.0",
    actorCapabilities: [CREATE_TASK_ACTION, RESOLVE_APPROVAL_ACTION],
    skills: [
      {
        id: "frontal-code.task.create",
        name: "Start a coding task",
        description: "Turns a Slack direct message, @mention, or /code command into a task.",
        examples: ["@Code fix the flaky integration test in api/"],
        tags: ["slack", "tasks"],
      },
      {
        id: "frontal-code.task.approval.resolve",
        name: "Resolve task approvals",
        description: "Retries, cancels, or acknowledges a task waiting on a human decision.",
        tags: ["slack", "approvals"],
      },
    ],
    onMessage: async (ctx) => {
      const outcome =
        ctx.message.metadata.kind === APPROVAL_DECISION_KIND
          ? await handleApprovalDecision(ctx)
          : await handleTaskPrompt(ctx, options);
      options.onOutcome?.(ctx.event.eventId, outcome);
    },
  };
}

async function handleTaskPrompt(
  ctx: AgentContext,
  options: FrontalCodeAgentOptions,
): Promise<AgentOutcome> {
  const userId = slackUserIdOf(ctx.event.actor ?? ctx.event.source);
  const channelId = ctx.channel?.channelId;
  const prompt = stripBotMention(ctx.message.text);
  if (!(userId && channelId && prompt)) {
    return { kind: "ignored", reason: "message has no Slack user, channel, or prompt" };
  }

  // Protobuf strings default to "": an empty thread means a top-level message (e.g. /code).
  const threadTs = ctx.channel?.threadId || undefined;
  const input: CreateTaskActionInput = {
    prompt,
    user_id: userId,
    channel_id: channelId,
    ...(threadTs ? { thread_ts: threadTs } : {}),
    source: "slack",
  };
  const result = await ctx.request<CreateTaskActionInput, FrontalCodeCreateTaskResponse>(
    CREATE_TASK_ACTION,
    input,
    { actionId: `task-create:${ctx.event.eventId}`, target: `slack:${channelId}` },
  );

  switch (result.status) {
    case "executed":
      await options.onTaskCreated(
        result.result,
        { taskId: result.result.task_id, channelId, threadTs, userId },
        prompt,
      );
      break;
    case "blocked":
      ctx.logger.warn("Task creation blocked by OCP policy", { reason: result.reason, userId });
      await ctx.reply(`Code could not start this task: ${result.reason}`);
      break;
    case "failed":
      ctx.logger.error("Task creation failed", { error: result.error.message, userId });
      await ctx.reply(`Code could not start this task: ${result.error.message}`);
      break;
    case "approval_required":
      await ctx.reply("This task is waiting for approval before it starts.");
      break;
  }
  return { kind: "task", result };
}

async function handleApprovalDecision(ctx: AgentContext): Promise<AgentOutcome> {
  const metadata = ctx.message.metadata;
  const input = {
    taskId: metadata.taskId,
    approvalKind: metadata.approvalKind,
    action: metadata.action,
    resolvedBy: metadata.resolvedBy,
    ...(metadata.reason ? { reason: metadata.reason } : {}),
  };
  // Input is validated by the action definition; a malformed decision fails the action.
  const result = await ctx.request<unknown, FrontalCodeTask>(RESOLVE_APPROVAL_ACTION, input, {
    actionId: `approval:${ctx.event.eventId}`,
    target: `task:${input.taskId ?? "unknown"}`,
    reason: `Slack approval decision by ${input.resolvedBy ?? "unknown"}`,
  });
  if (result.status === "failed") {
    ctx.logger.warn("Approval resolution failed", {
      taskId: input.taskId,
      error: result.error.message,
    });
  }
  return { kind: "approval", result };
}
