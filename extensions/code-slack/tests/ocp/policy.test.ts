import { ActorType, createActor } from "ocp-sdk";
import { describe, expect, it } from "vitest";
import { CREATE_TASK_ACTION, RESOLVE_APPROVAL_ACTION } from "../../src/actions";
import { frontalCodeAgentActor, slackUserActor, slackUserIdOf } from "../../src/actors";
import { createFrontalCodePolicy } from "../../src/policy";

describe("Frontal Code OCP policy", () => {
  const policy = createFrontalCodePolicy();
  const actor = frontalCodeAgentActor();

  it("allows task creation and approval resolution without a human approval step", () => {
    for (const actionType of [CREATE_TASK_ACTION, RESOLVE_APPROVAL_ACTION]) {
      const decision = policy.evaluate({ actionType, actor });
      expect(decision).toMatchObject({ allowed: true, approvalRequired: false });
    }
  });

  it("denies every other action by default", () => {
    const decision = policy.evaluate({ actionType: "frontal-code.repo.delete", actor });
    expect(decision.allowed).toBe(false);
    expect(decision.reason).toMatch(/not granted/);
  });
});

describe("OCP actors", () => {
  it("maps Slack users to slack:<id> human actors and back", () => {
    const user = slackUserActor("U123");
    expect(user).toMatchObject({ id: "slack:U123", type: ActorType.HUMAN });
    expect(slackUserIdOf(user)).toBe("U123");
  });

  it("returns no Slack user id for agents, systems, or a bare prefix", () => {
    expect(slackUserIdOf(frontalCodeAgentActor())).toBeUndefined();
    expect(slackUserIdOf(createActor("slack:", ActorType.HUMAN, "nobody"))).toBeUndefined();
    expect(slackUserIdOf(undefined)).toBeUndefined();
  });
});
