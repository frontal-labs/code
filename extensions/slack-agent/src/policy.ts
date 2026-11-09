// OCP policy for the Slack host. Default deny: only the two Frontal Code actions are granted.
// Who may trigger them (allowlists, task ownership) is enforced by `src/access.ts` before a request
// reaches the agent; this policy bounds what the agent itself is able to do.
import { type PolicyEngine, RuleBasedPolicyEngine } from "@openchannelprotocol/sdk";
import { CREATE_TASK_ACTION, RESOLVE_APPROVAL_ACTION } from "./actions";

export function createFrontalCodePolicy(): PolicyEngine {
  return new RuleBasedPolicyEngine({
    defaultAllowed: false,
    defaultReason: "Action is not granted to the Frontal Code Slack agent",
    rules: [
      {
        id: "frontal-code.slack.task-create",
        actionType: CREATE_TASK_ACTION,
        allowed: true,
        reason: "Allowlisted Slack users may start Frontal Code tasks",
      },
      {
        id: "frontal-code.slack.approval-resolve",
        actionType: RESOLVE_APPROVAL_ACTION,
        allowed: true,
        reason: "Task owners and admins may resolve Frontal Code approvals from Slack",
      },
    ],
  });
}
