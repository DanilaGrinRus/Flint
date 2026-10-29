//! Messenger operations shared by the Slint shell.
//!
//! The transport is selected per call: a local Tor gateway when this wallet
//! publishes an onion, otherwise the configured relay (SOCKS for `.onion`).
//! History remains local and encrypted; the key itself is managed by
//! `history.rs`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use base64::{decode as b64_decode, encode as b64_encode};
use chacha20poly1305::aead::{Aead, NewAead, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use flint_core::WalletService;
use flint_messenger::{
    Address, ContactBook, DirectOnionTransport, Direction, Envelope, GatewayTransport, History,
    Identity, MessageKind, MessageRecord, Messenger, Transport, DEFAULT_TTL_SECS,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::WalletRuntime;

/// A message that was sealed and persisted before its transport delivery. The
/// envelope is stored verbatim so a retry never re-signs it and the relay can
/// deduplicate by envelope id.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct OutboxEntry {
    envelope: Envelope,
    recipient: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    kind: Option<MessageKind>,
    #[serde(default)]
    kind_nonce: String,
    #[serde(default)]
    kind_ciphertext: String,
    created_at: u64,
}

impl OutboxEntry {
    fn plaintext_kind(&self, key: &[u8; 32]) -> Result<MessageKind, String> {
        if let Some(kind) = &self.kind {
            return Ok(kind.clone());
        }
        let nonce = b64_decode(&self.kind_nonce).map_err(|e| format!("outbox nonce: {e}"))?;
        let ciphertext =
            b64_decode(&self.kind_ciphertext).map_err(|e| format!("outbox kind: {e}"))?;
        if nonce.len() != 12 {
            return Err("outbox kind: неверная nonce".into());
        }
        let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
        let plaintext = cipher
            .decrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &ciphertext,
                    aad: self.envelope.id.as_bytes(),
                },
            )
            .map_err(|_| "outbox kind: не удалось расшифровать".to_string())?;
        serde_json::from_slice(&plaintext).map_err(|e| format!("outbox kind JSON: {e}"))
    }

    fn encrypt_kind(&mut self, key: &[u8; 32]) -> Result<(), String> {
        let plaintext = serde_json::to_vec(
            self.kind
                .as_ref()
                .ok_or_else(|| "outbox kind: отсутствует".to_string())?,
        )
        .map_err(|e| format!("outbox kind JSON: {e}"))?;
        let mut nonce = [0u8; 12];
        getrandom::getrandom(&mut nonce).map_err(|e| format!("outbox nonce RNG: {e}"))?;
        let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
        self.kind_ciphertext = b64_encode(
            cipher
                .encrypt(
                    Nonce::from_slice(&nonce),
                    Payload {
                        msg: &plaintext,
                        aad: self.envelope.id.as_bytes(),
                    },
                )
                .map_err(|_| "outbox kind: шифрование не удалось".to_string())?,
        );
        self.kind_nonce = b64_encode(nonce);
        self.kind = None;
        Ok(())
    }
}

/// Incoming armor persisted before automation touches it. This is the
/// crash-recovery counterpart of the outgoing outbox: a received message can
/// be retried after a UI/process restart instead of disappearing because it is
/// already present in the encrypted history.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct InboxEntry {
    id: String,
    from: String,
    armor: String,
    created_at: u64,
    state: String,
    #[serde(default)]
    reply_id: Option<String>,
    #[serde(default)]
    last_error: Option<String>,
}

enum AppTransport {
    Relay(GatewayTransport),
    Direct(DirectOnionTransport),
}

impl Transport for AppTransport {
    fn deliver(&self, envelope: &Envelope) -> flint_messenger::Result<()> {
        match self {
            Self::Relay(transport) => transport.deliver(envelope),
            Self::Direct(transport) => transport.deliver(envelope),
        }
    }

    fn fetch(
        &self,
        recipient: &Address,
        now: u64,
        limit: usize,
    ) -> flint_messenger::Result<Vec<Envelope>> {
        match self {
            Self::Relay(transport) => transport.fetch(recipient, now, limit),
            Self::Direct(transport) => transport.fetch(recipient, now, limit),
        }
    }

    fn ack(&self, recipient: &Address, ids: &[String]) -> flint_messenger::Result<usize> {
        match self {
            Self::Relay(transport) => transport.ack(recipient, ids),
            Self::Direct(transport) => transport.ack(recipient, ids),
        }
    }

    fn ack_signed(&self, recipient: &Identity, ids: &[String]) -> flint_messenger::Result<usize> {
        match self {
            Self::Relay(transport) => transport.ack_signed(recipient, ids),
            Self::Direct(transport) => transport.ack_signed(recipient, ids),
        }
    }
}

fn load_quarantine(path: &Path) -> Result<HashMap<String, u64>, String> {
    if !path.exists() {
        return Ok(HashMap::new());
    }
    let bytes = std::fs::read(path)
        .map_err(|e| format!("quarantine: не удалось прочитать {}: {e}", path.display()))?;
    serde_json::from_slice(&bytes)
        .map_err(|e| format!("quarantine: повреждён {}: {e}", path.display()))
}

fn save_quarantine(path: &Path, entries: &HashMap<String, u64>) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "quarantine: нет родительского каталога".to_string())?;
    std::fs::create_dir_all(parent)
        .map_err(|e| format!("quarantine: не удалось создать {}: {e}", parent.display()))?;
    let tmp = path.with_extension(format!("json.tmp-{}", std::process::id()));
    let bytes = serde_json::to_vec(entries)
        .map_err(|e| format!("quarantine: не удалось сериализовать: {e}"))?;
    std::fs::write(&tmp, bytes)
        .map_err(|e| format!("quarantine: не удалось записать {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("quarantine: не удалось заменить {}: {e}", path.display())
    })
}

fn load_outbox(path: &Path) -> Result<Vec<OutboxEntry>, String> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let bytes = std::fs::read(path)
        .map_err(|e| format!("outbox: не удалось прочитать {}: {e}", path.display()))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("outbox: повреждён {}: {e}", path.display()))
}

fn save_outbox(path: &Path, entries: &[OutboxEntry]) -> Result<(), String> {
    if entries.is_empty() {
        let _ = std::fs::remove_file(path);
        return Ok(());
    }
    let parent = path
        .parent()
        .ok_or_else(|| "outbox: нет родительского каталога".to_string())?;
    std::fs::create_dir_all(parent)
        .map_err(|e| format!("outbox: не удалось создать {}: {e}", parent.display()))?;
    let bytes = serde_json::to_vec(entries)
        .map_err(|e| format!("outbox: не удалось сериализовать: {e}"))?;
    flint_core::win_acl::write_private_atomic(path, &bytes)
        .map_err(|e| format!("outbox: не удалось записать {}: {e}", path.display()))
}

fn keep_unfinished_reply(entry: &InboxEntry, outbox: &[OutboxEntry]) -> bool {
    entry
        .reply_id
        .as_deref()
        .is_none_or(|reply_id| outbox.iter().any(|item| item.envelope.id == reply_id))
}

fn load_inbox(path: &Path) -> Result<Vec<InboxEntry>, String> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let bytes = std::fs::read(path)
        .map_err(|e| format!("inbox queue: не удалось прочитать {}: {e}", path.display()))?;
    serde_json::from_slice(&bytes)
        .map_err(|e| format!("inbox queue: повреждён {}: {e}", path.display()))
}

fn save_inbox(path: &Path, entries: &[InboxEntry]) -> Result<(), String> {
    if entries.is_empty() {
        let _ = std::fs::remove_file(path);
        return Ok(());
    }
    let parent = path
        .parent()
        .ok_or_else(|| "inbox queue: нет родительского каталога".to_string())?;
    std::fs::create_dir_all(parent)
        .map_err(|e| format!("inbox queue: не удалось создать {}: {e}", parent.display()))?;
    let bytes = serde_json::to_vec(entries)
        .map_err(|e| format!("inbox queue: не удалось сериализовать: {e}"))?;
    flint_core::win_acl::write_private_atomic(path, &bytes)
        .map_err(|e| format!("inbox queue: не удалось записать {}: {e}", path.display()))
}

fn relay_transport(relay: &str, socks: Option<String>) -> Result<GatewayTransport, String> {
    if relay.contains(".onion") {
        let socks = socks
            .or_else(|| std::env::var("FLINT_TOR_SOCKS").ok())
            .unwrap_or_else(|| "127.0.0.1:9050".to_string());
        GatewayTransport::with_socks(relay, &socks).map_err(err)
    } else {
        GatewayTransport::new(relay).map_err(err)
    }
}

fn app_transport(
    relay: &str,
    direct: Option<(String, String)>,
    relay_socks: Option<String>,
) -> Result<AppTransport, String> {
    if let Some((local, socks)) = direct {
        return DirectOnionTransport::new(&local, &socks)
            .map(AppTransport::Direct)
            .map_err(err);
    }
    relay_transport(relay, relay_socks).map(AppTransport::Relay)
}

impl WalletRuntime {
    fn messenger_backend(&self) -> &'static str {
        if self.is_rpc() {
            "grin-wallet"
        } else {
            "embedded"
        }
    }

    fn messenger_profile_dir(&self, wallet: &WalletService, kind: &str) -> PathBuf {
        let profile = wallet
            .settings()
            .wallet_name
            .clone()
            .unwrap_or_else(|| "default".to_string());
        wallet
            .paths()
            .root()
            .join("messenger")
            .join(kind)
            .join(&wallet.settings().network)
            .join(self.messenger_backend())
            .join(wallet.wallet_identity())
            .join(profile)
    }

    fn messenger_history_dir(&self, wallet: &WalletService) -> PathBuf {
        self.messenger_profile_dir(wallet, "history")
    }

    fn messenger_contacts_dir(&self, wallet: &WalletService) -> PathBuf {
        self.messenger_profile_dir(wallet, "contacts")
    }

    fn messenger_blocked_path(&self, wallet: &WalletService) -> PathBuf {
        self.messenger_profile_dir(wallet, "blocked")
            .join("blocked.json")
    }

    fn messenger_quarantine_path(&self, wallet: &WalletService) -> PathBuf {
        self.messenger_profile_dir(wallet, "quarantine")
            .join("quarantine.json")
    }

    fn messenger_outbox_path(&self, wallet: &WalletService) -> PathBuf {
        self.messenger_profile_dir(wallet, "outbox")
            .join("outbox.json")
    }

    fn messenger_inbox_path(&self, wallet: &WalletService) -> PathBuf {
        self.messenger_profile_dir(wallet, "inbox")
            .join("inbox.json")
    }

    /// Retry persisted outgoing envelopes. The envelope itself is reused, so a
    /// crash or transport failure cannot create a second signature/id. Entries
    /// leave the queue only after delivery and history recording both succeed.
    fn drain_outbox(
        &self,
        transport: &AppTransport,
        history: &History,
        path: &Path,
        key: &[u8; 32],
        entries: &mut Vec<OutboxEntry>,
    ) -> Vec<String> {
        let mut errors = Vec::new();
        entries.retain(|entry| {
            if let Err(error) = transport.deliver(&entry.envelope) {
                errors.push(format!("outbox {}: {error}", entry.envelope.id));
                return true;
            }
            let recipient = match entry.recipient.parse() {
                Ok(recipient) => recipient,
                Err(error) => {
                    errors.push(format!("outbox {}: {error}", entry.envelope.id));
                    return true;
                }
            };
            let kind = match entry.plaintext_kind(key) {
                Ok(kind) => kind,
                Err(error) => {
                    errors.push(format!("outbox {}: {error}", entry.envelope.id));
                    return true;
                }
            };
            if let Err(error) = history.record(
                &recipient,
                &MessageRecord {
                    id: entry.envelope.id.clone(),
                    direction: Direction::Outgoing,
                    kind,
                    created_at: entry.created_at,
                    expires_at: Some(entry.created_at.saturating_add(entry.envelope.ttl)),
                },
            ) {
                errors.push(format!("outbox history {}: {error}", entry.envelope.id));
                return true;
            }
            false
        });
        if let Err(error) = save_outbox(path, entries) {
            errors.push(format!("outbox save: {error}"));
        }
        errors
    }

    fn messenger_direct_endpoint(&self) -> Option<(String, String)> {
        let tor = self.tor();
        Some((tor.gateway_url()?, tor.socks_addr()?))
    }

    fn messenger_identity(&self) -> Result<Identity, String> {
        let network = {
            let wallet = self
                .wallet
                .lock()
                .map_err(|_| "кошелёк занят".to_string())?;
            wallet.settings().network.clone()
        };
        let hrp = if network == "mainnet" {
            "grin"
        } else {
            "tgrin"
        };
        let seed = self.messenger_seed()?;
        Identity::from_seed(seed, hrp).map_err(err)
    }

    fn messenger_network(&self) -> Result<(String, String, PathBuf), String> {
        let wallet = self
            .wallet
            .lock()
            .map_err(|_| "кошелёк занят".to_string())?;
        Ok((
            wallet.settings().network.clone(),
            wallet.settings().relay_url.trim().to_string(),
            self.messenger_history_dir(&wallet),
        ))
    }

    pub fn messenger_status(&self) -> Result<Value, String> {
        let (network, relay) = {
            let wallet = self
                .wallet
                .lock()
                .map_err(|_| "кошелёк занят".to_string())?;
            (
                wallet.settings().network.clone(),
                wallet.settings().relay_url.clone(),
            )
        };
        let legacy_history = self.legacy_history_view().unwrap_or_default();
        let (address, status_error, unlocked) = match self.messenger_identity() {
            Ok(identity) => (Some(identity.address().to_string()), None, true),
            Err(error) => (None, Some(error), false),
        };
        let onion = self.tor().onion();
        let direct = onion.is_some() && relay.trim().is_empty();
        Ok(json!({
            "address": address,
            "network": network,
            "relay_url": relay,
            "onion": onion,
            "direct": direct,
            "history_key": self.history_key_ready(),
            "history_error": self.history_error(),
            "legacy_history": legacy_history,
            "unlocked": unlocked,
            "error": status_error,
        }))
    }

    pub fn messenger_send(&self, recipient: &str, text: &str) -> Result<Value, String> {
        let _op = self
            .messenger_op
            .lock()
            .map_err(|_| "messenger занят".to_string())?;
        let recipient: Address = recipient.trim().parse().map_err(err)?;
        let identity = self.messenger_identity()?;
        let (_, relay, dir) = self.messenger_network()?;
        let direct = if relay.is_empty() {
            self.messenger_direct_endpoint()
        } else {
            None
        };
        let relay_socks = if !relay.is_empty() && relay.contains(".onion") {
            self.tor().socks_addr()
        } else {
            None
        };
        if relay.is_empty() && direct.is_none() {
            return Err(
                "relay не настроен: укажите gateway/relay или включите «Приём по Tor» для прямой доставки на onion получателя"
                    .into(),
            );
        }
        let key = self.cached_history_key()?;
        let transport = app_transport(&relay, direct, relay_socks)?;
        let outbox_path = {
            let wallet = self
                .wallet
                .lock()
                .map_err(|_| "кошелёк занят".to_string())?;
            self.messenger_outbox_path(&wallet)
        };
        let history = History::open(dir, key).map_err(err)?;
        let mut outbox = load_outbox(&outbox_path)?;
        let outbox_errors =
            self.drain_outbox(&transport, &history, &outbox_path, &key, &mut outbox);
        let kind = MessageKind::Text {
            text: text.to_string(),
        };
        let envelope = Envelope::seal_now(&identity, &recipient, kind.clone(), DEFAULT_TTL_SECS)
            .map_err(err)?;
        let now = flint_messenger::envelope::unix_now();
        let mut entry = OutboxEntry {
            envelope: envelope.clone(),
            recipient: recipient.to_string(),
            kind: Some(kind.clone()),
            kind_nonce: String::new(),
            kind_ciphertext: String::new(),
            created_at: now,
        };
        entry.encrypt_kind(&key)?;
        outbox.push(entry);
        // Persist the sealed envelope before delivery: a crash between these
        // two lines is retried with the same id, not re-signed.
        save_outbox(&outbox_path, &outbox)?;
        if let Err(error) = transport.deliver(&envelope) {
            return Ok(json!({
                "id": envelope.id,
                "queued": true,
                "queue_error": error.to_string(),
                "outbox_errors": outbox_errors,
            }));
        }
        if let Err(error) = history.record(
            &recipient,
            &MessageRecord {
                id: envelope.id.clone(),
                direction: Direction::Outgoing,
                kind,
                created_at: now,
                expires_at: Some(now.saturating_add(DEFAULT_TTL_SECS)),
            },
        ) {
            // The envelope is already delivered and remains in the outbox for
            // history reconciliation; do not make the caller retry the send
            // and create a second signed response.
            return Ok(json!({
                "id": envelope.id,
                "queued": true,
                "history_error": error.to_string(),
                "outbox_errors": outbox_errors,
            }));
        }
        outbox.retain(|entry| entry.envelope.id != envelope.id);
        save_outbox(&outbox_path, &outbox)?;
        Ok(json!({"id": envelope.id, "outbox_errors": outbox_errors}))
    }

    pub fn messenger_inbox(&self) -> Result<Value, String> {
        let _op = self
            .messenger_op
            .lock()
            .map_err(|_| "messenger занят".to_string())?;
        let identity = self.messenger_identity()?;
        let (_, relay, dir) = self.messenger_network()?;
        let (blocked, quarantine_path, outbox_path, inbox_path) = {
            let wallet = self
                .wallet
                .lock()
                .map_err(|_| "кошелёк занят".to_string())?;
            (
                load_blocked(&self.messenger_blocked_path(&wallet))?,
                self.messenger_quarantine_path(&wallet),
                self.messenger_outbox_path(&wallet),
                self.messenger_inbox_path(&wallet),
            )
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or_default();
        let mut quarantine = load_quarantine(&quarantine_path)?;
        quarantine.retain(|_, expires_at| *expires_at >= now);
        let skip: HashSet<String> = quarantine.keys().cloned().collect();
        let direct = if relay.is_empty() {
            self.messenger_direct_endpoint()
        } else {
            None
        };
        let relay_socks = if !relay.is_empty() && relay.contains(".onion") {
            self.tor().socks_addr()
        } else {
            None
        };
        if relay.is_empty() && direct.is_none() {
            return Err(
                "relay не настроен: укажите gateway/relay или включите «Приём по Tor» для прямой доставки"
                    .into(),
            );
        }
        let key = self.cached_history_key()?;
        let drain_transport = app_transport(&relay, direct.clone(), relay_socks.clone())?;
        let transport = app_transport(&relay, direct, relay_socks)?;
        let messenger = Messenger::new(transport);
        let batch = messenger
            .fetch_open_skipping(&identity, 20, &skip)
            .map_err(err)?;
        let history = History::open(dir, key).map_err(err)?;
        let mut inbox_queue = load_inbox(&inbox_path)?;
        let mut outbox = load_outbox(&outbox_path)?;
        let outbox_errors =
            self.drain_outbox(&drain_transport, &history, &outbox_path, &key, &mut outbox);
        // A queue task whose reply is still queued must not be re-signed; once
        // the reply envelope is gone (delivered and committed), retire the task.
        let before_reply_cleanup = inbox_queue.len();
        inbox_queue.retain(|entry| keep_unfinished_reply(entry, &outbox));
        if inbox_queue.len() != before_reply_cleanup {
            save_inbox(&inbox_path, &inbox_queue)?;
        }
        let mut ack_ids = batch.duplicates;
        let mut items = Vec::new();
        let mut blocked_count = 0usize;
        for message in batch.opened {
            let sender = message.sender.to_string();
            if blocked.iter().any(|entry| entry == &sender) {
                blocked_count += 1;
                ack_ids.push(message.id);
                continue;
            }
            // Persist the armor task before the history tombstone. A crash
            // between these writes then leaves a recoverable queue entry
            // instead of a known message with no automation task.
            if let MessageKind::Text { text } = &message.kind {
                let trimmed = text.trim();
                if trimmed.starts_with("BEGINSLATEPACK.")
                    && trimmed.ends_with("ENDSLATEPACK.")
                    && !inbox_queue.iter().any(|entry| entry.id == message.id)
                {
                    inbox_queue.push(InboxEntry {
                        id: message.id.clone(),
                        from: message.sender.to_string(),
                        armor: text.clone(),
                        created_at: message.created_at,
                        state: "received".into(),
                        reply_id: None,
                        last_error: None,
                    });
                    save_inbox(&inbox_path, &inbox_queue)?;
                }
            }
            if history
                .contains(&message.sender, &message.id)
                .map_err(err)?
            {
                ack_ids.push(message.id);
                continue;
            }
            history
                .record(
                    &message.sender,
                    &MessageRecord {
                        id: message.id.clone(),
                        direction: Direction::Incoming,
                        kind: message.kind.clone(),
                        created_at: message.created_at,
                        expires_at: Some(message.created_at.saturating_add(DEFAULT_TTL_SECS)),
                    },
                )
                .map_err(err)?;
            ack_ids.push(message.id.clone());
            items.push(json!({
                "id": message.id,
                "from": message.sender.to_string(),
                "kind": message.kind,
                "created_at": message.created_at,
            }));
        }
        save_inbox(&inbox_path, &inbox_queue)?;
        if self.auto_process_slatepacks() {
            let known: HashSet<String> = items
                .iter()
                .filter_map(|item| item["id"].as_str().map(str::to_string))
                .collect();
            for entry in &inbox_queue {
                if entry.state == "done" || known.contains(&entry.id) {
                    continue;
                }
                items.push(json!({
                    "id": entry.id,
                    "from": entry.from,
                    "kind": { "type": "text", "text": entry.armor },
                    "created_at": entry.created_at,
                    "queued": true,
                    "state": entry.state,
                    "reply_id": entry.reply_id,
                }));
            }
        }
        // Durable decision first: a broken envelope is tombstoned locally,
        // then its signed ack is sent. A crash before the rename leaves it in
        // the mailbox and it will be reported again.
        for failed in &batch.failed {
            quarantine.insert(failed.id.clone(), failed.expires_at.max(now));
            ack_ids.push(failed.id.clone());
        }
        if !batch.failed.is_empty() {
            save_quarantine(&quarantine_path, &quarantine)?;
        }
        // Retry acks for tombstones whose previous ack was lost.
        ack_ids.extend(skip);
        ack_ids.sort();
        ack_ids.dedup();
        // A failed mailbox ack must not hide opened messages from auto-processing.
        // History is the durable dedup record; the next poll retries the ack and
        // skips the already-known envelope without losing the first delivery.
        let ack_result = messenger.ack_signed(&identity, &ack_ids).map_err(err);
        let ack_error = ack_result.err();
        Ok(json!({
            "messages": items,
            "blocked": blocked_count,
            "failed": batch.failed.len(),
            "failed_ids": batch.failed.iter().map(|f| f.id.clone()).collect::<Vec<_>>(),
            "ack_error": ack_error,
            "outbox_errors": outbox_errors,
        }))
    }

    /// Mark one durable incoming automation step. `done` removes the entry;
    /// any other state is retained and returned again on the next inbox poll.
    pub fn messenger_inbox_mark(
        &self,
        id: &str,
        state: &str,
        error: Option<&str>,
    ) -> Result<(), String> {
        if !matches!(
            state,
            "processing" | "reply_sent" | "failed" | "needs_review" | "done"
        ) {
            return Err(format!("inbox queue: неизвестное состояние {state}"));
        }
        let path = {
            let wallet = self
                .wallet
                .lock()
                .map_err(|_| "кошелёк занят".to_string())?;
            self.messenger_inbox_path(&wallet)
        };
        let mut entries = load_inbox(&path)?;
        let Some(entry) = entries.iter_mut().find(|entry| entry.id == id) else {
            return Ok(());
        };
        if state == "done" {
            entries.retain(|entry| entry.id != id);
        } else {
            entry.state = state.to_string();
            entry.last_error = error.map(str::to_string);
        }
        save_inbox(&path, &entries)
    }

    /// Link the already-sealed outgoing reply to its incoming task before the
    /// task is marked done. A replay then knows a logical S2 exists and must
    /// not create another envelope.
    pub fn messenger_inbox_attach_reply(&self, id: &str, reply_id: &str) -> Result<(), String> {
        if reply_id.trim().is_empty() {
            return Err("inbox queue: пустой reply id".into());
        }
        let path = {
            let wallet = self
                .wallet
                .lock()
                .map_err(|_| "кошелёк занят".to_string())?;
            self.messenger_inbox_path(&wallet)
        };
        let mut entries = load_inbox(&path)?;
        let Some(entry) = entries.iter_mut().find(|entry| entry.id == id) else {
            return Ok(());
        };
        entry.reply_id = Some(reply_id.to_string());
        entry.state = "reply_sent".into();
        save_inbox(&path, &entries)
    }

    pub fn messenger_history(&self, peer: &str) -> Result<Value, String> {
        let peer: Address = peer.trim().parse().map_err(err)?;
        let (_, _, dir) = self.messenger_network()?;
        let key = self.cached_history_key()?;
        let history = History::open(dir, key).map_err(err)?;
        let records = history.list(&peer, 200).map_err(err)?;
        serde_json::to_value(records).map_err(err)
    }

    pub fn messenger_conversations(&self) -> Result<Value, String> {
        let (_, _, dir) = self.messenger_network()?;
        let key = self.cached_history_key()?;
        let history = History::open(dir, key).map_err(err)?;
        let peers = history.conversations().map_err(err)?;
        Ok(json!({
            "peers": peers.iter().map(|peer| peer.to_string()).collect::<Vec<_>>()
        }))
    }

    pub fn messenger_contacts(&self) -> Result<Value, String> {
        let dir = {
            let wallet = self
                .wallet
                .lock()
                .map_err(|_| "кошелёк занят".to_string())?;
            self.messenger_contacts_dir(&wallet)
        };
        let key = self.cached_history_key()?;
        let book = ContactBook::open(dir, key).map_err(err)?;
        serde_json::to_value(book.list().map_err(err)?).map_err(err)
    }

    pub fn messenger_contact_save(&self, address: &str, name: &str) -> Result<Value, String> {
        let dir = {
            let wallet = self
                .wallet
                .lock()
                .map_err(|_| "кошелёк занят".to_string())?;
            self.messenger_contacts_dir(&wallet)
        };
        let key = self.cached_history_key()?;
        let book = ContactBook::open(dir, key).map_err(err)?;
        serde_json::to_value(book.save(address, name).map_err(err)?).map_err(err)
    }

    pub fn messenger_contact_delete(&self, address: &str) -> Result<Value, String> {
        let dir = {
            let wallet = self
                .wallet
                .lock()
                .map_err(|_| "кошелёк занят".to_string())?;
            self.messenger_contacts_dir(&wallet)
        };
        let key = self.cached_history_key()?;
        let book = ContactBook::open(dir, key).map_err(err)?;
        Ok(json!({"deleted": book.delete(address).map_err(err)?}))
    }

    pub fn messenger_blocked(&self) -> Result<Value, String> {
        let path = {
            let wallet = self
                .wallet
                .lock()
                .map_err(|_| "кошелёк занят".to_string())?;
            self.messenger_blocked_path(&wallet)
        };
        Ok(json!({"addresses": load_blocked(&path)?}))
    }

    pub fn messenger_block(&self, address: &str, blocked: bool) -> Result<Value, String> {
        Address::parse(address).map_err(err)?;
        let path = {
            let wallet = self
                .wallet
                .lock()
                .map_err(|_| "кошелёк занят".to_string())?;
            self.messenger_blocked_path(&wallet)
        };
        let mut list = load_blocked(&path)?;
        list.retain(|entry| entry != address);
        if blocked {
            list.push(address.to_string());
        }
        list.sort();
        list.dedup();
        save_blocked(&path, &list)?;
        Ok(json!({"blocked": blocked, "addresses": list}))
    }
}

fn load_blocked(path: &Path) -> Result<Vec<String>, String> {
    if !path.is_file() {
        return Ok(Vec::new());
    }
    flint_core::win_acl::restrict(path).map_err(err)?;
    let bytes = std::fs::read(path).map_err(err)?;
    serde_json::from_slice(&bytes).map_err(err)
}

fn save_blocked(path: &Path, list: &[String]) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(list).map_err(err)?;
    flint_core::win_acl::write_private_atomic(path, &bytes).map_err(err)
}

fn err(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::{
        keep_unfinished_reply, load_inbox, load_outbox, load_quarantine, save_inbox, save_outbox,
        save_quarantine, InboxEntry, OutboxEntry,
    };
    use flint_messenger::{Address, Envelope, Identity, MessageKind, DEFAULT_TTL_SECS};
    use std::collections::HashMap;

    #[test]
    fn quarantine_is_written_atomically_and_read_back() {
        let dir = std::env::temp_dir().join(format!("flint-quarantine-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("quarantine.json");
        let mut entries = HashMap::new();
        entries.insert("broken".to_string(), 42);
        save_quarantine(&path, &entries).expect("save");
        assert_eq!(load_quarantine(&path).expect("load"), entries);
        assert!(!path.with_extension("json.tmp-0").exists());

        std::fs::write(&path, "{broken").unwrap();
        assert!(load_quarantine(&path).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn outbox_round_trips_the_same_sealed_envelope() {
        let dir = std::env::temp_dir().join(format!("flint-outbox-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let sender = Identity::generate("tgrin").expect("sender");
        let recipient = Identity::generate("tgrin").expect("recipient");
        let recipient_address: Address = recipient.address().to_string().parse().unwrap();
        let envelope = Envelope::seal_now(
            &sender,
            &recipient_address,
            MessageKind::Text {
                text: "queued".into(),
            },
            DEFAULT_TTL_SECS,
        )
        .expect("seal");
        let entry = OutboxEntry {
            envelope: envelope.clone(),
            recipient: recipient_address.to_string(),
            kind: Some(MessageKind::Text {
                text: "queued".into(),
            }),
            kind_nonce: String::new(),
            kind_ciphertext: String::new(),
            created_at: envelope.created_at,
        };
        let path = dir.join("outbox.json");
        let key = [7u8; 32];
        let mut entry = entry;
        entry.encrypt_kind(&key).expect("encrypt kind");
        save_outbox(&path, std::slice::from_ref(&entry)).expect("save");
        let text = String::from_utf8_lossy(&std::fs::read(&path).expect("read outbox")).to_string();
        assert!(!text.contains("queued"), "plaintext leaked into outbox");
        let loaded = load_outbox(&path).expect("load");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].envelope.id, envelope.id);
        assert_eq!(loaded[0].envelope.ciphertext, envelope.ciphertext);
        assert_eq!(
            loaded[0].plaintext_kind(&key).expect("decrypt kind"),
            MessageKind::Text {
                text: "queued".into()
            }
        );
        save_outbox(&path, &[]).expect("clear");
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn completed_reply_is_not_requeued_without_its_envelope() {
        let sender = Identity::generate("tgrin").expect("sender");
        let recipient = Identity::generate("tgrin").expect("recipient");
        let address: Address = recipient.address().to_string().parse().unwrap();
        let envelope = Envelope::seal_now(
            &sender,
            &address,
            MessageKind::Text {
                text: "reply".into(),
            },
            DEFAULT_TTL_SECS,
        )
        .expect("seal");
        let entry = |reply_id: Option<&str>| InboxEntry {
            id: "incoming".into(),
            from: "tgrin1sender".into(),
            armor: "BEGINSLATEPACK. payload ENDSLATEPACK.".into(),
            created_at: 1,
            state: "reply_sent".into(),
            reply_id: reply_id.map(str::to_string),
            last_error: None,
        };
        let outbox = vec![OutboxEntry {
            envelope: envelope.clone(),
            recipient: address.to_string(),
            kind: Some(MessageKind::Text {
                text: "reply".into(),
            }),
            kind_nonce: String::new(),
            kind_ciphertext: String::new(),
            created_at: envelope.created_at,
        }];
        // The exact outgoing envelope is still queued: do not re-create the S2.
        assert!(keep_unfinished_reply(&entry(Some(&envelope.id)), &outbox));
        // It was delivered and removed: retire the task, never requeue it.
        assert!(!keep_unfinished_reply(&entry(Some(&envelope.id)), &[]));
        assert!(keep_unfinished_reply(&entry(None), &[]));
    }

    #[test]
    fn inbound_queue_survives_restart_without_losing_armor() {
        let dir = std::env::temp_dir().join(format!("flint-inbox-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("inbox.json");
        let entry = InboxEntry {
            id: "slate-id".into(),
            from: "tgrin1sender".into(),
            armor: "BEGINSLATEPACK. payload ENDSLATEPACK.".into(),
            created_at: 42,
            state: "reply_sent".into(),
            reply_id: Some("envelope-id".into()),
            last_error: None,
        };
        save_inbox(&path, std::slice::from_ref(&entry)).expect("save");
        let loaded = load_inbox(&path).expect("load");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].armor, entry.armor);
        assert_eq!(loaded[0].reply_id.as_deref(), Some("envelope-id"));
        save_inbox(&path, &[]).expect("clear");
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(dir);
    }
}
