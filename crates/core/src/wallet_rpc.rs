//! Blocking client for the official `grin-wallet` owner API (v3).
//!
//! The wallet serves JSON-RPC on `/v3/owner` and requires an **encrypted
//! session**: a plain `init_secure_api` handshake (ECDH over secp256k1), then
//! AES-256-GCM for every request/response, wrapped in `encrypted_request_v3`.
//! Authentication is HTTP Basic with `grin:<secret>` from `.owner_api_secret`.
//!
//! Everything here is blocking on purpose: all callers run on `spawn_blocking`.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use grin_util::secp::key::{PublicKey, SecretKey};
use grin_util::{from_hex, static_secp_instance, ToHex};
use ring::aead;
use ring::rand::{SecureRandom, SystemRandom};
use serde_json::{json, Value};

use crate::error::{FlintError, Result};

pub struct WalletRpc {
    url: String,
    auth: String,
    http: reqwest::blocking::Client,
    key: [u8; 32],
    next_id: AtomicU64,
}

impl WalletRpc {
    /// Handshake with a running `grin-wallet owner_api`.
    pub fn connect(base_url: &str, secret: &str) -> Result<Self> {
        let http = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| FlintError::Wallet(format!("http client: {e}")))?;
        let auth = format!(
            "Basic {}",
            base64::encode(format!("grin:{}", secret.trim()))
        );
        let url = base_url.trim_end_matches('/').to_string();

        let secp_inst = static_secp_instance();
        let secp = secp_inst.lock();
        let mut seed = [0u8; 32];
        SystemRandom::new()
            .fill(&mut seed)
            .map_err(|_| FlintError::Wallet("rng: не удалось получить энтропию".into()))?;
        let our_sk = SecretKey::from_slice(&secp, &seed)
            .map_err(|e| FlintError::Wallet(format!("secp key: {e}")))?;
        let our_pk = PublicKey::from_secret_key(&secp, &our_sk)
            .map_err(|e| FlintError::Wallet(format!("secp pubkey: {e}")))?;
        let our_pub_hex = our_pk.serialize_vec(&secp, true).to_hex();
        drop(secp);

        let mut rpc = Self {
            url,
            auth,
            http,
            // Replaced by the real key below; a zero key never leaves this method.
            key: [0u8; 32],
            next_id: AtomicU64::new(1),
        };

        let response = rpc.post(&json!({
            "jsonrpc": "2.0",
            "method": "init_secure_api",
            "params": [our_pub_hex],
            "id": 0,
        }))?;
        let server_hex = response
            .pointer("/result/Ok")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                FlintError::Wallet(format!("init_secure_api: неожиданный ответ {response}"))
            })?;

        let secp_inst = static_secp_instance();
        let secp = secp_inst.lock();
        let server_bytes = from_hex(server_hex)
            .map_err(|e| FlintError::Wallet(format!("init_secure_api: ключ сервера: {e}")))?;
        let server_pk = PublicKey::from_slice(&secp, &server_bytes)
            .map_err(|e| FlintError::Wallet(format!("init_secure_api: {e}")))?;
        let mut shared = server_pk;
        shared
            .mul_assign(&secp, &our_sk)
            .map_err(|e| FlintError::Wallet(format!("ecdh: {e}")))?;
        // The wallet uses the X coordinate (compressed form without the prefix)
        // as the AES key — see `init_secure_api` in grin-wallet's owner_rpc.
        let x = shared.serialize_vec(&secp, true);
        let mut key = [0u8; 32];
        key.copy_from_slice(&x[1..33]);
        drop(secp);

        rpc.key = key;
        Ok(rpc)
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn call(&self, method: &str, params: Value) -> Result<Value> {
        self.call_via(&self.http, method, params)
    }

    /// Same as `call`, but with its own client timeout. Long operations (scans)
    /// must not be cut off by the default 30-second request timeout.
    pub fn call_with_timeout(
        &self,
        method: &str,
        params: Value,
        timeout: std::time::Duration,
    ) -> Result<Value> {
        let client = reqwest::blocking::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| FlintError::Wallet(format!("http client: {e}")))?;
        self.call_via(&client, method, params)
    }

    fn call_via(
        &self,
        client: &reqwest::blocking::Client,
        method: &str,
        params: Value,
    ) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let inner = json!({"jsonrpc": "2.0", "method": method, "id": id, "params": params});
        let (nonce, body) = seal(&self.key, &inner)?;
        let response = self.post_via(
            client,
            &json!({
                "jsonrpc": "2.0",
                "method": "encrypted_request_v3",
                "id": id,
                "params": {"nonce": nonce, "body_enc": body},
            }),
        )?;

        if let Some(err) = response.get("error").filter(|e| !e.is_null()) {
            return Err(FlintError::Wallet(format!("{method}: {err}")));
        }
        let payload = response
            .pointer("/result/Ok")
            .ok_or_else(|| FlintError::Wallet(format!("{method}: ответ без Ok: {response}")))?;
        let nonce = payload
            .get("nonce")
            .and_then(|v| v.as_str())
            .ok_or_else(|| FlintError::Wallet(format!("{method}: нет nonce")))?;
        let body = payload
            .get("body_enc")
            .and_then(|v| v.as_str())
            .ok_or_else(|| FlintError::Wallet(format!("{method}: нет body_enc")))?;
        let value = open(&self.key, nonce, body)?;

        if let Some(err) = value.get("error").filter(|e| !e.is_null()) {
            return Err(FlintError::Wallet(format!("{method}: {err}")));
        }
        match value.pointer("/result/Ok") {
            Some(ok) => Ok(ok.clone()),
            None => Err(FlintError::Wallet(format!(
                "{method}: внутри сессии нет Ok: {value}"
            ))),
        }
    }

    /// Unlock the wallet; the returned value is the keychain mask token that
    /// every other call expects as its first argument.
    pub fn open_wallet(&self, password: &str) -> Result<Value> {
        self.call("open_wallet", json!([null, password]))
    }

    /// Create (or recover) a wallet in a running, empty wallet directory.
    pub fn create_wallet(&self, mnemonic: &str, password: &str, words: u32) -> Result<()> {
        self.call("create_wallet", json!([null, mnemonic, words, password]))?;
        Ok(())
    }

    pub fn close_wallet(&self) -> Result<()> {
        self.call("close_wallet", json!([null]))?;
        Ok(())
    }

    pub fn address(&self, token: &Value) -> Result<String> {
        let value = self.call("get_slatepack_address", json!([token, 0]))?;
        value
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| FlintError::Wallet("адрес: неожиданный ответ".into()))
    }

    /// Wallet summary (`WalletInfo`). The RPC returns a `(bool, WalletInfo)`
    /// tuple, which JSON encodes as `[refreshed, info]`.
    pub fn summary(&self, token: &Value, refresh: bool, min_conf: u64) -> Result<(bool, Value)> {
        let value = self.call("retrieve_summary_info", json!([token, refresh, min_conf]))?;
        // The tuple is `(validated, WalletInfo)`; `validated == false` means the
        // node check failed and the balance may be cached/stale.
        let validated = value.get(0).and_then(|v| v.as_bool()).unwrap_or(true);
        let info = value
            .get(1)
            .cloned()
            .or_else(|| value.as_object().map(|_| value.clone()))
            .ok_or_else(|| FlintError::Wallet("summary: неожиданный ответ".into()))?;
        Ok((validated, info))
    }

    /// Transaction log entries (`TxLogEntry`), newest first as the wallet
    /// returns them.
    pub fn txs(&self, token: &Value, refresh: bool) -> Result<Vec<Value>> {
        let value = self.call("retrieve_txs", json!([token, refresh, null, null]))?;
        Ok(value
            .get(1)
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default())
    }

    pub fn node_height(&self, token: &Value) -> Result<u64> {
        let value = self.call("node_height", json!([token]))?;
        value
            .get("height")
            .and_then(|v| {
                // grin serializes u64 fields as strings.
                v.as_u64()
                    .or_else(|| v.as_str().and_then(|s| s.parse::<u64>().ok()))
            })
            .ok_or_else(|| FlintError::Wallet(format!("node_height: {value}")))
    }

    pub fn scan(&self, token: &Value, start_height: Option<u64>) -> Result<()> {
        self.call("scan", json!([token, start_height, false]))?;
        Ok(())
    }

    /// Plain JSON-RPC call to the wallet's foreign API (loopback, unencrypted):
    /// receiving builds its response there because the foreign API owns
    /// `receive_tx`.
    pub fn foreign_call(&self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let body = json!({"jsonrpc": "2.0", "method": method, "id": id, "params": params});
        let response = self
            .http
            .post(format!("{}/v2/foreign", self.url))
            .json(&body)
            .send()
            .map_err(|e| FlintError::Wallet(format!("foreign api: {e}")))?;
        let text = response
            .text()
            .map_err(|e| FlintError::Wallet(format!("foreign api: {e}")))?;
        let value: Value = serde_json::from_str(&text)
            .map_err(|e| FlintError::Wallet(format!("foreign api: не JSON ({e}): {text}")))?;
        if let Some(err) = value.get("error").filter(|e| !e.is_null()) {
            return Err(FlintError::Wallet(format!("{method}: {err}")));
        }
        value
            .pointer("/result/Ok")
            .cloned()
            .ok_or_else(|| FlintError::Wallet(format!("{method}: ответ без Ok: {value}")))
    }

    fn post(&self, body: &Value) -> Result<Value> {
        self.post_via(&self.http, body)
    }

    fn post_via(&self, client: &reqwest::blocking::Client, body: &Value) -> Result<Value> {
        let response = client
            .post(format!("{}/v3/owner", self.url))
            .header(reqwest::header::AUTHORIZATION, &self.auth)
            .json(body)
            .send()
            .map_err(|e| FlintError::Wallet(format!("owner api: {e}")))?;
        let status = response.status();
        let text = response
            .text()
            .map_err(|e| FlintError::Wallet(format!("owner api: {e}")))?;
        if status.as_u16() == 401 {
            return Err(FlintError::Wallet(
                "owner api: неверный секрет (проверьте .owner_api_secret)".into(),
            ));
        }
        serde_json::from_str(&text)
            .map_err(|e| FlintError::Wallet(format!("owner api: не JSON ({e}): {text}")))
    }
}

impl Drop for WalletRpc {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.key.zeroize();
        self.auth.zeroize();
    }
}

fn seal(key: &[u8; 32], value: &Value) -> Result<(String, String)> {
    let mut buffer =
        serde_json::to_vec(value).map_err(|e| FlintError::Wallet(format!("json: {e}")))?;
    let mut nonce = [0u8; 12];
    SystemRandom::new()
        .fill(&mut nonce)
        .map_err(|_| FlintError::Wallet("rng: nonce".into()))?;
    let unbound = aead::UnboundKey::new(&aead::AES_256_GCM, key)
        .map_err(|_| FlintError::Wallet("aes: неверный ключ".into()))?;
    aead::LessSafeKey::new(unbound)
        .seal_in_place_append_tag(
            aead::Nonce::assume_unique_for_key(nonce),
            aead::Aad::empty(),
            &mut buffer,
        )
        .map_err(|_| FlintError::Wallet("aes: шифрование не удалось".into()))?;
    Ok((nonce.to_hex(), base64::encode(&buffer)))
}

fn open(key: &[u8; 32], nonce_hex: &str, body_b64: &str) -> Result<Value> {
    let mut buffer =
        base64::decode(body_b64).map_err(|e| FlintError::Wallet(format!("base64: {e}")))?;
    let nonce_bytes =
        from_hex(nonce_hex).map_err(|e| FlintError::Wallet(format!("nonce hex: {e}")))?;
    if nonce_bytes.len() < 12 {
        return Err(FlintError::Wallet("nonce короче 12 байт".into()));
    }
    let mut nonce = [0u8; 12];
    nonce.copy_from_slice(&nonce_bytes[..12]);
    let unbound = aead::UnboundKey::new(&aead::AES_256_GCM, key)
        .map_err(|_| FlintError::Wallet("aes: неверный ключ".into()))?;
    let plain = aead::LessSafeKey::new(unbound)
        .open_in_place(
            aead::Nonce::assume_unique_for_key(nonce),
            aead::Aad::empty(),
            &mut buffer,
        )
        .map_err(|_| FlintError::Wallet("aes: расшифровка не удалась".into()))?;
    serde_json::from_slice(plain).map_err(|e| FlintError::Wallet(format!("json: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seal_open_roundtrip() {
        let key = [7u8; 32];
        let value = json!({"a": 1, "b": "текст"});
        let (nonce, body) = seal(&key, &value).expect("seal");
        assert_eq!(open(&key, &nonce, &body).expect("open"), value);
    }

    #[test]
    fn nonce_is_fresh_and_wrong_key_fails() {
        let key = [7u8; 32];
        let value = json!({"a": 1});
        let (n1, b1) = seal(&key, &value).expect("seal 1");
        let (n2, _b2) = seal(&key, &value).expect("seal 2");
        assert_ne!(n1, n2, "nonce must not repeat");
        assert!(open(&[9u8; 32], &n1, &b1).is_err(), "wrong key must fail");
    }
}
