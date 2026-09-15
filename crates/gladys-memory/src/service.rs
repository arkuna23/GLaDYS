use serde::Deserialize;

use crate::config::Config;
use crate::error::{MemoryError, Result};
use crate::store::Store;
use crate::types::{Conversation, Layer, Memory, Pack};

#[derive(Debug, Clone, Deserialize)]
pub struct WriteParams {
    pub layer: Layer,
    pub text: String,
    #[serde(default)]
    pub channel: Option<String>,
    #[serde(default)]
    pub conversation: Option<Conversation>,
    #[serde(default)]
    pub person: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GetParams {
    pub id: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ForgetParams {
    pub id: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SearchParams {
    pub query: String,
    #[serde(default)]
    pub layer: Option<Layer>,
    #[serde(default)]
    pub channel: Option<String>,
    #[serde(default)]
    pub conversation: Option<Conversation>,
    #[serde(default)]
    pub person: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: u32,
}

fn default_limit() -> u32 {
    50
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct PackParams {
    #[serde(default)]
    pub channel: Option<String>,
    #[serde(default)]
    pub conversation: Option<Conversation>,
    #[serde(default)]
    pub person: Option<String>,
}

#[derive(Clone)]
pub struct Service {
    store: Store,
    pack_limit: u32,
    pack_max_chars: usize,
}

impl Service {
    pub fn new(store: Store, pack_limit: u32, pack_max_chars: usize) -> Self {
        Self {
            store,
            pack_limit,
            pack_max_chars,
        }
    }

    pub fn from_config(cfg: &Config) -> Result<Self> {
        std::fs::create_dir_all(&cfg.data_dir)?;
        let store = Store::open(cfg.sqlite_path())?;
        Ok(Self::new(store, cfg.pack_limit, cfg.pack_max_chars))
    }

    pub fn write(&self, params: WriteParams) -> Result<Memory> {
        let text = params.text.trim();
        if text.is_empty() {
            return Err(MemoryError::Invalid("text required".into()));
        }
        let (channel, conversation, person) = match params.layer {
            Layer::Global => (None, None, None),
            Layer::Conversation => {
                let channel = params
                    .channel
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| MemoryError::Invalid("channel required".into()))?;
                let conversation = params
                    .conversation
                    .ok_or_else(|| MemoryError::Invalid("conversation required".into()))?;
                (Some(channel), Some(conversation), None)
            }
            Layer::Person => {
                let channel = params
                    .channel
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| MemoryError::Invalid("channel required".into()))?;
                let person = params
                    .person
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| MemoryError::Invalid("person required".into()))?;
                (Some(channel), None, Some(person))
            }
        };
        let mem = Memory {
            id: Memory::new_id(),
            layer: params.layer,
            channel,
            conversation,
            person,
            text: text.to_string(),
            ts: now_ts(),
        };
        self.store.insert(&mem)?;
        Ok(mem)
    }

    pub fn get(&self, params: GetParams) -> Result<Memory> {
        self.store.get(&params.id)?.ok_or(MemoryError::NotFound)
    }

    pub fn forget(&self, params: ForgetParams) -> Result<()> {
        if self.store.forget(&params.id)? {
            Ok(())
        } else {
            Err(MemoryError::NotFound)
        }
    }

    pub fn search(&self, params: SearchParams) -> Result<Vec<Memory>> {
        if params.query.trim().is_empty() {
            return Err(MemoryError::Invalid("query required".into()));
        }
        self.store.search(
            &params.query,
            params.layer,
            params.channel.as_deref(),
            params.conversation.as_ref(),
            params.person.as_deref(),
            params.limit,
        )
    }

    pub fn pack(&self, params: PackParams) -> Result<Pack> {
        let limit = self.pack_limit;
        let global = cap_chars(
            self.store
                .list_layer(Layer::Global, None, None, None, limit)?,
            self.pack_max_chars,
        );
        let conversation = if let (Some(channel), Some(conv)) =
            (params.channel.as_deref(), params.conversation.as_ref())
        {
            cap_chars(
                self.store.list_layer(
                    Layer::Conversation,
                    Some(channel),
                    Some(conv),
                    None,
                    limit,
                )?,
                self.pack_max_chars,
            )
        } else {
            Vec::new()
        };
        let person = if let (Some(channel), Some(person)) =
            (params.channel.as_deref(), params.person.as_deref())
        {
            cap_chars(
                self.store
                    .list_layer(Layer::Person, Some(channel), None, Some(person), limit)?,
                self.pack_max_chars,
            )
        } else {
            Vec::new()
        };
        Ok(Pack {
            global,
            conversation,
            person,
        })
    }
}

fn cap_chars(mut items: Vec<Memory>, max: usize) -> Vec<Memory> {
    if max == 0 {
        return Vec::new();
    }
    let mut used = 0;
    let mut out = Vec::new();
    for mut mem in items.drain(..) {
        if used >= max {
            break;
        }
        let remain = max - used;
        if mem.text.chars().count() > remain {
            mem.text = mem.text.chars().take(remain).collect();
        }
        used += mem.text.chars().count();
        out.push(mem);
    }
    out
}

fn now_ts() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ConversationKind;

    fn conv() -> Conversation {
        Conversation {
            kind: ConversationKind::Group,
            peer: "477517182".into(),
        }
    }

    #[test]
    fn layers_pack_search_forget() {
        let svc = Service::new(Store::memory().unwrap(), 20, 4000);
        svc.write(WriteParams {
            layer: Layer::Global,
            text: "always reply in chinese".into(),
            channel: None,
            conversation: None,
            person: None,
        })
        .unwrap();
        svc.write(WriteParams {
            layer: Layer::Conversation,
            text: "this group hates at-all".into(),
            channel: Some("onebot".into()),
            conversation: Some(conv()),
            person: None,
        })
        .unwrap();
        let person = svc
            .write(WriteParams {
                layer: Layer::Person,
                text: "alice likes short answers".into(),
                channel: Some("onebot".into()),
                conversation: None,
                person: Some("10001".into()),
            })
            .unwrap();

        let pack = svc
            .pack(PackParams {
                channel: Some("onebot".into()),
                conversation: Some(conv()),
                person: Some("10001".into()),
            })
            .unwrap();
        assert_eq!(pack.global.len(), 1);
        assert_eq!(pack.conversation.len(), 1);
        assert_eq!(pack.person.len(), 1);

        let other = svc
            .pack(PackParams {
                channel: Some("onebot".into()),
                conversation: Some(Conversation {
                    kind: ConversationKind::Group,
                    peer: "999".into(),
                }),
                person: Some("10002".into()),
            })
            .unwrap();
        assert_eq!(other.global.len(), 1);
        assert!(other.conversation.is_empty());
        assert!(other.person.is_empty());

        let hits = svc
            .search(SearchParams {
                query: "alice".into(),
                layer: Some(Layer::Person),
                channel: Some("onebot".into()),
                conversation: None,
                person: Some("10001".into()),
                limit: 10,
            })
            .unwrap();
        assert_eq!(hits.len(), 1);

        svc.forget(ForgetParams { id: person.id }).unwrap();
        let pack = svc
            .pack(PackParams {
                channel: Some("onebot".into()),
                conversation: Some(conv()),
                person: Some("10001".into()),
            })
            .unwrap();
        assert!(pack.person.is_empty());
        let hits = svc
            .search(SearchParams {
                query: "alice".into(),
                layer: None,
                channel: None,
                conversation: None,
                person: None,
                limit: 10,
            })
            .unwrap();
        assert!(hits.is_empty());
    }
}
