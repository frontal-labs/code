// OCP actor identities used by the Slack host. Slack users follow the `slack:<userId>` convention
// that `ocp-sdk/slack` uses when it normalizes inbound messages, so audit queries can join on it.
import { type ActorRef, ActorType, createActor } from "ocp-sdk";

export const SLACK_PROVIDER = "slack";
export const DEFAULT_AGENT_ID = "agent:frontal-code";

const SLACK_ACTOR_PREFIX = "slack:";

// Correlation ID that groups every OCP event about one Frontal Code task for audit queries.
export function taskCorrelationId(taskId: string): string {
  return `frontal-code-task:${taskId}`;
}

export function frontalCodeAgentActor(agentId: string = DEFAULT_AGENT_ID): ActorRef {
  return createActor(agentId, ActorType.AGENT, "Frontal Code", [
    "frontal-code.task.create",
    "frontal-code.task.approval.resolve",
  ]);
}

export function slackUserActor(userId: string): ActorRef {
  return createActor(`${SLACK_ACTOR_PREFIX}${userId}`, ActorType.HUMAN, userId);
}

// Returns the Slack user ID behind a `slack:<userId>` actor, or undefined for any other actor.
export function slackUserIdOf(actor: ActorRef | undefined): string | undefined {
  if (!actor?.id.startsWith(SLACK_ACTOR_PREFIX)) {
    return undefined;
  }
  const userId = actor.id.slice(SLACK_ACTOR_PREFIX.length);
  return userId || undefined;
}
