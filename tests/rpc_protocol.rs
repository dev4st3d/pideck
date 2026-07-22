use pi_gui::services::rpc::*;
use serde_json::{Value, json};

fn all_commands() -> Vec<(&'static str, Command)> {
    vec![
        (
            "prompt",
            Command::Prompt {
                message: "hello\nworld\u{2028}still one frame".to_owned(),
                images: Some(vec![ImageContent::new("AA==", "image/png")]),
                streaming_behavior: Some(StreamingBehavior::Steer),
            },
        ),
        (
            "steer",
            Command::Steer {
                message: "adjust".to_owned(),
                images: None,
            },
        ),
        (
            "follow_up",
            Command::FollowUp {
                message: "later".to_owned(),
                images: None,
            },
        ),
        ("abort", Command::Abort),
        (
            "new_session",
            Command::NewSession {
                parent_session: Some("C:/fixtures/parent.jsonl".to_owned()),
            },
        ),
        ("get_state", Command::GetState),
        (
            "set_model",
            Command::SetModel {
                provider: "synthetic".to_owned(),
                model_id: "model".to_owned(),
            },
        ),
        ("cycle_model", Command::CycleModel),
        ("get_available_models", Command::GetAvailableModels),
        (
            "set_thinking_level",
            Command::SetThinkingLevel {
                level: ThinkingLevel::High,
            },
        ),
        ("cycle_thinking_level", Command::CycleThinkingLevel),
        (
            "set_steering_mode",
            Command::SetSteeringMode {
                mode: QueueMode::All,
            },
        ),
        (
            "set_follow_up_mode",
            Command::SetFollowUpMode {
                mode: QueueMode::OneAtATime,
            },
        ),
        (
            "compact",
            Command::Compact {
                custom_instructions: Some("focus".to_owned()),
            },
        ),
        (
            "set_auto_compaction",
            Command::SetAutoCompaction { enabled: true },
        ),
        ("set_auto_retry", Command::SetAutoRetry { enabled: false }),
        ("abort_retry", Command::AbortRetry),
        (
            "bash",
            Command::Bash {
                command: "echo fixture".to_owned(),
                exclude_from_context: Some(true),
            },
        ),
        ("abort_bash", Command::AbortBash),
        ("get_session_stats", Command::GetSessionStats),
        (
            "export_html",
            Command::ExportHtml {
                output_path: Some("C:/fixtures/session.html".to_owned()),
            },
        ),
        (
            "switch_session",
            Command::SwitchSession {
                session_path: "C:/fixtures/session.jsonl".to_owned(),
            },
        ),
        (
            "fork",
            Command::Fork {
                entry_id: EntryId::from("entry-1"),
            },
        ),
        ("clone", Command::Clone),
        ("get_fork_messages", Command::GetForkMessages),
        (
            "get_entries",
            Command::GetEntries {
                since: Some(EntryId::from("entry-1")),
            },
        ),
        ("get_tree", Command::GetTree),
        ("get_last_assistant_text", Command::GetLastAssistantText),
        (
            "set_session_name",
            Command::SetSessionName {
                name: "Fixture".to_owned(),
            },
        ),
        ("get_messages", Command::GetMessages),
        ("get_commands", Command::GetCommands),
    ]
}

#[test]
fn every_command_serializes_as_one_lf_terminated_record() {
    for (index, (expected_type, command)) in all_commands().into_iter().enumerate() {
        let record = OutboundRecord::Command(RpcCommand::new(format!("request-{index}"), command));
        let bytes = encode_record(&record).expect("synthetic command should serialize");

        assert_eq!(bytes.last(), Some(&b'\n'));
        assert_eq!(bytes.iter().filter(|byte| **byte == b'\n').count(), 1);
        assert_ne!(bytes.get(bytes.len().saturating_sub(2)), Some(&b'\r'));

        let value: Value =
            serde_json::from_slice(&bytes[..bytes.len() - 1]).expect("record should be JSON");
        assert_eq!(value["type"], expected_type);
        assert_eq!(value["id"], format!("request-{index}"));
        if expected_type == "bash" {
            assert_eq!(value["command"], "echo fixture");
            assert_eq!(value["excludeFromContext"], true);
        }
    }
}

#[test]
fn extension_ui_responses_serialize_with_exact_wire_discriminators() {
    let responses = [
        (
            ExtensionUiResponseBody::Value("One".to_owned()),
            json!({"type":"extension_ui_response","id":"ui-1","value":"One"}),
        ),
        (
            ExtensionUiResponseBody::Confirmed(false),
            json!({"type":"extension_ui_response","id":"ui-1","confirmed":false}),
        ),
        (
            ExtensionUiResponseBody::Cancelled,
            json!({"type":"extension_ui_response","id":"ui-1","cancelled":true}),
        ),
    ];

    for (response, expected) in responses {
        let record = OutboundRecord::ExtensionUiResponse(ExtensionUiResponse {
            id: RequestId::from("ui-1"),
            response,
        });
        let bytes = encode_record(&record).expect("response should serialize");
        assert_eq!(bytes.last(), Some(&b'\n'));
        assert_eq!(bytes.iter().filter(|byte| **byte == b'\n').count(), 1);
        let actual: Value = serde_json::from_slice(&bytes[..bytes.len() - 1]).unwrap();
        assert_eq!(actual, expected);
    }
}

fn model() -> Value {
    json!({
        "id":"synthetic-model",
        "name":"Synthetic Model",
        "api":"synthetic-api",
        "provider":"synthetic-provider",
        "baseUrl":"https://invalid.example",
        "reasoning":true,
        "input":["text"],
        "cost":{"input":1.0,"output":2.0,"cacheRead":0.1,"cacheWrite":0.2},
        "contextWindow":200000,
        "maxTokens":8192
    })
}

fn success_response(command: &str, data: Option<Value>) -> Value {
    let mut value = json!({"type":"response","id":format!("response-{command}"),"command":command,"success":true});
    if let Some(data) = data {
        value["data"] = data;
    }
    value
}

#[test]
fn every_installed_response_shape_decodes() {
    let no_data = [
        "prompt",
        "steer",
        "follow_up",
        "abort",
        "set_thinking_level",
        "set_steering_mode",
        "set_follow_up_mode",
        "set_auto_compaction",
        "set_auto_retry",
        "abort_retry",
        "abort_bash",
        "set_session_name",
    ];
    let mut responses: Vec<_> = no_data
        .into_iter()
        .map(|command| success_response(command, None))
        .collect();
    responses.extend([
        success_response("new_session", Some(json!({"cancelled":false}))),
        success_response(
            "get_state",
            Some(json!({
                "model":null,"thinkingLevel":"medium","isStreaming":false,
                "isCompacting":false,"steeringMode":"all","followUpMode":"one-at-a-time",
                "sessionId":"session-1","autoCompactionEnabled":true,
                "messageCount":0,"pendingMessageCount":0
            })),
        ),
        success_response("set_model", Some(model())),
        success_response("cycle_model", Some(Value::Null)),
        success_response("get_available_models", Some(json!({"models":[model()]}))),
        success_response("cycle_thinking_level", Some(json!({"level":"minimal"}))),
        success_response(
            "compact",
            Some(json!({
                "summary":"summary","firstKeptEntryId":"entry-1","tokensBefore":100,
                "estimatedTokensAfter":20,"details":{"synthetic":true}
            })),
        ),
        success_response(
            "bash",
            Some(json!({
                "output":"fixture","cancelled":false,"truncated":true,
                "fullOutputPath":"C:/fixtures/full.txt"
            })),
        ),
        success_response(
            "get_session_stats",
            Some(json!({
                "sessionId":"session-1","userMessages":1,"assistantMessages":1,
                "toolCalls":1,"toolResults":1,"totalMessages":4,
                "tokens":{"input":10,"output":5,"cacheRead":2,"cacheWrite":1,"total":18},
                "cost":0.1,"contextUsage":{"tokens":null,"contextWindow":200000,"percent":null}
            })),
        ),
        success_response(
            "export_html",
            Some(json!({"path":"C:/fixtures/session.html"})),
        ),
        success_response("switch_session", Some(json!({"cancelled":false}))),
        success_response("fork", Some(json!({"text":"prompt","cancelled":false}))),
        success_response("clone", Some(json!({"cancelled":false}))),
        success_response(
            "get_fork_messages",
            Some(json!({"messages":[{"entryId":"entry-1","text":"prompt"}]})),
        ),
        success_response("get_entries", Some(json!({"entries":[],"leafId":null}))),
        success_response("get_tree", Some(json!({"tree":[],"leafId":null}))),
        success_response("get_last_assistant_text", Some(json!({"text":null}))),
        success_response("get_messages", Some(json!({"messages":[]}))),
        success_response("get_commands", Some(json!({"commands":[]}))),
    ]);

    assert_eq!(responses.len(), 31);
    for value in responses {
        let expected_command = value["command"].as_str().unwrap().to_owned();
        let record = IncomingRecord::from_value(value).expect("installed response should decode");
        let IncomingRecord::Response(response) = record else {
            panic!("expected response");
        };
        assert_eq!(response.result.command(), expected_command);
    }

    let failed = IncomingRecord::from_value(json!({
        "type":"response","id":"failure","command":"set_model",
        "success":false,"error":"synthetic model not found","futureField":true
    }))
    .expect("failure should decode");
    assert!(matches!(
        failed,
        IncomingRecord::Response(response)
            if matches!(response.result, ResponseResult::Failure { .. })
    ));

    let future = json!({
        "type":"response","id":"future","command":"future_command","success":true,
        "data":{"value":1},"futureTopLevel":"preserved"
    });
    let decoded = IncomingRecord::from_value(future.clone()).unwrap();
    assert_eq!(serde_json::to_value(decoded).unwrap(), future);
}

#[test]
fn captured_inbound_fixture_decodes_all_record_classes() {
    let fixture = include_bytes!("fixtures/pi_0_80_10_inbound.jsonl");
    let mut codec = JsonlCodec::default();
    let records = codec.feed(fixture).expect("captured fixture should decode");
    assert!(codec.finish().unwrap().is_none());

    assert_eq!(records.len(), 33);
    assert!(records.iter().any(|record| matches!(
        record,
        IncomingRecord::Response(response)
            if matches!(response.result, ResponseResult::GetCommands(_))
    )));
    assert!(records.iter().any(|record| matches!(
        record,
        IncomingRecord::Event(event)
            if matches!(event.as_ref(), RpcEvent::EntryAppended { .. })
    )));
    assert!(records.iter().any(|record| matches!(
        record,
        IncomingRecord::Event(event)
            if matches!(event.as_ref(), RpcEvent::SessionInfoChanged { name: None })
    )));
    assert!(records.iter().any(|record| matches!(
        record,
        IncomingRecord::Event(event)
            if matches!(
                event.as_ref(),
                RpcEvent::ThinkingLevelChanged {
                    level: ThinkingLevel::Xhigh
                }
            )
    )));
    assert_eq!(
        records
            .iter()
            .filter(|record| matches!(record, IncomingRecord::ExtensionUiRequest(_)))
            .count(),
        9
    );
    assert!(
        records
            .iter()
            .any(|record| matches!(record, IncomingRecord::ExtensionError(_)))
    );
    assert!(records.iter().any(|record| matches!(
        record,
        IncomingRecord::UnknownEvent(UnknownEvent { event_type, .. })
            if event_type == "future_runtime_event"
    )));

    let command_source = records.iter().find_map(|record| match record {
        IncomingRecord::Response(response) => match &response.result {
            ResponseResult::GetCommands(data) => data.commands.first(),
            _ => None,
        },
        _ => None,
    });
    let source = &command_source.expect("sourceInfo fixture").source_info;
    assert_eq!(source.scope, SourceScope::Project);
    assert_eq!(source.origin, SourceOrigin::Package);
    assert_eq!(source.base_dir.as_deref(), Some("C:/fixtures"));
}

#[test]
fn every_assistant_streaming_variant_decodes() {
    let assistant = json!({
        "role":"assistant","content":[],"api":"synthetic-api","provider":"synthetic-provider",
        "model":"synthetic-model","usage":{"input":0,"output":0,"cacheRead":0,
        "cacheWrite":0,"totalTokens":0,"cost":{"input":0.0,"output":0.0,
        "cacheRead":0.0,"cacheWrite":0.0,"total":0.0}},"stopReason":"stop","timestamp":1
    });
    let variants = [
        json!({"type":"start","partial":assistant}),
        json!({"type":"text_start","contentIndex":0,"partial":assistant}),
        json!({"type":"text_delta","contentIndex":0,"delta":"x","partial":assistant}),
        json!({"type":"text_end","contentIndex":0,"content":"x","partial":assistant}),
        json!({"type":"thinking_start","contentIndex":0,"partial":assistant}),
        json!({"type":"thinking_delta","contentIndex":0,"delta":"x","partial":assistant}),
        json!({"type":"thinking_end","contentIndex":0,"content":"x","partial":assistant}),
        json!({"type":"toolcall_start","contentIndex":0,"partial":assistant}),
        json!({"type":"toolcall_delta","contentIndex":0,"delta":"{}","partial":assistant}),
        json!({"type":"toolcall_end","contentIndex":0,"toolCall":{"type":"toolCall","id":"tool-1","name":"read","arguments":{}},"partial":assistant}),
        json!({"type":"done","reason":"stop","message":assistant}),
        json!({"type":"error","reason":"aborted","error":assistant}),
    ];

    for assistant_event in variants {
        let record = IncomingRecord::from_value(json!({
            "type":"message_update","message":assistant,
            "assistantMessageEvent":assistant_event
        }))
        .expect("streaming variant should decode");
        assert!(matches!(
            record,
            IncomingRecord::Event(event)
                if matches!(event.as_ref(), RpcEvent::MessageUpdate { .. })
        ));
    }
}

#[test]
fn tool_call_end_round_trips_required_tool_call_discriminator() {
    let assistant = json!({
        "role":"assistant","content":[],"api":"synthetic-api","provider":"synthetic-provider",
        "model":"synthetic-model","usage":{"input":0,"output":0,"cacheRead":0,
        "cacheWrite":0,"totalTokens":0,"cost":{"input":0.0,"output":0.0,
        "cacheRead":0.0,"cacheWrite":0.0,"total":0.0}},"stopReason":"stop","timestamp":1
    });
    let value = json!({
        "type":"message_update",
        "message":assistant,
        "assistantMessageEvent":{
            "type":"toolcall_end","contentIndex":0,
            "toolCall":{"type":"toolCall","id":"tool-1","name":"read","arguments":{}},
            "partial":assistant
        }
    });
    let record = IncomingRecord::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(record).unwrap(), value);
}

#[test]
fn malformed_and_non_object_tool_arguments_remain_decodable() {
    let message = json!({
        "role":"assistant",
        "content":[{
            "type":"toolCall","id":"tool-malformed","name":"custom",
            "arguments":"{not valid json"
        }],
        "api":"synthetic-api","provider":"synthetic-provider","model":"synthetic-model",
        "usage":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0,"totalTokens":0,
        "cost":{"input":0.0,"output":0.0,"cacheRead":0.0,"cacheWrite":0.0,"total":0.0}},
        "stopReason":"toolUse","timestamp":1
    });
    let record = IncomingRecord::from_value(json!({
        "type":"message_end","message":message
    }))
    .expect("scalar arguments must not fault the protocol");
    assert!(matches!(
        record,
        IncomingRecord::Event(event)
            if matches!(event.as_ref(), RpcEvent::MessageEnd { .. })
    ));
}

#[test]
fn every_installed_session_entry_variant_decodes_with_unknown_fallback() {
    let base = |entry_type: &str| json!({"type":entry_type,"id":format!("{entry_type}-id"),"parentId":null,"timestamp":"2026-01-01T00:00:00Z"});
    let mut entries = vec![
        {
            let mut value = base("message");
            value["message"] = json!({"role":"user","content":"hello","timestamp":1});
            value
        },
        {
            let mut value = base("thinking_level_change");
            value["thinkingLevel"] = json!("future-level");
            value
        },
        {
            let mut value = base("model_change");
            value["provider"] = json!("synthetic");
            value["modelId"] = json!("model");
            value
        },
        {
            let mut value = base("compaction");
            value["summary"] = json!("summary");
            value["firstKeptEntryId"] = json!("message-id");
            value["tokensBefore"] = json!(100);
            value
        },
        {
            let mut value = base("branch_summary");
            value["fromId"] = json!("message-id");
            value["summary"] = json!("summary");
            value
        },
        {
            let mut value = base("custom");
            value["customType"] = json!("fixture");
            value
        },
        {
            let mut value = base("custom_message");
            value["customType"] = json!("fixture");
            value["content"] = json!("content");
            value["display"] = json!(true);
            value
        },
        {
            let mut value = base("label");
            value["targetId"] = json!("message-id");
            value
        },
        {
            let mut value = base("session_info");
            value["name"] = json!("Fixture");
            value
        },
    ];
    entries.push(base("future_entry"));

    for (index, value) in entries.into_iter().enumerate() {
        let entry: SessionEntry = serde_json::from_value(value).expect("entry should decode");
        if index == 9 {
            assert!(matches!(entry, SessionEntry::Unknown { .. }));
        } else {
            assert!(matches!(entry, SessionEntry::Known(_)));
        }
    }
}

#[test]
fn codec_handles_lf_crlf_multiple_records_and_fragmented_utf8() {
    let mut codec = JsonlCodec::default();
    let bytes = "{\"type\":\"first\",\"text\":\"€\"}\r\n{\"type\":\"second\"}\n".as_bytes();
    let euro = bytes
        .windows(3)
        .position(|window| window == "€".as_bytes())
        .unwrap();

    assert!(codec.feed(&bytes[..euro + 1]).unwrap().is_empty());
    assert!(codec.feed(&bytes[euro + 1..euro + 2]).unwrap().is_empty());
    let records = codec.feed(&bytes[euro + 2..]).unwrap();
    assert_eq!(records.len(), 2);
    assert!(matches!(
        &records[0],
        IncomingRecord::UnknownEvent(UnknownEvent { event_type, .. }) if event_type == "first"
    ));
}

#[test]
fn codec_preserves_unicode_line_and_paragraph_separators() {
    let input = "{\"type\":\"future\",\"text\":\"left\u{2028}middle\u{2029}right\"}\n";
    let mut codec = JsonlCodec::default();
    let records = codec.feed(input.as_bytes()).unwrap();
    assert_eq!(records.len(), 1);
    let IncomingRecord::UnknownEvent(event) = &records[0] else {
        panic!("expected unknown event");
    };
    assert_eq!(event.raw["text"], "left\u{2028}middle\u{2029}right");
}

#[test]
fn codec_decodes_final_unterminated_record() {
    let mut codec = JsonlCodec::default();
    assert!(codec.feed(b"{\"type\":\"final\"}").unwrap().is_empty());
    assert!(matches!(
        codec.finish().unwrap(),
        Some(IncomingRecord::UnknownEvent(UnknownEvent { event_type, .. })) if event_type == "final"
    ));
}

#[test]
fn codec_reports_blank_malformed_invalid_utf8_and_oversized_frames() {
    let mut blank = JsonlCodec::default();
    assert!(matches!(
        blank.feed(b"\r\n"),
        Err(JsonlDecodeError::BlankFrame { frame: 1 })
    ));

    let mut malformed = JsonlCodec::default();
    let error = malformed.feed(b"{not-json}\n").unwrap_err();
    assert!(matches!(
        error,
        JsonlDecodeError::InvalidRecord { frame: 1, .. }
    ));
    assert!(error.to_string().contains("invalid JSON record"));

    let mut invalid_utf8 = JsonlCodec::default();
    assert!(matches!(
        invalid_utf8.feed(&[b'{', 0xff, b'}', b'\n']),
        Err(JsonlDecodeError::InvalidUtf8 {
            frame: 1,
            valid_up_to: 1,
            ..
        })
    ));

    let mut oversized = JsonlCodec::new(16);
    assert!(matches!(
        oversized.feed(b"{\"type\":\"far-too-large\"}\n"),
        Err(JsonlDecodeError::FrameTooLarge {
            frame: 1,
            max: 16,
            ..
        })
    ));
}

#[test]
fn codec_enforces_the_configured_limit_in_both_directions() {
    let value = json!({"type":"future","payload":"fixture"});
    let payload = serde_json::to_vec(&value).unwrap();

    let codec = JsonlCodec::new(payload.len());
    let encoded = codec.encode(&value).expect("exact-size payload should fit");
    assert_eq!(encoded.len(), payload.len() + 1);

    let too_small = JsonlCodec::new(payload.len() - 1);
    assert!(matches!(
        too_small.encode(&value),
        Err(JsonlEncodeError::FrameTooLarge { .. })
    ));

    let mut decoder = JsonlCodec::new(payload.len());
    let mut crlf_record = payload;
    crlf_record.extend_from_slice(b"\r\n");
    assert_eq!(decoder.feed(&crlf_record).unwrap().len(), 1);
}

#[test]
fn generation_and_epoch_newtypes_do_not_interchange() {
    let generation = ConnectionGeneration::new(4);
    let epoch = SessionEpoch::new(4);
    assert_eq!(generation.next().value(), 5);
    assert_eq!(epoch.next().value(), 5);
    assert_eq!(generation.to_string(), "4");
    assert_eq!(epoch.to_string(), "4");
}
