import { describe, expect, it } from "vitest";
import {
  canActOnTask,
  canCreateTask,
  createSlackAccessPolicy,
  isDirectMessage,
  stripBotMention,
} from "../src/access";

const allowlisted = createSlackAccessPolicy({
  allowedUserIds: ["U111"],
  allowedChannelIds: ["C222"],
  adminUserIds: ["U999"],
});

describe("task creation access", () => {
  it("allows an allowlisted user in an allowlisted channel", () => {
    expect(canCreateTask(allowlisted, { userId: "U111", channelId: "C222" })).toBe(true);
  });

  it("denies a user who is not allowlisted, even in an allowlisted channel", () => {
    expect(canCreateTask(allowlisted, { userId: "U333", channelId: "C222" })).toBe(false);
  });

  it("denies an allowlisted user in a channel that is not allowlisted", () => {
    expect(canCreateTask(allowlisted, { userId: "U111", channelId: "C404" })).toBe(false);
  });

  it("denies everything when the user allowlist is empty, even with channel and admin lists set", () => {
    const policy = createSlackAccessPolicy({
      allowedChannelIds: ["C222"],
      adminUserIds: ["U999"],
    });

    expect(canCreateTask(policy, { userId: "U111", channelId: "C222" })).toBe(false);
    expect(canCreateTask(policy, { userId: "U999", channelId: "C222" })).toBe(false);
    expect(canCreateTask(policy, { userId: "U111", channelId: "D123" })).toBe(false);
  });

  it("denies everything when no allowlist is configured at all", () => {
    const policy = createSlackAccessPolicy({});

    expect(canCreateTask(policy, { userId: "U111", channelId: "C222" })).toBe(false);
  });

  it("allows any channel when the channel allowlist is empty", () => {
    const policy = createSlackAccessPolicy({ allowedUserIds: ["U111"] });

    expect(canCreateTask(policy, { userId: "U111", channelId: "C404" })).toBe(true);
  });

  it("denies a request with no user id", () => {
    expect(canCreateTask(allowlisted, { userId: undefined, channelId: "C222" })).toBe(false);
  });

  it("does not apply the channel allowlist to direct messages", () => {
    expect(canCreateTask(allowlisted, { userId: "U111", channelId: "D123" })).toBe(true);
    expect(
      canCreateTask(allowlisted, { userId: "U111", channelId: "C404", channelType: "im" }),
    ).toBe(true);
  });

  it("still requires an allowlisted user for direct messages", () => {
    expect(canCreateTask(allowlisted, { userId: "U333", channelId: "D123" })).toBe(false);
  });
});

describe("task action access", () => {
  it("allows the task creator who is allowlisted", () => {
    expect(canActOnTask(allowlisted, { actorId: "U111", ownerId: "U111" })).toBe(true);
  });

  it("denies a different allowlisted user who is not an admin", () => {
    expect(canActOnTask(allowlisted, { actorId: "U111", ownerId: "U555" })).toBe(false);
  });

  it("allows an allowlisted admin to act on any task", () => {
    const policy = createSlackAccessPolicy({
      allowedUserIds: ["U111", "U999"],
      adminUserIds: ["U999"],
    });

    expect(canActOnTask(policy, { actorId: "U999", ownerId: "U111" })).toBe(true);
  });

  it("denies an admin who is not allowlisted", () => {
    const policy = createSlackAccessPolicy({
      allowedUserIds: ["U111"],
      adminUserIds: ["U999"],
    });

    expect(canActOnTask(policy, { actorId: "U999", ownerId: "U111" })).toBe(false);
  });

  it("denies the creator once they are removed from the allowlist", () => {
    const policy = createSlackAccessPolicy({ allowedUserIds: ["U222"] });

    expect(canActOnTask(policy, { actorId: "U111", ownerId: "U111" })).toBe(false);
  });

  it("only lets admins act when the task owner is unknown", () => {
    const policy = createSlackAccessPolicy({
      allowedUserIds: ["U111", "U999"],
      adminUserIds: ["U999"],
    });

    expect(canActOnTask(policy, { actorId: "U111", ownerId: undefined })).toBe(false);
    expect(canActOnTask(policy, { actorId: "U999", ownerId: undefined })).toBe(true);
  });
});

describe("helpers", () => {
  it("detects direct message conversations", () => {
    expect(isDirectMessage("D123")).toBe(true);
    expect(isDirectMessage("C123", "im")).toBe(true);
    expect(isDirectMessage("C123")).toBe(false);
    expect(isDirectMessage("G123", "mpim")).toBe(false);
  });

  it("strips bot mention tokens from prompts", () => {
    expect(stripBotMention("<@U0BOT123> fix the flaky test")).toBe("fix the flaky test");
    expect(stripBotMention("<@U0BOT123>")).toBe("");
  });
});
