// OcpBridge composes the OpenChannel Protocol host for the Slack bot: the Frontal Code agent, its
// policy-controlled actions, the `ocp-sdk/slack` channel, and the Postgres event store. SlackInterface
// talks to OCP only through this class, so every Slack request and every bot message is a typed OCP
// event recorded in the store.
import { randomUUID } from "node:crypto";
import {
  type ActorRef,
  type AgentLogger,
  type ChannelRef,
  type ConversationRef,
  type EventEnvelope,
  messageReceived,
  messageSent,
  type OpenChannel,
  openChannel,
} from "ocp-sdk";
import { slackChannel } from "ocp-sdk/slack";
import { type FrontalCodeActionsApi, frontalCodeActions } from "./actions";
import { frontalCodeAgentActor, SLACK_PROVIDER, slackUserActor, taskCorrelationId } from "./actors";
import {
  type AgentOutcome,
  APPROVAL_DECISION_KIND,
  createFrontalCodeAgent,
  type FrontalCodeAgentOptions,
} from "./agent";
import type { Logger } from "./log";
import { createFrontalCodePolicy } from "./policy";
import type { OcpStoreHandle } from "./store";
import { createSlackTransport, type SlackChatClient, slackBlocksArtifact } from "./transport";
import type {
  FrontalCodeApprovalAction,
  FrontalCodeApprovalKind,
  FrontalCodeTask,
  SlackBlock,
} from "./types";

const STOP_TIMEOUT_MS = 10_000;

export interface SlackUpdateClient extends SlackChatClient {
  chat: SlackChatClient["chat"] & {
    update(args: {
      channel: string;
      ts: string;
      text?: string;
      blocks?: SlackBlock[];
    }): Promise<unknown>;
  };
}

export interface OcpBridgeOptions {
  agentId?: string;
  api: FrontalCodeActionsApi;
  slack: SlackUpdateClient;
  store: OcpStoreHandle;
  runMigrations: boolean;
  logger: Logger;
  onTaskCreated: FrontalCodeAgentOptions["onTaskCreated"];
}

export interface SlackPrompt {
  eventId?: string;
  userId: string;
  channelId: string;
  threadTs?: string;
  text: string;
}

export interface SlackApprovalDecision {
  eventId?: string;
  taskId: string;
  approvalKind: FrontalCodeApprovalKind;
  action: FrontalCodeApprovalAction;
  slackUserId: string;
  // Recorded by the control plane as the approver; may be a Slack display name.
  resolvedBy: string;
  reason?: string;
  channelId: string;
  threadTs?: string;
}

export interface SlackThreadMessage {
  channelId: string;
  threadTs?: string;
  text: string;
  blocks?: SlackBlock[];
  correlationId?: string;
}

export interface SlackMessageUpdate {
  channelId: string;
  ts: string;
  text?: string;
  blocks?: SlackBlock[];
}

export class OcpBridge {
  readonly app: OpenChannel;
  private readonly agent: ActorRef;
  private readonly outcomes = new Map<string, AgentOutcome>();
  private starting?: Promise<void>;

  constructor(private readonly options: OcpBridgeOptions) {
    this.agent = frontalCodeAgentActor(options.agentId);
    this.app = openChannel({
      policy: createFrontalCodePolicy(),
      store: options.store.store,
      agents: [
        createFrontalCodeAgent({
          id: this.agent.id,
          onTaskCreated: options.onTaskCreated,
          onOutcome: (eventId, outcome) => this.outcomes.set(eventId, outcome),
        }),
      ],
      actions: frontalCodeActions(options.api),
      channels: [
        slackChannel({ transport: createSlackTransport(options.slack), recipient: this.agent }),
      ],
      logger: toAgentLogger(options.logger),
      hooks: {
        onError: (error) => {
          options.logger.error("OpenChannel error", error, {
            code: error.code,
            stage: error.stage,
          });
        },
        onDelivery: (receipt) => {
          options.logger.debug("OpenChannel delivery", {
            eventId: receipt.eventId,
            provider: receipt.provider,
            externalId: receipt.externalId,
          });
        },
      },
    });
  }

  // Idempotent; Slack handlers call it too so events that race `connect()` are not rejected.
  start(): Promise<void> {
    this.starting ??= (async () => {
      if (this.options.runMigrations) {
        await this.options.store.migrate();
      }
      await this.app.start();
    })();
    return this.starting;
  }

  async stop(): Promise<void> {
    if (this.starting) {
      await this.starting.catch(() => undefined);
      await this.app.stop({ timeoutMs: STOP_TIMEOUT_MS });
    }
    await this.options.store.close();
  }

  ping(): Promise<boolean> {
    return this.options.store.ping();
  }

  // Ingests a Slack Events API message through the `ocp-sdk/slack` normalizer. Accepts either the
  // event_callback envelope or a bare message event. Returns false for duplicates and non-messages.
  async receiveSlackEvent(input: unknown, eventId?: string): Promise<boolean> {
    await this.start();
    const results = await this.app.receive(
      SLACK_PROVIDER,
      input,
      eventId ? { eventId } : undefined,
    );
    return results.some((result) => !result.duplicate);
  }

  // Ingests a prompt that has no Events API message behind it (@mentions and /code commands).
  async receiveSlackPrompt(prompt: SlackPrompt): Promise<boolean> {
    await this.start();
    const actor = slackUserActor(prompt.userId);
    const eventId = prompt.eventId ?? randomUUID();
    const result = await this.app.ingest(
      messageReceived({
        actor,
        source: actor,
        recipient: this.agent,
        channel: slackChannelRef(prompt.channelId, prompt.threadTs),
        conversation: slackConversationRef(prompt.channelId),
        eventId,
        idempotencyKey: eventId,
        extensions: { "ocp.provider": SLACK_PROVIDER },
        text: prompt.text,
      }),
    );
    return !result.duplicate;
  }

  // Records a Slack approval click as an inbound message and lets the agent resolve it through the
  // `frontal-code.task.approval.resolve` action. Resolves with the updated task or throws.
  async resolveApproval(decision: SlackApprovalDecision): Promise<FrontalCodeTask> {
    await this.start();
    const actor = slackUserActor(decision.slackUserId);
    const eventId = decision.eventId ?? `slack-approval:${randomUUID()}`;
    const ingested = await this.app.ingest(
      messageReceived({
        actor,
        source: actor,
        recipient: this.agent,
        channel: slackChannelRef(decision.channelId, decision.threadTs),
        conversation: slackConversationRef(decision.channelId),
        // The resulting action.executed/action.failed event inherits this, joining the task trail.
        correlationId: taskCorrelationId(decision.taskId),
        eventId,
        idempotencyKey: eventId,
        extensions: { "ocp.provider": SLACK_PROVIDER },
        metadata: {
          kind: APPROVAL_DECISION_KIND,
          taskId: decision.taskId,
          approvalKind: decision.approvalKind,
          action: decision.action,
          resolvedBy: decision.resolvedBy,
          ...(decision.reason ? { reason: decision.reason } : {}),
        },
        text: `${decision.action} ${decision.approvalKind} for task ${decision.taskId}`,
      }),
    );

    const outcome = this.outcomes.get(eventId);
    this.outcomes.delete(eventId);
    if (ingested.duplicate) {
      throw new Error(`Approval decision ${eventId} was already processed`);
    }
    if (outcome?.kind !== "approval") {
      throw new Error(`Approval decision ${eventId} was not handled by the Frontal Code agent`);
    }

    const { result } = outcome;
    switch (result.status) {
      case "executed":
        return result.result;
      case "failed":
        throw result.error;
      case "blocked":
        throw new Error(`Approval blocked by policy: ${result.reason}`);
      case "approval_required":
        throw new Error(`Approval decision ${eventId} unexpectedly requires approval`);
    }
  }

  // Posts a message as the agent: recorded as `message.sent`, then delivered by the Slack sink.
  async sendToThread(message: SlackThreadMessage): Promise<{ ts?: string }> {
    await this.start();
    const event = messageSent({
      actor: this.agent,
      source: this.agent,
      channel: slackChannelRef(message.channelId, message.threadTs),
      conversation: slackConversationRef(message.channelId),
      ...(message.correlationId ? { correlationId: message.correlationId } : {}),
      text: message.text,
      attachments: blockAttachments(message.blocks),
    });
    await this.app.ingest(event);
    const receipt = await this.app.gateway.dispatch(event);
    return { ts: receipt.externalId };
  }

  // Edits an existing Slack message. The SDK sink has no update operation, so the edit goes through
  // the Web API directly and is recorded afterwards as a `message.sent` with kind=update.
  async updateMessage(update: SlackMessageUpdate): Promise<void> {
    await this.options.slack.chat.update({
      channel: update.channelId,
      ts: update.ts,
      ...(update.text !== undefined ? { text: update.text } : {}),
      ...(update.blocks ? { blocks: update.blocks } : {}),
    });
    await this.recordAudit(
      messageSent({
        actor: this.agent,
        source: this.agent,
        channel: slackChannelRef(update.channelId),
        conversation: slackConversationRef(update.channelId),
        metadata: { kind: "update", slack_ts: update.ts },
        text: update.text ?? "",
        attachments: blockAttachments(update.blocks),
      }),
    );
  }

  // The Slack side effect already happened; an audit write failure is logged, not rethrown.
  private async recordAudit(event: EventEnvelope): Promise<void> {
    try {
      await this.start();
      await this.app.ingest(event);
    } catch (error) {
      this.options.logger.warn("Failed to record OCP audit event", {
        eventId: event.eventId,
        error: (error as Error).message,
      });
    }
  }
}

// Envelope builders wrap their input with protobuf `create()`, so plain initializers are valid at
// runtime; the casts only bridge the generated message types.
function slackChannelRef(channelId: string, threadTs?: string): ChannelRef {
  return { provider: SLACK_PROVIDER, channelId, threadId: threadTs ?? "" } as ChannelRef;
}

function slackConversationRef(channelId: string): ConversationRef {
  return { id: `${SLACK_PROVIDER}:${channelId}` } as ConversationRef;
}

type MessageAttachments = NonNullable<Parameters<typeof messageSent>[0]["attachments"]>;

function blockAttachments(blocks: SlackBlock[] | undefined): MessageAttachments {
  return (blocks ? [slackBlocksArtifact(blocks)] : []) as MessageAttachments;
}

function toAgentLogger(logger: Logger): AgentLogger {
  return {
    debug: (message, details) => logger.debug(message, details),
    info: (message, details) => logger.info(message, details),
    warn: (message, details) => logger.warn(message, details),
    error: (message, details) => logger.error(message, undefined, details),
  };
}
