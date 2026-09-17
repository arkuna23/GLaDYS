use crate::types::{ConvKey, ConversationKind, ListMode};

#[derive(Debug, Clone)]
pub struct Policy {
    pub group_mode: ListMode,
    pub dm_mode: ListMode,
    pub groups: Vec<String>,
    pub dms: Vec<String>,
    pub owners: Vec<String>,
}

impl Policy {
    pub fn admit(&self, key: &ConvKey) -> bool {
        let listed = match key.kind {
            ConversationKind::Group => self.groups.iter().any(|k| k == &key.as_list_key()),
            ConversationKind::Dm => self.dms.iter().any(|k| k == &key.as_list_key()),
        };
        let mode = match key.kind {
            ConversationKind::Group => self.group_mode,
            ConversationKind::Dm => self.dm_mode,
        };
        match mode {
            ListMode::Whitelist => listed,
            ListMode::Blacklist => !listed,
        }
    }

    pub fn is_owner(&self, channel: &str, person: &str) -> bool {
        let key = format!("{channel}:{person}");
        self.owners.iter().any(|o| o == &key)
    }
}


fn has_new_cmd(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut from = 0;
    while let Some(pos) = text[from..].find("/new") {
        let at = from + pos;
        if at == 0 || bytes[at - 1].is_ascii_whitespace() || bytes[at - 1] == b']' {
            return true;
        }
        from = at + 1;
    }
    false
}
pub fn parse_slash(text: &str) -> Option<(String, Vec<String>)> {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'/' {
            let ok = i == 0 || bytes[i - 1].is_ascii_whitespace() || bytes[i - 1] == b']';
            if ok {
                let rest = &text[i + 1..];
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
                    .collect();
                if !name.is_empty() {
                    let argv = rest[name.len()..]
                        .split_whitespace()
                        .map(str::to_string)
                        .collect();
                    return Some((name, argv));
                }
            }
        }
        i += 1;
    }
    None
}

pub fn command_invocation(dm: bool, mentioned: bool, text: &str) -> Option<(String, Vec<String>)> {
    let parsed = parse_slash(text)?;
    if dm {
        if text.trim().starts_with('/') {
            Some(parsed)
        } else {
            None
        }
    } else if mentioned {
        Some(parsed)
    } else {
        None
    }
}

pub fn is_new_command(dm: bool, mentioned: bool, owner: bool, text: &str) -> bool {
    if !owner {
        return false;
    }
    let has_cmd = has_new_cmd(text);
    if !has_cmd {
        return false;
    }
    if dm {
        text.trim().starts_with("/new") || has_cmd
    } else {
        mentioned
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Conversation;

    fn key(kind: ConversationKind, peer: &str) -> ConvKey {
        ConvKey::new(
            "onebot",
            &Conversation {
                kind,
                peer: peer.into(),
            },
        )
    }

    #[test]
    fn group_whitelist_dm_blacklist() {
        let p = Policy {
            group_mode: ListMode::Whitelist,
            dm_mode: ListMode::Blacklist,
            groups: vec!["onebot:group:1".into()],
            dms: vec!["onebot:dm:bad".into()],
            owners: vec!["onebot:9".into()],
        };
        assert!(p.admit(&key(ConversationKind::Group, "1")));
        assert!(!p.admit(&key(ConversationKind::Group, "2")));
        assert!(p.admit(&key(ConversationKind::Dm, "ok")));
        assert!(!p.admit(&key(ConversationKind::Dm, "bad")));
        assert!(p.is_owner("onebot", "9"));
        assert!(!p.is_owner("onebot", "8"));
    }

    #[test]
    fn new_command_rules() {
        assert!(is_new_command(true, false, true, "/new"));
        assert!(!is_new_command(true, false, false, "/new"));
        assert!(!is_new_command(false, false, true, "/new please"));
        assert!(is_new_command(false, true, true, "@bot /new"));
        assert!(is_new_command(false, true, true, "[CQ:at,qq=1]/new"));
    }
}
