use serde_json::{json, Value};

use crate::message::{DroppedPart, MentionTarget, Part};

const PASSTHROUGH: &[&str] = &[
    "face", "rps", "dice", "shake", "poke", "share", "contact", "location", "music", "xml",
    "json", "anonymous", "forward", "node", "markdown", "mface", "keyboard", "image", "record",
    "video", "file", "at", "text", "reply",
];

pub fn segments_to_parts(message: &Value) -> Vec<Part> {
    match message {
        Value::String(s) => vec![Part::text(s.clone())],
        Value::Array(items) => items.iter().filter_map(segment_to_part).collect(),
        _ => Vec::new(),
    }
}

fn segment_to_part(seg: &Value) -> Option<Part> {
    let ty = seg.get("type")?.as_str()?;
    let data = seg.get("data").cloned().unwrap_or(Value::Null);
    Some(match ty {
        "text" => Part::Text {
            text: data
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        },
        "at" => {
            let qq = data
                .get("qq")
                .and_then(|v| v.as_str().map(ToOwned::to_owned).or_else(|| v.as_i64().map(|n| n.to_string())))
                .unwrap_or_default();
            if qq == "all" {
                Part::Mention {
                    target: MentionTarget::All,
                    id: None,
                }
            } else {
                Part::Mention {
                    target: MentionTarget::User,
                    id: Some(qq),
                }
            }
        }
        "image" => Part::Image {
            blob_id: None,
            url: str_field(&data, "url"),
            mime: None,
            filename: str_field(&data, "file"),
        },
        "record" => Part::Audio {
            blob_id: None,
            url: str_field(&data, "url"),
            mime: None,
            filename: str_field(&data, "file"),
        },
        "video" => Part::Video {
            blob_id: None,
            url: str_field(&data, "url"),
            mime: None,
            filename: str_field(&data, "file"),
        },
        "file" => Part::File {
            blob_id: None,
            url: str_field(&data, "url"),
            mime: None,
            filename: str_field(&data, "name").or_else(|| str_field(&data, "file")),
        },
        "reply" => Part::Reply {
            platform_id: str_field(&data, "id").unwrap_or_default(),
        },
        other => Part::Unknown {
            native_type: other.to_string(),
            data,
        },
    })
}

fn str_field(data: &Value, key: &str) -> Option<String> {
    data.get(key).and_then(|v| {
        v.as_str()
            .map(ToOwned::to_owned)
            .or_else(|| v.as_i64().map(|n| n.to_string()))
    })
}

pub fn parts_to_segments(parts: &[Part]) -> (Vec<Value>, Vec<DroppedPart>) {
    let mut segs = Vec::new();
    let mut dropped = Vec::new();
    for part in parts {
        match part {
            Part::Text { text } => segs.push(json!({"type": "text", "data": {"text": text}})),
            Part::Mention {
                target: MentionTarget::All,
                ..
            } => segs.push(json!({"type": "at", "data": {"qq": "all"}})),
            Part::Mention {
                target: MentionTarget::User,
                id,
            } => {
                let qq = id.clone().unwrap_or_default();
                segs.push(json!({"type": "at", "data": {"qq": qq}}));
            }
            Part::Image {
                url,
                filename,
                blob_id,
                ..
            } => {
                let file = url
                    .clone()
                    .or_else(|| filename.clone())
                    .or_else(|| blob_id.clone())
                    .unwrap_or_default();
                segs.push(json!({"type": "image", "data": {"file": file}}));
            }
            Part::Audio {
                url,
                filename,
                blob_id,
                ..
            } => {
                let file = url
                    .clone()
                    .or_else(|| filename.clone())
                    .or_else(|| blob_id.clone())
                    .unwrap_or_default();
                segs.push(json!({"type": "record", "data": {"file": file}}));
            }
            Part::Video {
                url,
                filename,
                blob_id,
                ..
            } => {
                let file = url
                    .clone()
                    .or_else(|| filename.clone())
                    .or_else(|| blob_id.clone())
                    .unwrap_or_default();
                segs.push(json!({"type": "video", "data": {"file": file}}));
            }
            Part::File {
                url,
                filename,
                blob_id,
                ..
            } => {
                let file = url
                    .clone()
                    .or_else(|| filename.clone())
                    .or_else(|| blob_id.clone())
                    .unwrap_or_default();
                segs.push(json!({"type": "file", "data": {"file": file, "name": filename}}));
            }
            Part::Reply { platform_id } => {
                segs.push(json!({"type": "reply", "data": {"id": platform_id}}));
            }
            Part::Unknown { native_type, data } => {
                if PASSTHROUGH.contains(&native_type.as_str()) {
                    segs.push(json!({"type": native_type, "data": data}));
                } else {
                    dropped.push(DroppedPart {
                        reason: "unknown native type".into(),
                        native_type: Some(native_type.clone()),
                    });
                }
            }
        }
    }
    (segs, dropped)
}

pub fn first_reply_id(parts: &[Part]) -> Option<String> {
    parts.iter().find_map(|p| match p {
        Part::Reply { platform_id } if !platform_id.is_empty() => Some(platform_id.clone()),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inbound_common_and_unknown_segments() {
        let raw = json!([
            {"type": "reply", "data": {"id": "455"}},
            {"type": "at", "data": {"qq": "123"}},
            {"type": "text", "data": {"text": "hi"}},
            {"type": "image", "data": {"url": "https://x/a.jpg", "file": "a.jpg"}},
            {"type": "face", "data": {"id": "32"}},
            {"type": "json", "data": {"data": "{}"}}
        ]);
        let parts = segments_to_parts(&raw);
        assert!(matches!(&parts[0], Part::Reply { platform_id } if platform_id == "455"));
        assert!(matches!(
            &parts[1],
            Part::Mention { target: MentionTarget::User, id: Some(id) } if id == "123"
        ));
        assert!(matches!(&parts[2], Part::Text { text } if text == "hi"));
        assert!(matches!(&parts[3], Part::Image { url: Some(u), .. } if u == "https://x/a.jpg"));
        assert!(matches!(&parts[4], Part::Unknown { native_type, .. } if native_type == "face"));
        assert!(matches!(&parts[5], Part::Unknown { native_type, .. } if native_type == "json"));
    }

    #[test]
    fn outbound_roundtrip_and_passthrough() {
        let parts = vec![
            Part::Reply {
                platform_id: "455".into(),
            },
            Part::Mention {
                target: MentionTarget::User,
                id: Some("123".into()),
            },
            Part::text("hi"),
            Part::Image {
                blob_id: None,
                url: Some("https://x/a.jpg".into()),
                mime: None,
                filename: None,
            },
            Part::Unknown {
                native_type: "face".into(),
                data: json!({"id": "32"}),
            },
            Part::Unknown {
                native_type: "not-a-segment".into(),
                data: json!({}),
            },
        ];
        let (segs, dropped) = parts_to_segments(&parts);
        assert_eq!(dropped.len(), 1);
        assert_eq!(dropped[0].native_type.as_deref(), Some("not-a-segment"));
        assert_eq!(segs[0]["type"], "reply");
        assert_eq!(segs[1]["type"], "at");
        assert_eq!(segs[2]["data"]["text"], "hi");
        assert_eq!(segs[3]["data"]["file"], "https://x/a.jpg");
        assert_eq!(segs[4]["type"], "face");
        let back = segments_to_parts(&Value::Array(segs));
        assert!(matches!(&back[2], Part::Text { text } if text == "hi"));
    }
}
