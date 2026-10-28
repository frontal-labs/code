// Slack Web API port for the `ocp-sdk/slack` sink.
//
// OCP `message.sent` events carry attachments as `ArtifactRef`s, which have no slot for Block Kit.
// Blocks travel as one artifact with media type `application/vnd.slack.blocks+json` whose `uri` is a
// base64 `data:` URI; the transport decodes it back into `blocks`. Other artifacts are ignored.
import type { SlackSendInput, SlackTransport } from "ocp-sdk/slack";
import type { SlackBlock } from "./types";

export const SLACK_BLOCKS_MEDIA_TYPE = "application/vnd.slack.blocks+json";

const DATA_URI_PREFIX = `data:${SLACK_BLOCKS_MEDIA_TYPE};base64,`;

export interface SlackBlocksArtifact {
  id: string;
  uri: string;
  title: string;
  mediaType: string;
}

export interface SlackChatClient {
  chat: {
    postMessage(args: {
      channel: string;
      text: string;
      thread_ts?: string;
      blocks?: SlackBlock[];
    }): Promise<{ ts?: string }>;
  };
}

export function slackBlocksArtifact(blocks: SlackBlock[]): SlackBlocksArtifact {
  return {
    id: "slack-blocks",
    uri: `${DATA_URI_PREFIX}${Buffer.from(JSON.stringify(blocks)).toString("base64")}`,
    title: "Slack Block Kit",
    mediaType: SLACK_BLOCKS_MEDIA_TYPE,
  };
}

export function blocksFromAttachments(attachments: readonly unknown[]): SlackBlock[] | undefined {
  for (const attachment of attachments) {
    const artifact = attachment as Partial<SlackBlocksArtifact> | undefined;
    if (
      artifact?.mediaType !== SLACK_BLOCKS_MEDIA_TYPE ||
      !artifact.uri?.startsWith(DATA_URI_PREFIX)
    ) {
      continue;
    }
    const json = Buffer.from(artifact.uri.slice(DATA_URI_PREFIX.length), "base64").toString("utf8");
    const blocks = JSON.parse(json) as unknown;
    return Array.isArray(blocks) ? (blocks as SlackBlock[]) : undefined;
  }
  return undefined;
}

export function createSlackTransport(client: SlackChatClient): SlackTransport {
  return {
    postMessage: async (input: SlackSendInput) => {
      const blocks = blocksFromAttachments(input.attachments);
      const response = await client.chat.postMessage({
        channel: input.channelId,
        text: input.text,
        // Protobuf strings default to "", which Slack would reject as a thread anchor.
        thread_ts: input.threadId || undefined,
        ...(blocks ? { blocks } : {}),
      });
      return { messageId: response.ts };
    },
  };
}
