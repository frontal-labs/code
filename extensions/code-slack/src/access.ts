// Access control for the Slack bot. All checks are pure so they can be unit tested without Slack.
//
// Rules:
// - Creating a task requires the acting user to be in the user allowlist. An empty user allowlist
//   denies all task creation.
// - When the channel allowlist is non-empty, task creation from a channel (or group/mpim) must
//   happen in an allowlisted channel. Direct messages are never channel-restricted.
// - Approving, retrying, or cancelling a task requires the actor to be allowlisted AND either the
//   task's creator or an admin.

export interface SlackAccessPolicy {
  readonly allowedUserIds: ReadonlySet<string>;
  readonly allowedChannelIds: ReadonlySet<string>;
  readonly adminUserIds: ReadonlySet<string>;
}

export function createSlackAccessPolicy(input: {
  allowedUserIds?: readonly string[];
  allowedChannelIds?: readonly string[];
  adminUserIds?: readonly string[];
}): SlackAccessPolicy {
  return {
    allowedUserIds: new Set(input.allowedUserIds ?? []),
    allowedChannelIds: new Set(input.allowedChannelIds ?? []),
    adminUserIds: new Set(input.adminUserIds ?? []),
  };
}

// Slack direct-message conversations are identified by channel_type "im" or a "D" channel id.
export function isDirectMessage(channelId: string, channelType?: string): boolean {
  return channelType === "im" || channelId.startsWith("D");
}

export function isUserAllowed(policy: SlackAccessPolicy, userId: string | undefined): boolean {
  return Boolean(userId) && policy.allowedUserIds.has(userId as string);
}

export function canCreateTask(
  policy: SlackAccessPolicy,
  input: { userId: string | undefined; channelId: string; channelType?: string },
): boolean {
  if (!isUserAllowed(policy, input.userId)) {
    return false;
  }
  if (isDirectMessage(input.channelId, input.channelType)) {
    return true;
  }
  return policy.allowedChannelIds.size === 0 || policy.allowedChannelIds.has(input.channelId);
}

// Actor may act on a task when allowlisted and either the task owner or an admin.
// An unknown owner (undefined) only permits admins.
export function canActOnTask(
  policy: SlackAccessPolicy,
  input: { actorId: string | undefined; ownerId: string | undefined },
): boolean {
  if (!isUserAllowed(policy, input.actorId)) {
    return false;
  }
  if (input.actorId === input.ownerId && input.ownerId !== undefined) {
    return true;
  }
  return Boolean(input.actorId) && policy.adminUserIds.has(input.actorId as string);
}

// Removes an @-mention token for the bot from a Slack message so the prompt is clean.
export function stripBotMention(text: string): string {
  return text.replace(/<@[UW][A-Z0-9]+>/g, "").trim();
}
