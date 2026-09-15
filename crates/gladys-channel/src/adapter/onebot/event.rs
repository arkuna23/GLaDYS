use serde_json::Value;

use super::segment::{first_reply_id, segments_to_parts};
use crate::message::{
    Actor, Conversation, Direction, Envelope, Ingress, NativePayload, PlatformEvent, ReplyTo,
};

pub fn parse_frame(account: &str, value: &Value) -> Option<Ingress> {
    if value.get("echo").is_some() && value.get("retcode").is_some() {
        return None;
    }
    let post_type = value.get("post_type")?.as_str()?;
    match post_type {
        "message" | "message_sent" => parse_message(account, value, post_type == "message_sent")
            .map(Ingress::Message),
        "notice" => parse_notice(account, value).map(Ingress::Notice),
        "request" => parse_request(account, value).map(Ingress::Request),
        _ => None,
    }
}

fn parse_message(account: &str, value: &Value, echo: bool) -> Option<Envelope> {
    let message_type = value.get("message_type")?.as_str()?;
    let conversation = match message_type {
        "private" => Conversation::dm(json_id(value.get("user_id")?)?),
        "group" => Conversation::group(json_id(value.get("group_id")?)?),
        _ => return None,
    };
    let sender_obj = value.get("sender").cloned().unwrap_or(Value::Null);
    let sender_id = json_id(value.get("user_id")?)?;
    let sender_name = sender_obj
        .get("card")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .or_else(|| sender_obj.get("nickname").and_then(Value::as_str))
        .map(ToOwned::to_owned);
    let parts = segments_to_parts(value.get("message").unwrap_or(&Value::Null));
    let reply_pid = first_reply_id(&parts);
    Some(Envelope {
        id: Envelope::new_id(),
        channel: "onebot".into(),
        account: account.into(),
        conversation,
        direction: if echo {
            Direction::Out
        } else {
            Direction::In
        },
        sender: Actor {
            id: sender_id,
            name: sender_name,
        },
        ts: value.get("time").and_then(Value::as_i64).unwrap_or(0),
        platform_id: json_id(value.get("message_id")?),
        reply_to: reply_pid.map(|platform_id| ReplyTo {
            id: None,
            platform_id: Some(platform_id),
        }),
        parts,
        native: Some(NativePayload {
            kind: "onebot.message".into(),
            data: value.clone(),
        }),
        recalled: false,
    })
}

fn parse_notice(account: &str, value: &Value) -> Option<PlatformEvent> {
    let notice_type = value
        .get("notice_type")
        .and_then(Value::as_str)
        .unwrap_or("notice");
    let sub = value.get("sub_type").and_then(Value::as_str);
    let kind = match (notice_type, sub) {
        ("notify", Some("poke")) => "poke".into(),
        ("group_recall", _) => "group_recall".into(),
        ("friend_recall", _) => "friend_recall".into(),
        (n, Some(s)) => format!("{n}.{s}"),
        (n, None) => n.to_string(),
    };
    let conversation = if let Some(gid) = value.get("group_id").and_then(json_id) {
        Some(Conversation::group(gid))
    } else {
        value
            .get("user_id")
            .and_then(json_id)
            .map(Conversation::dm)
    };
    Some(PlatformEvent {
        id: Envelope::new_id(),
        channel: "onebot".into(),
        account: account.into(),
        ts: value.get("time").and_then(Value::as_i64).unwrap_or(0),
        kind,
        conversation,
        native: value.clone(),
    })
}

fn parse_request(account: &str, value: &Value) -> Option<PlatformEvent> {
    let request_type = value
        .get("request_type")
        .and_then(Value::as_str)
        .unwrap_or("request");
    Some(PlatformEvent {
        id: Envelope::new_id(),
        channel: "onebot".into(),
        account: account.into(),
        ts: value.get("time").and_then(Value::as_i64).unwrap_or(0),
        kind: format!("request.{request_type}"),
        conversation: None,
        native: value.clone(),
    })
}

pub fn json_id(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(ToOwned::to_owned)
        .or_else(|| value.as_i64().map(|n| n.to_string()))
        .or_else(|| value.as_u64().map(|n| n.to_string()))
}

pub fn recalled_platform_id(ev: &PlatformEvent) -> Option<String> {
    if ev.kind == "group_recall" || ev.kind == "friend_recall" {
        ev.native.get("message_id").and_then(json_id)
    } else {
        None
    }
}

pub fn envelope_from_get_msg(account: &str, data: &Value) -> Option<Envelope> {
    let mut v = data.clone();
    if v.get("post_type").is_none()
        && let Some(obj) = v.as_object_mut()
    {
        obj.insert("post_type".into(), Value::String("message".into()));
    }
    parse_message(account, &v, false)
}
