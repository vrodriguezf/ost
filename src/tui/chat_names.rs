//! Account-scoped labels only: no message content or credentials.
use super::sidebar::Chat;
use crate::api::{names::valid_name, ChatNameSource};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Label {
    name: String,
    source: ChatNameSource,
}
#[derive(Default, Serialize, Deserialize)]
pub struct ChatNames {
    labels: BTreeMap<String, Label>,
    #[serde(skip)]
    path: Option<PathBuf>,
    #[serde(skip)]
    dirty: bool,
}
fn account_path(base: &Path, tenant: &str, user: &str) -> PathBuf {
    let digest = Sha256::digest(format!(
        "{}\0{}",
        tenant.to_lowercase(),
        user.to_lowercase()
    ));
    base.join("chat-names").join(format!("{digest:x}.json"))
}
impl ChatNames {
    #[cfg(not(test))]
    pub fn load_for_account(tenant: &str, user: &str) -> Result<Self> {
        let dirs = directories::ProjectDirs::from("com", "teams-cli", "teams-cli")
            .context("No config directory")?;
        Self::load_path(&account_path(dirs.config_dir(), tenant, user))
    }
    fn load_path(path: &Path) -> Result<Self> {
        let mut state: Self = match fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes).context("Invalid chat name cache")?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(e) => return Err(e.into()),
        };
        state.labels.retain(|_, label| {
            valid_name(&label.name) && label.source != ChatNameSource::Identifier
        });
        state.path = Some(path.to_owned());
        Ok(state)
    }
    pub fn restore(&self, chats: &mut [Chat]) {
        for chat in chats {
            if let Some(label) = self.labels.get(&chat.id) {
                if chat.name_source == ChatNameSource::Identifier
                    || (chat.name_source == ChatNameSource::LastSender
                        && label.source != ChatNameSource::LastSender)
                {
                    chat.name.clone_from(&label.name);
                    chat.name_source = label.source;
                }
            }
        }
    }
    pub fn remember(&mut self, chats: &[Chat]) {
        for chat in chats {
            if chat.name_source == ChatNameSource::Identifier || !valid_name(&chat.name) {
                continue;
            }
            let label = Label {
                name: chat.name.clone(),
                source: chat.name_source,
            };
            if self.labels.get(&chat.id) != Some(&label) {
                self.labels.insert(chat.id.clone(), label);
                self.dirty = true;
            }
        }
    }
    pub fn save(&mut self) -> Result<()> {
        if !self.dirty {
            return Ok(());
        }
        let Some(path) = &self.path else {
            return Ok(());
        };
        fs::create_dir_all(path.parent().context("No cache directory")?)?;
        let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let result = (|| -> Result<()> {
            let mut file = options.open(&temporary)?;
            file.write_all(&serde_json::to_vec(self)?)?;
            file.sync_all()?;
            fs::rename(&temporary, path)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(temporary);
        } else {
            self.dirty = false;
        }
        result
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn restart_restores_names_and_accounts_are_isolated() {
        let base = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        let path = account_path(&base, "tenant", "me");
        let mut cache = ChatNames::load_path(&path).unwrap();
        let mut chats = vec![Chat {
            id: "chat".into(),
            name: "Alice".into(),
            name_source: ChatNameSource::Participants,
            is_group: false,
            online: false,
            unread: Default::default(),
        }];
        cache.remember(&chats);
        cache.save().unwrap();
        let restored = ChatNames::load_path(&path).unwrap();
        chats[0].name = "Unknown conversation".into();
        chats[0].name_source = ChatNameSource::Identifier;
        restored.restore(&mut chats);
        assert_eq!(chats[0].name, "Alice");
        assert_ne!(path, account_path(&base, "other", "me"));
        assert_ne!(path, account_path(&base, "tenant", "other"));
        chats[0].name = "New title".into();
        chats[0].name_source = ChatNameSource::Topic;
        restored.restore(&mut chats);
        assert_eq!(chats[0].name, "New title");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        fs::write(&path, "corrupt").unwrap();
        assert!(ChatNames::load_path(&path).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "corrupt");
        fs::remove_dir_all(base).unwrap();
    }
}
