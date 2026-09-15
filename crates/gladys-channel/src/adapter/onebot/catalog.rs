use serde_json::{json, Value};

use crate::capability::{CommonCaps, NativeOp, Profile};

#[derive(Clone, Copy)]
enum Set {
    Public,
    Gocq,
    Napcat,
}

struct Def {
    name: &'static str,
    description: &'static str,
    set: Set,
    required: &'static [&'static str],
    properties: &'static [(&'static str, &'static str)],
}

const OPS: &[Def] = &[
    Def { name: "send_like", description: "send friend likes", set: Set::Public, required: &["user_id"], properties: &[("user_id", "number"), ("times", "number")] },
    Def { name: "set_group_kick", description: "kick a group member", set: Set::Public, required: &["group_id", "user_id"], properties: &[("group_id", "number"), ("user_id", "number"), ("reject_add_request", "boolean")] },
    Def { name: "set_group_ban", description: "mute a group member", set: Set::Public, required: &["group_id", "user_id"], properties: &[("group_id", "number"), ("user_id", "number"), ("duration", "number")] },
    Def { name: "set_group_anonymous_ban", description: "mute an anonymous member", set: Set::Public, required: &["group_id"], properties: &[("group_id", "number"), ("flag", "string"), ("duration", "number")] },
    Def { name: "set_group_whole_ban", description: "mute the whole group", set: Set::Public, required: &["group_id"], properties: &[("group_id", "number"), ("enable", "boolean")] },
    Def { name: "set_group_admin", description: "set group admin", set: Set::Public, required: &["group_id", "user_id"], properties: &[("group_id", "number"), ("user_id", "number"), ("enable", "boolean")] },
    Def { name: "set_group_anonymous", description: "toggle anonymous chat", set: Set::Public, required: &["group_id"], properties: &[("group_id", "number"), ("enable", "boolean")] },
    Def { name: "set_group_card", description: "set group card", set: Set::Public, required: &["group_id", "user_id"], properties: &[("group_id", "number"), ("user_id", "number"), ("card", "string")] },
    Def { name: "set_group_name", description: "set group name", set: Set::Public, required: &["group_id", "group_name"], properties: &[("group_id", "number"), ("group_name", "string")] },
    Def { name: "set_group_leave", description: "leave a group", set: Set::Public, required: &["group_id"], properties: &[("group_id", "number"), ("is_dismiss", "boolean")] },
    Def { name: "set_group_special_title", description: "set special title", set: Set::Public, required: &["group_id", "user_id"], properties: &[("group_id", "number"), ("user_id", "number"), ("special_title", "string")] },
    Def { name: "set_friend_add_request", description: "handle friend request", set: Set::Public, required: &["flag"], properties: &[("flag", "string"), ("approve", "boolean"), ("remark", "string")] },
    Def { name: "set_group_add_request", description: "handle group request", set: Set::Public, required: &["flag"], properties: &[("flag", "string"), ("sub_type", "string"), ("approve", "boolean"), ("reason", "string")] },
    Def { name: "get_login_info", description: "bot login info", set: Set::Public, required: &[], properties: &[] },
    Def { name: "get_stranger_info", description: "stranger info", set: Set::Public, required: &["user_id"], properties: &[("user_id", "number"), ("no_cache", "boolean")] },
    Def { name: "get_friend_list", description: "friend list", set: Set::Public, required: &[], properties: &[] },
    Def { name: "get_group_info", description: "group info", set: Set::Public, required: &["group_id"], properties: &[("group_id", "number"), ("no_cache", "boolean")] },
    Def { name: "get_group_list", description: "group list", set: Set::Public, required: &[], properties: &[] },
    Def { name: "get_group_member_info", description: "group member info", set: Set::Public, required: &["group_id", "user_id"], properties: &[("group_id", "number"), ("user_id", "number"), ("no_cache", "boolean")] },
    Def { name: "get_group_member_list", description: "group member list", set: Set::Public, required: &["group_id"], properties: &[("group_id", "number")] },
    Def { name: "get_group_honor_info", description: "group honor info", set: Set::Public, required: &["group_id"], properties: &[("group_id", "number"), ("type", "string")] },
    Def { name: "get_cookies", description: "cookies", set: Set::Public, required: &[], properties: &[("domain", "string")] },
    Def { name: "get_csrf_token", description: "csrf token", set: Set::Public, required: &[], properties: &[] },
    Def { name: "get_credentials", description: "cookies and csrf", set: Set::Public, required: &[], properties: &[("domain", "string")] },
    Def { name: "get_record", description: "download voice file", set: Set::Public, required: &["file"], properties: &[("file", "string"), ("out_format", "string")] },
    Def { name: "get_image", description: "download image file", set: Set::Public, required: &["file"], properties: &[("file", "string")] },
    Def { name: "can_send_image", description: "whether image send is available", set: Set::Public, required: &[], properties: &[] },
    Def { name: "can_send_record", description: "whether voice send is available", set: Set::Public, required: &[], properties: &[] },
    Def { name: "get_status", description: "runtime status", set: Set::Public, required: &[], properties: &[] },
    Def { name: "get_version_info", description: "implementation version", set: Set::Public, required: &[], properties: &[] },
    Def { name: "set_restart", description: "restart the implementation", set: Set::Public, required: &[], properties: &[("delay", "number")] },
    Def { name: "clean_cache", description: "clean cache files", set: Set::Public, required: &[], properties: &[] },
    Def { name: "get_forward_msg", description: "get merged forward content", set: Set::Public, required: &["id"], properties: &[("id", "string")] },
    Def { name: "delete_friend", description: "delete a friend", set: Set::Gocq, required: &["user_id"], properties: &[("user_id", "number")] },
    Def { name: "mark_msg_as_read", description: "mark a message read", set: Set::Gocq, required: &["message_id"], properties: &[("message_id", "number")] },
    Def { name: "send_group_forward_msg", description: "send group merged forward", set: Set::Gocq, required: &["group_id", "messages"], properties: &[("group_id", "number"), ("messages", "array")] },
    Def { name: "send_private_forward_msg", description: "send private merged forward", set: Set::Gocq, required: &["user_id", "messages"], properties: &[("user_id", "number"), ("messages", "array")] },
    Def { name: "get_group_msg_history", description: "fetch group history from the platform", set: Set::Gocq, required: &["group_id"], properties: &[("group_id", "number"), ("message_seq", "number"), ("count", "number")] },
    Def { name: "ocr_image", description: "OCR an image", set: Set::Gocq, required: &["image"], properties: &[("image", "string")] },
    Def { name: "get_group_system_msg", description: "group system messages", set: Set::Gocq, required: &[], properties: &[] },
    Def { name: "get_essence_msg_list", description: "essence messages", set: Set::Gocq, required: &["group_id"], properties: &[("group_id", "number")] },
    Def { name: "set_essence_msg", description: "set essence message", set: Set::Gocq, required: &["message_id"], properties: &[("message_id", "number")] },
    Def { name: "delete_essence_msg", description: "remove essence message", set: Set::Gocq, required: &["message_id"], properties: &[("message_id", "number")] },
    Def { name: "send_group_sign", description: "group check-in", set: Set::Gocq, required: &["group_id"], properties: &[("group_id", "number")] },
    Def { name: "_send_group_notice", description: "send group notice", set: Set::Gocq, required: &["group_id", "content"], properties: &[("group_id", "number"), ("content", "string")] },
    Def { name: "_get_group_notice", description: "list group notices", set: Set::Gocq, required: &["group_id"], properties: &[("group_id", "number")] },
    Def { name: "upload_group_file", description: "upload a group file", set: Set::Gocq, required: &["group_id", "file", "name"], properties: &[("group_id", "number"), ("file", "string"), ("name", "string"), ("folder", "string")] },
    Def { name: "get_group_root_files", description: "list group root files", set: Set::Gocq, required: &["group_id"], properties: &[("group_id", "number")] },
    Def { name: "get_group_files_by_folder", description: "list group folder files", set: Set::Gocq, required: &["group_id", "folder_id"], properties: &[("group_id", "number"), ("folder_id", "string")] },
    Def { name: "get_group_file_url", description: "group file URL", set: Set::Gocq, required: &["group_id", "file_id"], properties: &[("group_id", "number"), ("file_id", "string"), ("busid", "number")] },
    Def { name: "upload_private_file", description: "upload a private file", set: Set::Gocq, required: &["user_id", "file", "name"], properties: &[("user_id", "number"), ("file", "string"), ("name", "string")] },
    Def { name: "set_msg_emoji_like", description: "react with an emoji", set: Set::Napcat, required: &["message_id", "emoji_id"], properties: &[("message_id", "number"), ("emoji_id", "string")] },
    Def { name: "send_poke", description: "poke in group or DM", set: Set::Napcat, required: &["user_id"], properties: &[("user_id", "number"), ("group_id", "number")] },
    Def { name: "friend_poke", description: "poke a friend", set: Set::Napcat, required: &["user_id"], properties: &[("user_id", "number")] },
    Def { name: "group_poke", description: "poke in a group", set: Set::Napcat, required: &["group_id", "user_id"], properties: &[("group_id", "number"), ("user_id", "number")] },
    Def { name: "send_forward_msg", description: "send merged forward", set: Set::Napcat, required: &["messages"], properties: &[("message_type", "string"), ("user_id", "number"), ("group_id", "number"), ("messages", "array")] },
    Def { name: "get_friend_msg_history", description: "fetch friend history from the platform", set: Set::Napcat, required: &["user_id"], properties: &[("user_id", "string"), ("message_seq", "string"), ("count", "number")] },
    Def { name: "mark_private_msg_as_read", description: "mark private chat read", set: Set::Napcat, required: &["user_id"], properties: &[("user_id", "number")] },
    Def { name: "mark_group_msg_as_read", description: "mark group chat read", set: Set::Napcat, required: &["group_id"], properties: &[("group_id", "number")] },
    Def { name: "set_input_status", description: "set typing status", set: Set::Napcat, required: &["user_id"], properties: &[("user_id", "number"), ("event_type", "number")] },
    Def { name: "get_recent_contact", description: "recent contacts", set: Set::Napcat, required: &[], properties: &[("count", "number")] },
    Def { name: "get_ai_record", description: "AI TTS", set: Set::Napcat, required: &["character", "group_id", "text"], properties: &[("character", "string"), ("group_id", "number"), ("text", "string")] },
    Def { name: "get_ai_characters", description: "AI TTS characters", set: Set::Napcat, required: &["group_id"], properties: &[("group_id", "number"), ("chat_type", "number")] },
    Def { name: "send_group_ai_record", description: "send AI voice in group", set: Set::Napcat, required: &["character", "group_id", "text"], properties: &[("character", "string"), ("group_id", "number"), ("text", "string")] },
    Def { name: "get_file", description: "file info", set: Set::Napcat, required: &["file_id"], properties: &[("file_id", "string")] },
    Def { name: "forward_friend_single_msg", description: "forward one message to a friend", set: Set::Napcat, required: &["user_id", "message_id"], properties: &[("user_id", "number"), ("message_id", "number")] },
    Def { name: "forward_group_single_msg", description: "forward one message to a group", set: Set::Napcat, required: &["group_id", "message_id"], properties: &[("group_id", "number"), ("message_id", "number")] },
    Def { name: "get_friends_with_category", description: "friends grouped by category", set: Set::Napcat, required: &[], properties: &[] },
    Def { name: "set_qq_avatar", description: "set avatar", set: Set::Napcat, required: &["file"], properties: &[("file", "string")] },
    Def { name: "set_online_status", description: "set online status", set: Set::Napcat, required: &["status"], properties: &[("status", "number"), ("ext_status", "number"), ("battery_status", "number")] },
    Def { name: "set_self_longnick", description: "set signature", set: Set::Napcat, required: &["longNick"], properties: &[("longNick", "string")] },
    Def { name: "fetch_emoji_like", description: "list emoji reactions", set: Set::Napcat, required: &[], properties: &[("message_id", "string")] },
    Def { name: "get_group_info_ex", description: "extended group info", set: Set::Napcat, required: &["group_id"], properties: &[("group_id", "number")] },
    Def { name: "_del_group_notice", description: "delete group notice", set: Set::Napcat, required: &["group_id", "notice_id"], properties: &[("group_id", "number"), ("notice_id", "string")] },
    Def { name: "get_group_shut_list", description: "muted members", set: Set::Napcat, required: &["group_id"], properties: &[("group_id", "number")] },
    Def { name: "get_mini_app_ark", description: "sign a mini-app card", set: Set::Napcat, required: &[], properties: &[] },
    Def { name: "set_group_sign", description: "group sign-in", set: Set::Napcat, required: &["group_id"], properties: &[("group_id", "number")] },
];

fn schema(required: &[&str], properties: &[(&str, &str)]) -> Value {
    let mut props = serde_json::Map::new();
    for (name, ty) in properties {
        props.insert((*name).into(), json!({"type": ty}));
    }
    json!({
        "type": "object",
        "properties": props,
        "required": required,
        "additionalProperties": true
    })
}

fn included(set: Set, profile: Profile) -> bool {
    matches!(
        (set, profile),
        (Set::Public, _) | (Set::Gocq | Set::Napcat, Profile::Napcat)
    )
}

pub fn native_ops(profile: Profile) -> Vec<NativeOp> {
    OPS.iter()
        .filter(|d| included(d.set, profile))
        .map(|d| NativeOp {
            name: d.name.into(),
            description: d.description.into(),
            params_schema: schema(d.required, d.properties),
        })
        .collect()
}

pub fn has_native(profile: Profile, op: &str) -> bool {
    OPS.iter()
        .any(|d| d.name == op && included(d.set, profile))
}

pub fn common_caps(profile: Profile) -> CommonCaps {
    match profile {
        Profile::Public => CommonCaps::onebot_public(),
        Profile::Napcat => CommonCaps::full(),
    }
}

pub fn native_message_types(profile: Profile) -> Vec<String> {
    let mut types = vec![
        "face".into(),
        "rps".into(),
        "dice".into(),
        "shake".into(),
        "share".into(),
        "contact".into(),
        "location".into(),
        "music".into(),
        "xml".into(),
        "json".into(),
        "anonymous".into(),
    ];
    if profile == Profile::Napcat {
        types.extend([
            "poke".into(),
            "forward".into(),
            "node".into(),
            "mface".into(),
            "markdown".into(),
            "keyboard".into(),
        ]);
    }
    types
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_hides_napcat_ops() {
        assert!(!has_native(Profile::Public, "send_poke"));
        assert!(has_native(Profile::Napcat, "send_poke"));
        assert!(has_native(Profile::Public, "set_group_kick"));
        let names: Vec<_> = native_ops(Profile::Public)
            .into_iter()
            .map(|o| o.name)
            .collect();
        assert!(!names.iter().any(|n| n == "send_poke"));
        assert!(!common_caps(Profile::Public).react);
        assert!(common_caps(Profile::Napcat).react);
    }
}
