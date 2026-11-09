import { describe, expect, it, vi } from "vitest";
import {
  blocksFromAttachments,
  createSlackTransport,
  SLACK_BLOCKS_MEDIA_TYPE,
  slackBlocksArtifact,
} from "../../src/transport";
import type { SlackBlock } from "../../src/types";

const blocks: SlackBlock[] = [
  { type: "section", text: { type: "mrkdwn", text: "*Task needs approval* — retry?" } },
];

describe("Slack OCP transport", () => {
  it("round-trips Block Kit through a data-URI artifact", () => {
    const artifact = slackBlocksArtifact(blocks);

    expect(artifact.mediaType).toBe(SLACK_BLOCKS_MEDIA_TYPE);
    expect(artifact.uri.startsWith(`data:${SLACK_BLOCKS_MEDIA_TYPE};base64,`)).toBe(true);
    expect(blocksFromAttachments([artifact])).toEqual(blocks);
  });

  it("ignores artifacts that are not Slack blocks", () => {
    expect(
      blocksFromAttachments([
        { mediaType: "image/png", uri: "https://example.com/a.png" },
        { mediaType: SLACK_BLOCKS_MEDIA_TYPE, uri: "https://example.com/blocks.json" },
      ]),
    ).toBeUndefined();
    expect(blocksFromAttachments([])).toBeUndefined();
  });

  it("posts text, thread, and decoded blocks and returns the Slack ts as the message id", async () => {
    const postMessage = vi.fn().mockResolvedValue({ ts: "1710000000.200" });
    const transport = createSlackTransport({ chat: { postMessage } });

    const result = await transport.postMessage({
      channelId: "C123",
      threadId: "1710000000.100",
      text: "hello",
      attachments: [slackBlocksArtifact(blocks)],
    });

    expect(postMessage).toHaveBeenCalledWith({
      channel: "C123",
      text: "hello",
      thread_ts: "1710000000.100",
      blocks,
    });
    expect(result).toEqual({ messageId: "1710000000.200" });
  });

  it("posts top-level messages when the OCP thread id is empty", async () => {
    const postMessage = vi.fn().mockResolvedValue({ ts: "1710000000.300" });
    const transport = createSlackTransport({ chat: { postMessage } });

    await transport.postMessage({ channelId: "C123", threadId: "", text: "hi", attachments: [] });

    expect(postMessage).toHaveBeenCalledWith({ channel: "C123", text: "hi", thread_ts: undefined });
  });
});
