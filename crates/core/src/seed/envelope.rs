//! Flint's Argon2id seed envelope (`wallet.seed.v2`).
//!
//! Design and threat model: `docs/history/ARGON2ID.md`. Short version: the envelope is
//! the source of truth for the seed; `grin-wallet` never sees the user password.
//! At unlock the entropy is decrypted here, written to a temporary upstream
//! `wallet.seed` under a random 256-bit password, and the file is removed again
//! once the wallet is open and its address matches the envelope.
//!
//! Layout:
//!
//! ```text
//! {
//!   "format": "flint-seed", "version": 1, "network": "testnet",
//!   "kdf":    { "algorithm": "argon2id", "version": 19,
//!               "m_kib": 65536, "t": 3, "p": 4, "salt": "<base64 16>" },
//!   "cipher": { "algorithm": "chacha20poly1305", "nonce": "<base64 12>" },
//!   "seed_len": 32, "address": "tgrin1…", "ciphertext": "<base64>"
//! }
//! ```
//!
//! AAD binds every security-relevant header field:
//!
//! ```text
//! "flint-seed-v1\0" || network || 0x00 ||
//! m_kib LE || t LE || p LE || 19 LE || salt || nonce ||
//! seed_len LE || address
//! ```
//!
//! Plaintext is `{"seed":"<base64>","legacy":"<base64, optional>"}`: the Grin
//! entropy and, for migrated wallets, the exact bytes of the original
//! `wallet.seed` so a rollback can restore them byte for byte.

use std::fs;
use std::path::{Path, PathBuf};

use argon2::{Algorithm, Argon2, Params as Argon2Params, Version};
use base64;
use ring::aead;
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

use crate::error::{FlintError, Result};
use crate::seed::legacy::write_bytes_atomic;

/// File name inside the upstream `wallet_data` directory.
pub const ENVELOPE_FILE: &str = "wallet.seed.v2";

const FORMAT: &str = "flint-seed";
const VERSION: u32 = 1;
const MAGIC: &[u8] = b"flint-seed-v1\x00";
const KDF_ALGORITHM: &str = "argon2id";
const CIPHER_ALGORITHM: &str = "chacha20poly1305";
/// Argon2 v0x13 (`Version::V0x13`).
const ARGON2_VERSION: u32 = 19;
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;
const TAG_LEN: usize = 16;
const MAX_FILE: u64 = 64 * 1024;
const MAX_LEGACY_BLOB: usize = 4096;
const MAX_ADDRESS: usize = 128;
/// Grin entropy sizes: 12/15/18/21/24 words.
const SEED_LENGTHS: [usize; 5] = [16, 20, 24, 28, 32];

/// Argon2id cost, from RFC 9106's second recommendation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KdfCost {
    pub m_kib: u32,
    pub t: u32,
    pub p: u32,
}

impl KdfCost {
    /// 64 MiB, 3 passes, 4 lanes: ~0.3–0.6 s and 64 MiB per guess.
    pub const SECURE: Self = Self {
        m_kib: 65_536,
        t: 3,
        p: 4,
    };

    /// Bounds accepted when reading a file. Bounds also cap what a damaged or
    /// hostile envelope can make the app allocate.
    pub const MIN_M_KIB: u32 = 19_456; // 19 MiB
    pub const MAX_M_KIB: u32 = 262_144; // 256 MiB
    pub const MAX_T: u32 = 10;
    pub const MAX_P: u32 = 8;

    pub fn validate(&self) -> Result<()> {
        if !(Self::MIN_M_KIB..=Self::MAX_M_KIB).contains(&self.m_kib) {
            return Err(FlintError::Invalid(format!(
                "конверт seed: память Argon2id {} КиБ вне допустимого диапазона",
                self.m_kib
            )));
        }
        if self.t == 0 || self.t > Self::MAX_T {
            return Err(FlintError::Invalid(format!(
                "конверт seed: число проходов {} вне допустимого диапазона",
                self.t
            )));
        }
        if self.p == 0 || self.p > Self::MAX_P {
            return Err(FlintError::Invalid(format!(
                "конверт seed: параллелизм {} вне допустимого диапазона",
                self.p
            )));
        }
        Ok(())
    }
}

impl Default for KdfCost {
    fn default() -> Self {
        Self::SECURE
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct KdfJson {
    algorithm: String,
    version: u32,
    m_kib: u32,
    t: u32,
    p: u32,
    salt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CipherJson {
    algorithm: String,
    nonce: String,
}

/// The on-disk envelope. Fields are private: a caller cannot build one that
/// skipped validation, and `open` re-validates everything it reads anyway.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeedEnvelope {
    format: String,
    version: u32,
    network: String,
    kdf: KdfJson,
    cipher: CipherJson,
    seed_len: u32,
    address: String,
    ciphertext: String,
}

/// Decrypted envelope contents.
pub struct OpenedSeed {
    /// Grin seed entropy (16–32 bytes), zeroized on drop.
    pub seed: Zeroizing<Vec<u8>>,
    /// Exact bytes of the original `wallet.seed`, for migrated wallets.
    pub legacy: Option<Zeroizing<Vec<u8>>>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PlainSeed {
    seed: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    legacy: Option<String>,
}

impl Drop for PlainSeed {
    fn drop(&mut self) {
        self.seed.zeroize();
        if let Some(legacy) = &mut self.legacy {
            legacy.zeroize();
        }
    }
}

impl SeedEnvelope {
    /// Encrypt `seed` under `password`. `legacy` keeps the original
    /// `wallet.seed` bytes for a byte-exact rollback after a migration.
    pub fn seal(
        password: &str,
        network: &str,
        address: &str,
        seed: &[u8],
        legacy: Option<&[u8]>,
        cost: KdfCost,
    ) -> Result<Self> {
        cost.validate()?;
        check_network(network)?;
        check_address(network, address)?;
        if !SEED_LENGTHS.contains(&seed.len()) {
            return Err(FlintError::Invalid(
                "конверт seed: неожиданная длина seed (16/20/24/28/32 байта)".into(),
            ));
        }
        if legacy.is_some_and(|b| b.len() > MAX_LEGACY_BLOB) {
            return Err(FlintError::Invalid(
                "конверт seed: исходный wallet.seed слишком большой".into(),
            ));
        }
        if legacy.is_some_and(|b| b.is_empty()) {
            return Err(FlintError::Invalid(
                "конверт seed: пустой legacy-блоб недопустим".into(),
            ));
        }
        let password = password.as_bytes();
        if password.is_empty() {
            return Err(FlintError::Invalid(
                "конверт seed: пустой пароль недопустим".into(),
            ));
        }

        let rng = SystemRandom::new();
        let mut salt = [0u8; SALT_LEN];
        let mut nonce = [0u8; NONCE_LEN];
        rng.fill(&mut salt)
            .map_err(|_| FlintError::Invalid("нет системного источника случайности".into()))?;
        rng.fill(&mut nonce)
            .map_err(|_| FlintError::Invalid("нет системного источника случайности".into()))?;

        let mut envelope = SeedEnvelope {
            format: FORMAT.to_string(),
            version: VERSION,
            network: network.to_string(),
            kdf: KdfJson {
                algorithm: KDF_ALGORITHM.to_string(),
                version: ARGON2_VERSION,
                m_kib: cost.m_kib,
                t: cost.t,
                p: cost.p,
                salt: base64::encode(salt),
            },
            cipher: CipherJson {
                algorithm: CIPHER_ALGORITHM.to_string(),
                nonce: base64::encode(nonce),
            },
            seed_len: seed.len() as u32,
            address: address.to_string(),
            ciphertext: String::new(),
        };

        let plain = PlainSeed {
            seed: base64::encode(seed),
            legacy: legacy.map(base64::encode),
        };
        let mut plaintext = serde_json::to_vec(&plain).map_err(FlintError::Json)?;
        let aad = envelope.aad()?;
        let mut key = derive_key(password, &salt, &cost)?;
        let unbound = aead::UnboundKey::new(&aead::CHACHA20_POLY1305, key.as_ref())
            .map_err(|_| FlintError::Invalid("конверт seed: не удалось создать ключ".into()))?;
        let sealing = aead::LessSafeKey::new(unbound);
        sealing
            .seal_in_place_append_tag(
                aead::Nonce::assume_unique_for_key(nonce),
                aead::Aad::from(aad.as_slice()),
                &mut plaintext,
            )
            .map_err(|_| FlintError::Invalid("конверт seed: не удалось зашифровать".into()))?;
        envelope.ciphertext = base64::encode(&plaintext);

        plaintext.zeroize();
        key.zeroize();
        Ok(envelope)
    }

    /// Decrypt with `password`. Wrong password and damaged file are one error.
    pub fn open(&self, password: &str) -> Result<OpenedSeed> {
        self.validate()?;
        if password.is_empty() {
            return Err(FlintError::Invalid(
                "конверт seed: пустой пароль недопустим".into(),
            ));
        }
        let salt = base64::decode(&self.kdf.salt)
            .map_err(|_| FlintError::Invalid("конверт seed: соль — не base64".into()))?;
        let nonce_bytes = base64::decode(&self.cipher.nonce)
            .map_err(|_| FlintError::Invalid("конверт seed: nonce — не base64".into()))?;
        let mut ciphertext = base64::decode(&self.ciphertext)
            .map_err(|_| FlintError::Invalid("конверт seed: шифротекст — не base64".into()))?;
        if ciphertext.len() <= TAG_LEN || ciphertext.len() > MAX_FILE as usize {
            return Err(FlintError::Invalid(
                "конверт seed: неверная длина шифротекста".into(),
            ));
        }
        let mut nonce = [0u8; NONCE_LEN];
        nonce.copy_from_slice(&nonce_bytes);
        let cost = self.cost();
        let aad = self.aad()?;
        let mut key = derive_key(password.as_bytes(), &salt, &cost)?;
        let unbound = aead::UnboundKey::new(&aead::CHACHA20_POLY1305, key.as_ref())
            .map_err(|_| FlintError::Invalid("конверт seed: не удалось создать ключ".into()))?;
        let opening = aead::LessSafeKey::new(unbound);
        let plaintext = opening
            .open_in_place(
                aead::Nonce::assume_unique_for_key(nonce),
                aead::Aad::from(aad.as_slice()),
                &mut ciphertext,
            )
            .map_err(|_| {
                FlintError::Invalid("конверт seed: неверный пароль или повреждённый файл".into())
            })?;
        let plain: PlainSeed = match serde_json::from_slice(plaintext) {
            Ok(plain) => plain,
            Err(_) => {
                // The AEAD tag was valid, but the plaintext is not our JSON:
                // wipe the decrypted buffer before returning.
                ciphertext.zeroize();
                return Err(FlintError::Invalid(
                    "конверт seed: неожиданное содержимое".into(),
                ));
            }
        };
        ciphertext.zeroize();
        let seed = Zeroizing::new(
            base64::decode(&plain.seed)
                .map_err(|_| FlintError::Invalid("конверт seed: seed — не base64".into()))?,
        );
        if seed.len() != self.seed_len as usize || !SEED_LENGTHS.contains(&seed.len()) {
            return Err(FlintError::Invalid(
                "конверт seed: длина seed не совпадает с заголовком".into(),
            ));
        }
        let legacy = match plain.legacy.as_deref() {
            None => None,
            Some(text) => {
                let bytes =
                    Zeroizing::new(base64::decode(text).map_err(|_| {
                        FlintError::Invalid("конверт seed: legacy — не base64".into())
                    })?);
                if bytes.is_empty() || bytes.len() > MAX_LEGACY_BLOB {
                    return Err(FlintError::Invalid(
                        "конверт seed: исходный wallet.seed неверной длины".into(),
                    ));
                }
                Some(bytes)
            }
        };
        key.zeroize();
        Ok(OpenedSeed { seed, legacy })
    }

    /// Re-encrypt the same contents under a new password and fresh salt/nonce.
    pub fn rewrap(&self, old_password: &str, new_password: &str) -> Result<Self> {
        let opened = self.open(old_password)?;
        Self::seal(
            new_password,
            &self.network,
            &self.address,
            opened.seed.as_slice(),
            opened.legacy.as_deref().map(|v| v.as_slice()),
            self.cost(),
        )
    }

    /// Read and structurally validate an envelope file.
    pub fn read(path: &Path) -> Result<Self> {
        let meta = fs::metadata(path).map_err(FlintError::Io)?;
        if meta.len() > MAX_FILE {
            return Err(FlintError::Invalid(
                "конверт seed: файл подозрительно большой".into(),
            ));
        }
        let raw = fs::read(path).map_err(FlintError::Io)?;
        let envelope: SeedEnvelope = serde_json::from_slice(&raw)
            .map_err(|_| FlintError::Invalid("конверт seed: не JSON ожидаемого формата".into()))?;
        envelope.validate()?;
        Ok(envelope)
    }

    /// Atomically write the envelope (`tmp` → `fsync` → `rename`) with an ACL
    /// that keeps it to the current user.
    pub fn write_atomic(&self, path: &Path) -> Result<()> {
        let bytes = serde_json::to_vec_pretty(self).map_err(FlintError::Json)?;
        write_bytes_atomic(path, &bytes)
    }

    pub fn validate(&self) -> Result<()> {
        if self.format != FORMAT {
            return Err(FlintError::Invalid(
                "конверт seed: неизвестный формат".into(),
            ));
        }
        if self.version != VERSION {
            return Err(FlintError::Invalid(format!(
                "конверт seed: версия {} не поддерживается",
                self.version
            )));
        }
        check_network(&self.network)?;
        check_address(&self.network, &self.address)?;
        if self.kdf.algorithm != KDF_ALGORITHM || self.kdf.version != ARGON2_VERSION {
            return Err(FlintError::Invalid("конверт seed: неизвестный KDF".into()));
        }
        self.cost().validate()?;
        if base64::decode(&self.kdf.salt).map(|s| s.len()) != Ok(SALT_LEN) {
            return Err(FlintError::Invalid(
                "конверт seed: соль неверной длины".into(),
            ));
        }
        if self.cipher.algorithm != CIPHER_ALGORITHM {
            return Err(FlintError::Invalid("конверт seed: неизвестный шифр".into()));
        }
        if base64::decode(&self.cipher.nonce).map(|n| n.len()) != Ok(NONCE_LEN) {
            return Err(FlintError::Invalid(
                "конверт seed: nonce неверной длины".into(),
            ));
        }
        if !SEED_LENGTHS.contains(&(self.seed_len as usize)) {
            return Err(FlintError::Invalid(
                "конверт seed: seed_len вне допустимых значений".into(),
            ));
        }
        if self.ciphertext.len() > MAX_FILE as usize * 4 / 3 + 8 {
            return Err(FlintError::Invalid(
                "конверт seed: шифротекст подозрительно большой".into(),
            ));
        }
        base64::decode(&self.ciphertext)
            .map_err(|_| FlintError::Invalid("конверт seed: шифротекст — не base64".into()))?;
        Ok(())
    }

    pub fn network(&self) -> &str {
        &self.network
    }

    pub fn address(&self) -> &str {
        &self.address
    }

    pub fn seed_len(&self) -> usize {
        self.seed_len as usize
    }

    pub fn cost(&self) -> KdfCost {
        KdfCost {
            m_kib: self.kdf.m_kib,
            t: self.kdf.t,
            p: self.kdf.p,
        }
    }

    /// `<wallet_home>/wallet_data/wallet.seed.v2`.
    pub fn path_in(wallet_home: &Path) -> PathBuf {
        wallet_home.join("wallet_data").join(ENVELOPE_FILE)
    }

    fn aad(&self) -> Result<Vec<u8>> {
        let salt = base64::decode(&self.kdf.salt)
            .map_err(|_| FlintError::Invalid("конверт seed: соль — не base64".into()))?;
        let nonce = base64::decode(&self.cipher.nonce)
            .map_err(|_| FlintError::Invalid("конверт seed: nonce — не base64".into()))?;
        let mut aad = Vec::with_capacity(64 + self.address.len());
        aad.extend_from_slice(MAGIC);
        aad.extend_from_slice(self.network.as_bytes());
        aad.push(0);
        aad.extend_from_slice(&self.kdf.m_kib.to_le_bytes());
        aad.extend_from_slice(&self.kdf.t.to_le_bytes());
        aad.extend_from_slice(&self.kdf.p.to_le_bytes());
        aad.extend_from_slice(&ARGON2_VERSION.to_le_bytes());
        aad.extend_from_slice(&salt);
        aad.extend_from_slice(&nonce);
        aad.extend_from_slice(&self.seed_len.to_le_bytes());
        aad.extend_from_slice(self.address.as_bytes());
        Ok(aad)
    }
}

fn derive_key(password: &[u8], salt: &[u8], cost: &KdfCost) -> Result<Zeroizing<[u8; KEY_LEN]>> {
    let params = Argon2Params::new(cost.m_kib, cost.t, cost.p, Some(KEY_LEN))
        .map_err(|_| FlintError::Invalid("конверт seed: недопустимые параметры Argon2id".into()))?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = Zeroizing::new([0u8; KEY_LEN]);
    argon
        .hash_password_into(password, salt, key.as_mut())
        .map_err(|_| FlintError::Invalid("конверт seed: Argon2id не смог вывести ключ".into()))?;
    Ok(key)
}

fn check_network(network: &str) -> Result<()> {
    match network {
        "testnet" | "mainnet" => Ok(()),
        _ => Err(FlintError::Invalid(
            "конверт seed: сеть должна быть testnet или mainnet".into(),
        )),
    }
}

fn check_address(network: &str, address: &str) -> Result<()> {
    let prefix = if network == "mainnet" {
        "grin1"
    } else {
        "tgrin1"
    };
    if address.len() < 8
        || address.len() > MAX_ADDRESS
        || !address.is_ascii()
        || address.bytes().any(|b| !b.is_ascii_alphanumeric())
        || !address.starts_with(prefix)
    {
        return Err(FlintError::Invalid(
            "конверт seed: адрес не похож на адрес этой сети".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fast cost for tests; still inside the accepted range.
    const FAST: KdfCost = KdfCost {
        m_kib: 19_456,
        t: 1,
        p: 1,
    };
    const ADDR: &str = "tgrin1testaddress";

    fn seed() -> Vec<u8> {
        (0..32u8).collect()
    }

    fn sealed() -> SeedEnvelope {
        SeedEnvelope::seal("correct horse", "testnet", ADDR, &seed(), None, FAST).expect("seal")
    }

    #[test]
    fn envelope_round_trip() {
        let opened = sealed().open("correct horse").expect("open");
        assert_eq!(opened.seed.as_slice(), seed());
        assert!(opened.legacy.is_none());
    }

    #[test]
    fn all_seed_lengths_round_trip_and_tampering_is_rejected() {
        for (i, len) in SEED_LENGTHS.iter().copied().enumerate() {
            let entropy: Vec<u8> = (0..len).map(|n| (n as u8).wrapping_add(i as u8)).collect();
            let password = format!("pw-{i}-Пароль-{:02}", len);
            let envelope =
                SeedEnvelope::seal(&password, "testnet", ADDR, &entropy, None, FAST).expect("seal");
            let opened = envelope.open(&password).expect("open");
            assert_eq!(opened.seed.as_slice(), entropy.as_slice());
            assert!(envelope.open("wrong password").is_err());

            // Property: flipping a ciphertext byte must never authenticate.
            let mut value = serde_json::to_value(&envelope).unwrap();
            let ciphertext = value["ciphertext"].as_str().unwrap().to_string();
            let mut raw = base64::decode(&ciphertext).unwrap();
            raw[0] ^= 1;
            value["ciphertext"] = serde_json::Value::String(base64::encode(&raw));
            let tampered: SeedEnvelope = serde_json::from_value(value).unwrap();
            assert!(
                tampered.open(&password).is_err(),
                "tampered ciphertext opened"
            );
        }
    }

    #[test]
    fn empty_legacy_blob_is_refused_at_seal_time() {
        let err = SeedEnvelope::seal(
            "pw",
            "testnet",
            ADDR,
            &[1u8; 32],
            Some(&[]),
            KdfCost::SECURE,
        )
        .unwrap_err();
        assert!(err.to_string().contains("legacy"), "{err}");
    }

    #[test]
    fn wrong_password_is_refused() {
        assert!(sealed().open("wrong").is_err());
        assert!(sealed().open("").is_err());
    }

    #[test]
    fn envelope_keeps_the_legacy_blob_byte_exact() {
        let legacy = br#"{"encrypted_seed":"aa","salt":"bb","nonce":"cc"}"#.to_vec();
        let env =
            SeedEnvelope::seal("pw", "testnet", ADDR, &seed(), Some(&legacy), FAST).expect("seal");
        let opened = env.open("pw").expect("open");
        assert_eq!(
            opened.legacy.as_ref().expect("legacy present").as_slice(),
            legacy.as_slice()
        );
    }

    #[test]
    fn envelope_aad_binds_every_header_field() {
        let base = sealed();
        let mut cases: Vec<SeedEnvelope> = Vec::new();

        let mut e = base.clone();
        e.network = "mainnet".into();
        e.address = "grin1testaddress".into();
        cases.push(e);

        let mut e = base.clone();
        e.address = "tgrin1anotheraddress".into();
        cases.push(e);

        let mut e = base.clone();
        e.kdf.m_kib += 1024;
        cases.push(e);

        let mut e = base.clone();
        e.kdf.t += 1;
        cases.push(e);

        let mut e = base.clone();
        e.kdf.p += 1;
        cases.push(e);

        let mut e = base.clone();
        // Same length, different bytes: only the tag can notice.
        let mut salt = base64::decode(&e.kdf.salt).unwrap();
        salt[0] ^= 0xff;
        e.kdf.salt = base64::encode(salt);
        cases.push(e);

        let mut e = base.clone();
        let mut nonce = base64::decode(&e.cipher.nonce).unwrap();
        nonce[0] ^= 0xff;
        e.cipher.nonce = base64::encode(nonce);
        cases.push(e);

        let mut e = base.clone();
        let mut ct = base64::decode(&e.ciphertext).unwrap();
        ct[0] ^= 0xff;
        e.ciphertext = base64::encode(ct);
        cases.push(e);

        for (i, case) in cases.iter().enumerate() {
            assert!(
                case.open("correct horse").is_err(),
                "case {i} must not authenticate"
            );
        }
    }

    #[test]
    fn absurd_kdf_parameters_are_refused_before_allocating() {
        let base = sealed();
        for (m, t, p) in [
            (1u32, 3u32, 4u32),
            (4_000_000, 3, 4),
            (65_536, 0, 4),
            (65_536, 11, 4),
            (65_536, 3, 0),
            (65_536, 3, 9),
        ] {
            let mut e = base.clone();
            e.kdf.m_kib = m;
            e.kdf.t = t;
            e.kdf.p = p;
            assert!(e.validate().is_err(), "params {m}/{t}/{p} must be refused");
            assert!(e.open("correct horse").is_err());
        }
    }

    #[test]
    fn envelope_json_is_strict() {
        let mut value: serde_json::Value =
            serde_json::from_slice(&serde_json::to_vec(&sealed()).unwrap()).unwrap();
        value["extra"] = serde_json::json!(1);
        assert!(serde_json::from_value::<SeedEnvelope>(value.clone()).is_err());

        value.as_object_mut().unwrap().remove("extra");
        value["version"] = serde_json::json!(2);
        let env: SeedEnvelope = serde_json::from_value(value).unwrap();
        assert!(env.validate().is_err());
    }

    #[test]
    fn envelope_file_round_trip_and_size_limit() {
        let dir = std::env::temp_dir().join(format!(
            "flint-envelope-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = SeedEnvelope::path_in(&dir);
        sealed().write_atomic(&path).expect("write");
        let env = SeedEnvelope::read(&path).expect("read");
        assert_eq!(env.open("correct horse").unwrap().seed.as_slice(), seed());

        let big = dir.join("big.v2");
        std::fs::write(&big, vec![0u8; (MAX_FILE + 1) as usize]).unwrap();
        assert!(SeedEnvelope::read(&big).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rewrap_changes_salt_and_password() {
        let old = sealed();
        let new = old.rewrap("correct horse", "new password").expect("rewrap");
        assert_ne!(old.kdf.salt, new.kdf.salt);
        assert_ne!(old.cipher.nonce, new.cipher.nonce);
        assert!(new.open("correct horse").is_err());
        assert_eq!(new.open("new password").unwrap().seed.as_slice(), seed());
        assert_eq!(old.open("correct horse").unwrap().seed.as_slice(), seed());
    }

    #[test]
    fn envelope_never_contains_the_plain_seed() {
        let bytes = serde_json::to_vec(&sealed()).unwrap();
        assert!(!bytes.windows(4).any(|w| w == &seed()[..4]));
        assert!(!String::from_utf8_lossy(&bytes).contains(&base64::encode(seed())));
    }

    #[test]
    fn seed_length_is_recorded_and_checked() {
        let short = vec![7u8; 16];
        let env = SeedEnvelope::seal("pw", "testnet", ADDR, &short, None, FAST).unwrap();
        assert_eq!(env.seed_len(), 16);
        assert_eq!(env.open("pw").unwrap().seed.as_slice(), short.as_slice());
        assert!(SeedEnvelope::seal("pw", "testnet", ADDR, &[1, 2, 3], None, FAST).is_err());
    }

    #[test]
    fn argon2id_matches_reference_vector() {
        // Reference implementation test suite
        // (phc-winner-argon2/src/test.c, carried in vendor/argon2/tests/kat.rs):
        // Argon2id v=19, m=65536, t=2, p=1, password "password", salt "somesalt".
        let key = derive_key(
            b"password",
            b"somesalt",
            &KdfCost {
                m_kib: 65_536,
                t: 2,
                p: 1,
            },
        )
        .expect("derive");
        let expected =
            grin_util::from_hex("09316115d5cf24ed5a15a31a3ba326e5cf32edc24702987c02b6566f61913cf7")
                .expect("hex");
        assert_eq!(&key[..], expected.as_slice());
    }

    #[test]
    fn address_and_network_are_checked() {
        assert!(SeedEnvelope::seal("pw", "regtest", ADDR, &seed(), None, FAST).is_err());
        assert!(SeedEnvelope::seal("pw", "mainnet", ADDR, &seed(), None, FAST).is_err());
        assert!(SeedEnvelope::seal("pw", "testnet", "grin1x", &seed(), None, FAST).is_err());
        assert!(SeedEnvelope::seal("pw", "testnet", "", &seed(), None, FAST).is_err());
    }
}
