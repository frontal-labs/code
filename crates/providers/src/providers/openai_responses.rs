//! `OpenAI` Responses API adapter.
//!
//! Codex-family models (for example `gpt-5-codex`) are served only through the
//! Responses API, not chat completions. This module translates requests and
//! responses (both non-streaming and streamed) to and from the shared
//! `MessageRequest` / `StreamEvent` types, so callers see the same interface
//! regardless of endpoint. The chat-completions path lives in `openai.rs`.

use std::collections::BTreeMap;

use serde_json::{json, Map, Value};

use crate::error::ApiError;
use crate::types::{
    ContentBlockDelta, ContentBlockDeltaEvent, ContentBlockStartEvent, ContentBlockStopEvent,
    InputContentBlock, InputMessage, MessageDelta, MessageDeltaEvent, MessageRequest,
    MessageResponse, MessageStartEvent, MessageStopEvent, OutputContentBlock, StreamEvent,
    ToolChoice, ToolDefinition, ToolResultContentBlock, Usage,
};

/// Whether `model` is served only through the Responses API.
#[must_use]
pub fn model_requires_responses(model: &str) -> bool {
    model.contains("codex")
}

/// Full URL for the Responses endpoint, accepting either a base URL or a full endpoint.
pub(crate) fn responses_endpoint(base_url: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    if trimmed.ends_with("/responses") {
        trimmed.to_string()
    } else {
        format!("{trimmed}/responses")
    }
}

/// Builds a Responses API request body. Storage is disabled so prompts are not kept
/// on the provider side by default.
pub(crate) fn build_request(request: &MessageRequest) -> Value {
    let input = request
        .messages
        .iter()
        .flat_map(translate_message)
        .collect::<Vec<_>>();
    let mut payload = json!({
        "model": request.model,
        "input": input,
        "max_output_tokens": request.max_tokens,
        "stream": request.stream,
        "store": false,
    });
    if let Some(system) = request.system.as_ref().filter(|value| !value.is_empty()) {
        payload["instructions"] = json!(system);
    }
    if let Some(tools) = &request.tools {
        payload["tools"] = Value::Array(tools.iter().map(tool_definition).collect());
    }
    if let Some(tool_choice) = &request.tool_choice {
        payload["tool_choice"] = tool_choice_value(tool_choice);
    }
    payload
}

fn translate_message(message: &InputMessage) -> Vec<Value> {
    let is_assistant = message.role == "assistant";
    message
        .content
        .iter()
        .map(|block| match block {
            InputContentBlock::Text { text } if is_assistant => json!({
                "role": "assistant",
                "content": [{ "type": "output_text", "text": text }],
            }),
            InputContentBlock::Text { text } => json!({
                "role": "user",
                "content": [{ "type": "input_text", "text": text }],
            }),
            InputContentBlock::ToolUse { id, name, input } => json!({
                "type": "function_call",
                "call_id": id,
                "name": name,
                "arguments": input.to_string(),
            }),
            InputContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                let text = flatten_tool_result(content);
                let output = if *is_error {
                    format!("[tool error] {text}")
                } else {
                    text
                };
                json!({
                    "type": "function_call_output",
                    "call_id": tool_use_id,
                    "output": output,
                })
            }
        })
        .collect()
}

fn flatten_tool_result(content: &[ToolResultContentBlock]) -> String {
    content
        .iter()
        .map(|block| match block {
            ToolResultContentBlock::Text { text } => text.clone(),
            ToolResultContentBlock::Json { value } => value.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn tool_definition(tool: &ToolDefinition) -> Value {
    let mut definition = Map::new();
    definition.insert("type".to_string(), json!("function"));
    definition.insert("name".to_string(), json!(tool.name));
    if let Some(description) = &tool.description {
        definition.insert("description".to_string(), json!(description));
    }
    definition.insert("parameters".to_string(), tool.input_schema.clone());
    Value::Object(definition)
}

fn tool_choice_value(tool_choice: &ToolChoice) -> Value {
    match tool_choice {
        ToolChoice::Auto => json!("auto"),
        ToolChoice::Any => json!("required"),
        ToolChoice::Tool { name } => json!({ "type": "function", "name": name }),
    }
}

/// Converts a non-streaming Responses reply into a `MessageResponse`.
pub(crate) fn normalize_response(fallback_model: &str, value: &Value) -> MessageResponse {
    let mut content = Vec::new();
    let mut saw_tool_use = false;
    for item in value
        .get("output")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        match item.get("type").and_then(Value::as_str) {
            Some("message") => {
                for part in item
                    .get("content")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    if part.get("type").and_then(Value::as_str) == Some("output_text") {
                        if let Some(text) = part
                            .get("text")
                            .and_then(Value::as_str)
                            .filter(|text| !text.is_empty())
                        {
                            content.push(OutputContentBlock::Text {
                                text: text.to_string(),
                            });
                        }
                    }
                }
            }
            Some("function_call") => {
                saw_tool_use = true;
                content.push(OutputContentBlock::ToolUse {
                    id: str_field(item, "call_id"),
                    name: str_field(item, "name"),
                    input: parse_arguments(item.get("arguments").and_then(Value::as_str)),
                });
            }
            // Reasoning and other item types are not surfaced to callers.
            _ => {}
        }
    }

    let model = value
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or(fallback_model)
        .to_string();
    MessageResponse {
        id: str_field(value, "id"),
        kind: "message".to_string(),
        role: "assistant".to_string(),
        content,
        model,
        stop_reason: Some(stop_reason(
            saw_tool_use,
            value.get("status").and_then(Value::as_str),
            incomplete_reason(value),
        )),
        stop_sequence: None,
        usage: parse_usage(value.get("usage")),
        request_id: None,
    }
}

fn stop_reason(saw_tool_use: bool, status: Option<&str>, reason: Option<&str>) -> String {
    if saw_tool_use {
        "tool_use"
    } else if status == Some("incomplete") && reason == Some("max_output_tokens") {
        "max_tokens"
    } else {
        "end_turn"
    }
    .to_string()
}

fn incomplete_reason(response: &Value) -> Option<&str> {
    response
        .get("incomplete_details")
        .and_then(|details| details.get("reason"))
        .and_then(Value::as_str)
}

fn parse_usage(usage: Option<&Value>) -> Usage {
    let number = |value: Option<&Value>| {
        value
            .and_then(Value::as_u64)
            .map_or(0, |count| u32::try_from(count).unwrap_or(u32::MAX))
    };
    let usage_value = usage;
    Usage {
        input_tokens: number(usage_value.and_then(|value| value.get("input_tokens"))),
        cache_creation_input_tokens: 0,
        cache_read_input_tokens: number(
            usage_value
                .and_then(|value| value.get("input_tokens_details"))
                .and_then(|details| details.get("cached_tokens")),
        ),
        output_tokens: number(usage_value.and_then(|value| value.get("output_tokens"))),
    }
}

fn str_field(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn parse_arguments(arguments: Option<&str>) -> Value {
    let raw = arguments.unwrap_or_default();
    serde_json::from_str(raw).unwrap_or_else(|_| json!({ "raw": raw }))
}

/// Parser state for a streamed Responses reply. Feed raw bytes with `push` and call
/// `finish` when the body ends.
#[derive(Debug, Default)]
pub(crate) struct ResponsesStream {
    buffer: Vec<u8>,
    model: String,
    message_started: bool,
    completed: bool,
    next_index: u32,
    block_by_output: BTreeMap<u64, u32>,
    saw_tool_use: bool,
}

impl ResponsesStream {
    pub(crate) fn new(model: String) -> Self {
        Self {
            model,
            ..Self::default()
        }
    }

    pub(crate) fn push(&mut self, chunk: &[u8]) -> Result<Vec<StreamEvent>, ApiError> {
        self.buffer.extend_from_slice(chunk);
        let mut events = Vec::new();
        while let Some(frame) = super::openai::next_sse_frame(&mut self.buffer) {
            if let Some(data) = frame_data(&frame) {
                let event = serde_json::from_str::<Value>(&data)?;
                events.extend(self.ingest(&event)?);
            }
        }
        Ok(events)
    }

    /// Called when the body ends. A reply that never reached `response.completed` was
    /// cut off, so it is reported as an error rather than returned as a partial answer.
    pub(crate) fn finish(&mut self) -> Result<Vec<StreamEvent>, ApiError> {
        if self.completed {
            Ok(Vec::new())
        } else {
            Err(ApiError::InvalidSseFrame(
                "responses stream ended before response.completed",
            ))
        }
    }

    fn ingest(&mut self, event: &Value) -> Result<Vec<StreamEvent>, ApiError> {
        let mut events = Vec::new();
        match event
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
        {
            "response.created" => {
                if !self.message_started {
                    self.message_started = true;
                    events.push(StreamEvent::MessageStart(MessageStartEvent {
                        message: MessageResponse {
                            id: event
                                .get("response")
                                .map(|response| str_field(response, "id"))
                                .unwrap_or_default(),
                            kind: "message".to_string(),
                            role: "assistant".to_string(),
                            content: Vec::new(),
                            model: self.model.clone(),
                            stop_reason: None,
                            stop_sequence: None,
                            usage: parse_usage(None),
                            request_id: None,
                        },
                    }));
                }
            }
            "response.output_item.added" => {
                let output_index = output_index(event);
                let item = event.get("item").unwrap_or(&Value::Null);
                let content_block = match item.get("type").and_then(Value::as_str) {
                    Some("message") => OutputContentBlock::Text {
                        text: String::new(),
                    },
                    Some("function_call") => {
                        self.saw_tool_use = true;
                        OutputContentBlock::ToolUse {
                            id: str_field(item, "call_id"),
                            name: str_field(item, "name"),
                            input: json!({}),
                        }
                    }
                    _ => return Ok(events),
                };
                let index = self.next_index;
                self.next_index += 1;
                self.block_by_output.insert(output_index, index);
                events.push(StreamEvent::ContentBlockStart(ContentBlockStartEvent {
                    index,
                    content_block,
                }));
            }
            "response.output_text.delta" => {
                if let Some(index) = self.block_for(event) {
                    events.push(StreamEvent::ContentBlockDelta(ContentBlockDeltaEvent {
                        index,
                        delta: ContentBlockDelta::TextDelta {
                            text: str_field(event, "delta"),
                        },
                    }));
                }
            }
            "response.function_call_arguments.delta" => {
                if let Some(index) = self.block_for(event) {
                    events.push(StreamEvent::ContentBlockDelta(ContentBlockDeltaEvent {
                        index,
                        delta: ContentBlockDelta::InputJsonDelta {
                            partial_json: str_field(event, "delta"),
                        },
                    }));
                }
            }
            "response.output_item.done" => {
                if let Some(index) = self.block_by_output.remove(&output_index(event)) {
                    events.push(StreamEvent::ContentBlockStop(ContentBlockStopEvent {
                        index,
                    }));
                }
            }
            "response.completed" | "response.incomplete" => {
                events.extend(self.complete(event));
            }
            "response.failed" | "error" => return Err(stream_error(event)),
            // Lifecycle and part events carry no content the caller needs.
            _ => {}
        }
        Ok(events)
    }

    /// Final usage and stop reason, emitted once when the reply finishes.
    fn complete(&mut self, event: &Value) -> Vec<StreamEvent> {
        self.completed = true;
        let response = event.get("response").unwrap_or(&Value::Null);
        let stop = stop_reason(
            self.saw_tool_use,
            response.get("status").and_then(Value::as_str),
            incomplete_reason(response),
        );
        vec![
            StreamEvent::MessageDelta(MessageDeltaEvent {
                delta: MessageDelta {
                    stop_reason: Some(stop),
                    stop_sequence: None,
                },
                usage: parse_usage(response.get("usage")),
            }),
            StreamEvent::MessageStop(MessageStopEvent {}),
        ]
    }

    fn block_for(&self, event: &Value) -> Option<u32> {
        self.block_by_output.get(&output_index(event)).copied()
    }
}

/// Error for a `response.failed` or `error` event, keeping the server's message.
fn stream_error(event: &Value) -> ApiError {
    let error = event.get("error").or_else(|| {
        event
            .get("response")
            .and_then(|response| response.get("error"))
    });
    ApiError::Api {
        status: reqwest::StatusCode::BAD_GATEWAY,
        error_type: error.and_then(|error| {
            error
                .get("type")
                .and_then(Value::as_str)
                .map(str::to_string)
        }),
        message: Some(
            error
                .and_then(|error| error.get("message").and_then(Value::as_str))
                .unwrap_or("responses stream reported an error")
                .to_string(),
        ),
        request_id: None,
        body: event.to_string(),
        retryable: false,
    }
}

fn output_index(event: &Value) -> u64 {
    event
        .get("output_index")
        .and_then(Value::as_u64)
        .unwrap_or_default()
}

/// Joins the `data:` lines of one SSE frame. Returns `None` for frames without data.
fn frame_data(frame: &str) -> Option<String> {
    let lines = frame
        .lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .map(|line| line.strip_prefix(' ').unwrap_or(line))
        .collect::<Vec<_>>();
    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        build_request, model_requires_responses, normalize_response, responses_endpoint,
        ResponsesStream,
    };
    use crate::error::ApiError;
    use crate::types::{
        ContentBlockDelta, InputContentBlock, InputMessage, MessageRequest, OutputContentBlock,
        StreamEvent, ToolChoice, ToolDefinition, ToolResultContentBlock,
    };

    fn request(messages: Vec<InputMessage>) -> MessageRequest {
        MessageRequest {
            model: "gpt-5-codex".to_string(),
            max_tokens: 512,
            messages,
            system: Some("be brief".to_string()),
            tools: None,
            tool_choice: None,
            stream: false,
        }
    }

    #[test]
    fn routes_only_codex_models_to_responses() {
        assert!(model_requires_responses("gpt-5-codex"));
        assert!(model_requires_responses("gpt-5.3-codex"));
        assert!(!model_requires_responses("gpt-4.1"));
    }

    #[test]
    fn endpoint_accepts_base_or_full_url() {
        assert_eq!(
            responses_endpoint("https://api.openai.com/v1"),
            "https://api.openai.com/v1/responses"
        );
        assert_eq!(
            responses_endpoint("https://api.openai.com/v1/responses"),
            "https://api.openai.com/v1/responses"
        );
    }

    #[test]
    fn request_translates_system_tools_and_tool_round_trip() {
        let mut req = request(vec![
            InputMessage::user_text("list files"),
            InputMessage {
                role: "assistant".to_string(),
                content: vec![InputContentBlock::ToolUse {
                    id: "call_1".to_string(),
                    name: "read_file".to_string(),
                    input: json!({ "path": "a.txt" }),
                }],
            },
            InputMessage {
                role: "user".to_string(),
                content: vec![InputContentBlock::ToolResult {
                    tool_use_id: "call_1".to_string(),
                    content: vec![ToolResultContentBlock::Text {
                        text: "hello".to_string(),
                    }],
                    is_error: false,
                }],
            },
        ]);
        req.tools = Some(vec![ToolDefinition {
            name: "read_file".to_string(),
            description: Some("Read a file".to_string()),
            input_schema: json!({ "type": "object" }),
        }]);
        req.tool_choice = Some(ToolChoice::Tool {
            name: "read_file".to_string(),
        });

        let body = build_request(&req);
        assert_eq!(body["instructions"], "be brief");
        assert_eq!(body["max_output_tokens"], 512);
        assert_eq!(body["store"], false);
        assert_eq!(body["input"][0]["content"][0]["type"], "input_text");
        assert_eq!(body["input"][1]["type"], "function_call");
        assert_eq!(body["input"][1]["call_id"], "call_1");
        assert_eq!(body["input"][2]["type"], "function_call_output");
        assert_eq!(body["input"][2]["output"], "hello");
        assert_eq!(body["tools"][0]["name"], "read_file");
        assert_eq!(body["tools"][0]["parameters"]["type"], "object");
        assert_eq!(body["tool_choice"]["name"], "read_file");
    }

    #[test]
    fn non_streaming_reply_maps_text_tool_calls_and_usage() {
        let reply = json!({
            "id": "resp_1",
            "model": "gpt-5-codex",
            "status": "completed",
            "output": [
                { "type": "reasoning", "summary": [] },
                { "type": "message", "content": [{ "type": "output_text", "text": "Done." }] },
                { "type": "function_call", "call_id": "call_9", "name": "bash", "arguments": "{\"cmd\":\"ls\"}" }
            ],
            "usage": { "input_tokens": 10, "output_tokens": 4, "input_tokens_details": { "cached_tokens": 3 } }
        });

        let message = normalize_response("gpt-5-codex", &reply);
        assert_eq!(message.id, "resp_1");
        assert_eq!(
            message.content,
            vec![
                OutputContentBlock::Text {
                    text: "Done.".to_string()
                },
                OutputContentBlock::ToolUse {
                    id: "call_9".to_string(),
                    name: "bash".to_string(),
                    input: json!({ "cmd": "ls" }),
                },
            ]
        );
        assert_eq!(message.stop_reason.as_deref(), Some("tool_use"));
        assert_eq!(message.usage.input_tokens, 10);
        assert_eq!(message.usage.cache_read_input_tokens, 3);
        assert_eq!(message.usage.output_tokens, 4);
    }

    #[test]
    fn incomplete_reply_reports_max_tokens() {
        let reply = json!({
            "id": "resp_2",
            "status": "incomplete",
            "incomplete_details": { "reason": "max_output_tokens" },
            "output": [{ "type": "message", "content": [{ "type": "output_text", "text": "part" }] }],
            "usage": { "input_tokens": 1, "output_tokens": 1 }
        });
        let message = normalize_response("gpt-5-codex", &reply);
        assert_eq!(message.stop_reason.as_deref(), Some("max_tokens"));
    }

    fn sse(event: &str, data: &serde_json::Value) -> String {
        format!("event: {event}\ndata: {data}\n\n")
    }

    #[test]
    fn stream_text_reply_handles_frames_split_across_chunks() {
        let body = [
            sse(
                "response.created",
                &json!({ "type": "response.created", "response": { "id": "resp_3" } }),
            ),
            sse(
                "response.output_item.added",
                &json!({ "type": "response.output_item.added", "output_index": 0, "item": { "type": "message" } }),
            ),
            sse(
                "response.output_text.delta",
                &json!({ "type": "response.output_text.delta", "output_index": 0, "delta": "Hi" }),
            ),
            sse(
                "response.output_item.done",
                &json!({ "type": "response.output_item.done", "output_index": 0, "item": { "type": "message" } }),
            ),
            sse(
                "response.completed",
                &json!({ "type": "response.completed", "response": { "id": "resp_3", "status": "completed", "usage": { "input_tokens": 2, "output_tokens": 1 } } }),
            ),
        ]
        .concat();

        // Split every 7 bytes so frame boundaries fall mid-chunk.
        let mut stream = ResponsesStream::new("gpt-5-codex".to_string());
        let mut events = Vec::new();
        for chunk in body.as_bytes().chunks(7) {
            events.extend(stream.push(chunk).expect("push chunk"));
        }
        events.extend(stream.finish().expect("completed stream finishes"));

        assert!(matches!(events[0], StreamEvent::MessageStart(_)));
        assert!(events.iter().any(|event| matches!(
            event,
            StreamEvent::ContentBlockDelta(delta)
                if delta.delta == ContentBlockDelta::TextDelta { text: "Hi".to_string() }
        )));
        assert!(matches!(events.last(), Some(StreamEvent::MessageStop(_))));
        let stop = events.iter().find_map(|event| match event {
            StreamEvent::MessageDelta(delta) => delta.delta.stop_reason.clone(),
            _ => None,
        });
        assert_eq!(stop.as_deref(), Some("end_turn"));
    }

    #[test]
    fn stream_function_call_emits_tool_block_with_argument_deltas() {
        let body = [
            sse(
                "response.output_item.added",
                &json!({ "type": "response.output_item.added", "output_index": 0, "item": { "type": "function_call", "call_id": "call_7", "name": "bash", "arguments": "" } }),
            ),
            sse(
                "response.function_call_arguments.delta",
                &json!({ "type": "response.function_call_arguments.delta", "output_index": 0, "delta": "{\"cmd\":" }),
            ),
            sse(
                "response.function_call_arguments.delta",
                &json!({ "type": "response.function_call_arguments.delta", "output_index": 0, "delta": "\"ls\"}" }),
            ),
            sse(
                "response.output_item.done",
                &json!({ "type": "response.output_item.done", "output_index": 0, "item": { "type": "function_call" } }),
            ),
            sse(
                "response.completed",
                &json!({ "type": "response.completed", "response": { "status": "completed" } }),
            ),
        ]
        .concat();

        let mut stream = ResponsesStream::new("gpt-5-codex".to_string());
        let events = stream.push(body.as_bytes()).expect("push body");

        assert!(events.iter().any(|event| matches!(
            event,
            StreamEvent::ContentBlockStart(start)
                if matches!(&start.content_block, OutputContentBlock::ToolUse { id, name, .. }
                    if id == "call_7" && name == "bash")
        )));
        let partial: String = events
            .iter()
            .filter_map(|event| match event {
                StreamEvent::ContentBlockDelta(delta) => match &delta.delta {
                    ContentBlockDelta::InputJsonDelta { partial_json } => {
                        Some(partial_json.clone())
                    }
                    _ => None,
                },
                _ => None,
            })
            .collect();
        assert_eq!(partial, "{\"cmd\":\"ls\"}");
        let stop = events.iter().find_map(|event| match event {
            StreamEvent::MessageDelta(delta) => delta.delta.stop_reason.clone(),
            _ => None,
        });
        assert_eq!(stop.as_deref(), Some("tool_use"));
    }

    #[test]
    fn stream_error_event_is_surfaced_with_its_message() {
        let body = sse(
            "error",
            &json!({ "type": "error", "error": { "type": "rate_limit", "message": "slow down" } }),
        );
        let mut stream = ResponsesStream::new("gpt-5-codex".to_string());
        let error = stream
            .push(body.as_bytes())
            .expect_err("error event should fail the stream");
        assert!(matches!(
            error,
            ApiError::Api { message: Some(ref message), .. } if message == "slow down"
        ));
    }

    #[test]
    fn truncated_stream_is_an_error_not_a_partial_answer() {
        let body = sse(
            "response.output_text.delta",
            &json!({ "type": "response.output_text.delta", "output_index": 0, "delta": "partial" }),
        );
        let mut stream = ResponsesStream::new("gpt-5-codex".to_string());
        stream.push(body.as_bytes()).expect("push partial body");
        assert!(stream.finish().is_err());
    }
}
