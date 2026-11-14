# Product analytics

`frontal-code` can send a small set of anonymous usage events to PostHog so we can see which commands and tools people use and where they fail. This page is the reference for what is sent, how to turn it off, and how it works.

**Analytics is off by default.** Nothing is sent until you opt in.

## Turn it on or off

Opt in by adding this to your **user** settings file (`~/.frontal-code/settings.json`, or `$FRONTAL_CODE_CONFIG_HOME/settings.json`):

```json
{
  "analytics": {
    "enabled": true
  }
}
```

The first time you start the interactive REPL in a terminal, Frontal Code asks once whether to share anonymous usage events. The default is no. Your answer is saved to the user settings file, so the question is not asked again.

Or for a single shell session:

```sh
export FRONTAL_CODE_ANALYTICS_ENABLED=1
```

Check the current state with `frontal-code telemetry` (or `/telemetry` in the REPL). It prints the reason when analytics is off.

### Ways to turn it off

Any one of these wins over an opt-in:

| Signal | Effect |
|---|---|
| `FRONTAL_CODE_ANALYTICS_DISABLED=1` | Disabled for this environment. |
| `DO_NOT_TRACK=1` | Disabled. Honours the common convention. |
| `CI` set to any non-empty value | Disabled in CI. |
| `--no-analytics` flag | Disabled for this run. |
| `"analytics": { "enabled": false }` in user settings | Disabled. |

Analytics is also disabled when no PostHog project key is set (`FRONTAL_CODE_POSTHOG_KEY`), or when the build was made without the `posthog` feature.

### Settings are read from user files only

`analytics` settings are read only from the **user** settings file. A project or local `.frontal-code/settings.json` cannot turn analytics on, change where it sends data, or override the opt-out. If one of those files contains an `analytics` key, the CLI prints a warning and ignores it.

## What is sent

Every event has the same set of fixed fields, listed below. Each event adds only the properties in its table. Every string value is one of a fixed set of labels, and there is no free text.

**Attached to every event:**

| Field | Example | Notes |
|---|---|---|
| `distinct_id` | `45c7c700-…` | A random anonymous install ID. See [Anonymous ID](#anonymous-id). |
| `app_version` | `0.3.0` | Version of `frontal-code`. |
| `environment` | `production` | `development` for debug builds. |
| `$os` | `Mac OS` | Added by the PostHog SDK. OS family name only. The OS version is removed before sending. |
| `$lib`, `$lib_version` | `posthog-rs`, `0.33.1` | Added by the PostHog SDK. |

IP-based location lookup is disabled (`$geoip_disable`), and person profiles are not created (`$process_person_profile: false`).

### Events

| Event | Properties | Sent when |
|---|---|---|
| `session_started` | `mode`: `repl`, `prompt`, `resume`, `hosted` | A session starts. |
| `session_ended` | `duration_secs`, `turns`, `exit`: `ok`, `error`, `interrupted` | A session ends. `turns` is the number of agent turns completed in that process. `interrupted` means the user stopped the REPL with Ctrl-C. |
| `command_run` | `command`, `outcome`: `success`, `failure`, `duration_ms` | A top-level command finishes. `command` is one of `prompt`, `repl`, `doctor`, `init`, `status`, `config`, `login`, `logout`, `plugins`, `mcp`, `skills`, `agents`, `hosted`, `telemetry`, `other`. |
| `turn_completed` | `provider`, `iterations`, `tool_calls`, `duration_ms` | An agent turn finishes successfully. `provider` is `anthropic`, `openai`, `xai`, `frontal` or `other`. |
| `tool_used` | `tool`, `outcome`, `duration_ms` | A built-in tool finishes. `tool` is one of `bash`, `read_file`, `write_file`, `edit_file`, `glob_search`, `grep_search`, `web_fetch`, `web_search`, or `other`. |
| `error_occurred` | `class`: `provider` (a provider request failed without retry), `turn` (an agent turn failed) | Something fails. Only the category is sent. |
| `login_completed` | `provider`, `outcome` | A `frontal-code login` attempt finishes. |

Durations are whole milliseconds or seconds, and counts are integers.

## What is never sent

- Prompts, model responses, or any conversation content.
- File paths, file names, or file contents.
- Tool arguments, shell commands, or tool output.
- Names of MCP servers, plugins, or tools outside the built-in list above.
- Error messages or stack traces. Only the category is sent.
- API keys, tokens, or other credentials.
- Hostnames, user names, or email addresses.
- Source code, repository names, or git information.

Keeping this list true is enforced by types rather than by filtering. Event properties are closed enums and integers, so free text cannot be expressed. As a second layer, every event passes through a filter before it leaves the process. The filter drops unknown event names, unknown property keys, and string values that are not known labels. Both layers are covered by tests.

## Anonymous ID

The first time analytics sends an event, a random UUID v4 is written to `analytics_id` in the config directory. It is not derived from your hostname, user name, or any path, and it is not linked to an account.

To reset it, run:

```sh
frontal-code telemetry reset-id
```

A new ID is created the next time an event is sent. You can also delete `analytics_id` from the config directory by hand. If the file cannot be written, a new random ID is used for that process only.

## Where events go

Events are sent to `https://us.i.posthog.com` by default. To use a different PostHog host (for example EU or self-hosted), set it in user settings:

```json
{
  "analytics": {
    "enabled": true,
    "host": "https://eu.i.posthog.com"
  }
}
```

## How sending works

- Events are queued in memory and sent in batches of up to 20, or every 10 seconds, whichever comes first.
- The queue holds at most 1,000 events. When it is full, new events are dropped. Recording an event never blocks the CLI.
- Requests time out after 5 seconds. Server errors and timeouts are attempted up to 3 times in total, with exponential backoff starting at 200 ms and capped at 5 s.
- At exit the CLI waits up to about 2 seconds to drain the queue. A request that is already in flight can run until its 5-second timeout.

At exit, the CLI first waits for any batch that is waiting to be retried, for up to 2 seconds. A batch that still cannot be delivered is dropped, so an unreachable analytics host adds at most a few seconds to exit and never blocks the command's own output.

## For developers

- The event model lives in `crates/code-analytics/src/event.rs`. The `CATALOG` constant there is the source of truth for event names and property keys. A test checks this page against it.
- The CLI wiring is in `cli/src/analytics.rs`, and the event emission points are in `cli/src/main.rs`.
- The existing local JSONL telemetry (`telemetry.path`, `FRONTAL_CODE_TELEMETRY_PATH`) is separate. It stays on your machine and is unchanged by this feature.
- To run the CLI against a local recorder instead of PostHog, set `analytics.host` to the recorder's URL and `FRONTAL_CODE_POSTHOG_KEY` to any value.
- Tests use a local stub server, so they never send data anywhere. Run them with `cargo test -p code-analytics`.
