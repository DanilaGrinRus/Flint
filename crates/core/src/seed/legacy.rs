//! The upstream `wallet.seed` format of `grin-wallet` 5.5.x, reimplemented.
//!
//! Upstream keeps `WalletSeed`/`EncryptedWalletSeed` private (`mod lifecycle` is
//! not exported by `grin_wallet_impls`), so Flint has to speak the format
//! itself: it must be able to read a seed written by `grin-wallet` and to write
//! a seed file the upstream process will open.
//!
//! The format is pinned by `vendor/grin_wallet_impls/src/lifecycle/seed.rs`:
//!
//! ```text
//! JSON { encrypted_seed: hex, salt: hex, nonce: hex }
//! key   = PBKDF2-HMAC-SHA512(password, salt, 100 iterations, 32 bytes)
//! nonce = 12 random bytes
//! data  = ChaCha20-Poly1305(key, nonce, no AAD, seed bytes) || 16-byte tag
//! ```
//!
//! This module is a transport codec, not a security boundary: the 100
//! iterations are worthless against an offline guesser. Flint writes this file
//! only with a random 256-bit password (see `docs/history/ARGON2ID.md`), and the real
//! user password is stretched with Argon2id by [`super::envelope`].
//!
//! Compatibility is checked in both directions by
//! `crates/core/tests/seed_codec.rs`: a wallet created by upstream is read by
//! this codec, and a seed file written by this codec is opened by upstream.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};

use ring::aead;
use ring::pbkdf2;
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

use crate::error::{FlintError, Result};

/// File name upstream uses inside its `wallet_data` directory.
pub const SEED_FILE: &str = "wallet.seed";

/// Upstream iteration count. Kept as a named constant so nothing here looks
/// like a tunable security parameter — it is not, it is the legacy format.
pub const PBKDF2_ITERATIONS: u32 = 100;

const SALT_LEN: usize = 8;
const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;
/// Tag length of ChaCha20-Poly1305 (and of AES-256-GCM, which upstream uses in
/// its `pop` calls).
const TAG_LEN: usize = 16;
/// A seed file is ~200 bytes; anything near this bound is not a seed file.
pub const MAX_SEED_FILE: u64 = 64 * 1024;
/// Grin entropy sizes: 12/15/18/21/24 words.
const SEED_LENGTHS: [usize; 5] = [16, 20, 24, 28, 32];

#[derive(Debug, Serialize, Deserialize)]
struct SeedJson {
    encrypted_seed: String,
    salt: String,
    nonce: String,
}

/// Namespace for the upstream seed-file codec.
pub struct LegacySeed;

impl LegacySeed {
    /// Read the raw bytes of a seed file with the size limit applied. Callers
    /// that need the exact bytes (migration, rollback) use this instead of
    /// `fs::read`, which would happily allocate a huge file.
    pub fn read_raw(path: &Path) -> Result<Vec<u8>> {
        let meta = fs::metadata(path).map_err(FlintError::Io)?;
        if meta.len() > MAX_SEED_FILE {
            return Err(FlintError::Invalid(
                "wallet.seed: файл подозрительно большой".into(),
            ));
        }
        fs::read(path).map_err(FlintError::Io)
    }

    /// Read and decrypt `<path>` with `password`.
    pub fn read_file(path: &Path, password: &str) -> Result<Zeroizing<Vec<u8>>> {
        let raw = Self::read_raw(path)?;
        Self::decrypt(&raw, password)
    }

    /// Decrypt the JSON bytes of a seed file.
    pub fn decrypt(raw: &[u8], password: &str) -> Result<Zeroizing<Vec<u8>>> {
        if raw.len() as u64 > MAX_SEED_FILE {
            return Err(FlintError::Invalid(
                "wallet.seed: файл подозрительно большой".into(),
            ));
        }
        let parsed: SeedJson = serde_json::from_slice(raw)
            .map_err(|_| FlintError::Invalid("wallet.seed: не JSON ожидаемого формата".into()))?;
        let mut ciphertext = decode_hex(&parsed.encrypted_seed, "encrypted_seed")?;
        let salt = decode_hex(&parsed.salt, "salt")?;
        let nonce_bytes = decode_hex(&parsed.nonce, "nonce")?;
        if salt.len() != SALT_LEN || nonce_bytes.len() != NONCE_LEN {
            return Err(FlintError::Invalid(
                "wallet.seed: неверная длина соли или nonce".into(),
            ));
        }
        if ciphertext.len() <= TAG_LEN {
            return Err(FlintError::Invalid(
                "wallet.seed: шифротекст короче тега".into(),
            ));
        }

        let mut key = Zeroizing::new([0u8; KEY_LEN]);
        pbkdf2::derive(
            pbkdf2::PBKDF2_HMAC_SHA512,
            NonZeroU32::new(PBKDF2_ITERATIONS).expect("nonzero"),
            &salt,
            password.as_bytes(),
            key.as_mut(),
        );

        let mut nonce = [0u8; NONCE_LEN];
        nonce.copy_from_slice(&nonce_bytes);
        let unbound = aead::UnboundKey::new(&aead::CHACHA20_POLY1305, key.as_ref())
            .map_err(|_| FlintError::Invalid("wallet.seed: не удалось создать ключ".into()))?;
        let opening = aead::LessSafeKey::new(unbound);
        let plain = opening
            .open_in_place(
                aead::Nonce::assume_unique_for_key(nonce),
                aead::Aad::from(&[][..]),
                &mut ciphertext,
            )
            // The same message for a wrong password and a damaged file: an
            // attacker must not learn which of the two it was from the app.
            .map_err(|_| {
                FlintError::Invalid("wallet.seed: неверный пароль или повреждённый файл".into())
            })?;
        let seed = Zeroizing::new(plain.to_vec());
        ciphertext.zeroize();
        if !SEED_LENGTHS.contains(&seed.len()) {
            return Err(FlintError::Invalid(
                "wallet.seed: неожиданная длина seed".into(),
            ));
        }
        Ok(seed)
    }

    /// Encrypt `seed` with `password` into the bytes of a seed file.
    pub fn encrypt(seed: &[u8], password: &str) -> Result<Vec<u8>> {
        if !SEED_LENGTHS.contains(&seed.len()) {
            return Err(FlintError::Invalid(
                "seed: неожиданная длина (16/20/24/28/32 байта)".into(),
            ));
        }
        let rng = SystemRandom::new();
        let mut salt = [0u8; SALT_LEN];
        let mut nonce = [0u8; NONCE_LEN];
        rng.fill(&mut salt)
            .map_err(|_| FlintError::Invalid("нет системного источника случайности".into()))?;
        rng.fill(&mut nonce)
            .map_err(|_| FlintError::Invalid("нет системного источника случайности".into()))?;

        let mut key = Zeroizing::new([0u8; KEY_LEN]);
        pbkdf2::derive(
            pbkdf2::PBKDF2_HMAC_SHA512,
            NonZeroU32::new(PBKDF2_ITERATIONS).expect("nonzero"),
            &salt,
            password.as_bytes(),
            key.as_mut(),
        );
        let unbound = aead::UnboundKey::new(&aead::CHACHA20_POLY1305, key.as_ref())
            .map_err(|_| FlintError::Invalid("wallet.seed: не удалось создать ключ".into()))?;
        let sealing = aead::LessSafeKey::new(unbound);
        let mut ciphertext = seed.to_vec();
        sealing
            .seal_in_place_append_tag(
                aead::Nonce::assume_unique_for_key(nonce),
                aead::Aad::from(&[][..]),
                &mut ciphertext,
            )
            .map_err(|_| FlintError::Invalid("wallet.seed: не удалось зашифровать".into()))?;

        let json = SeedJson {
            encrypted_seed: ToHexExt::to_hex(&ciphertext),
            salt: ToHexExt::to_hex(&salt),
            nonce: ToHexExt::to_hex(&nonce),
        };
        ciphertext.zeroize();
        serde_json::to_vec(&json).map_err(FlintError::Json)
    }

    /// Write a seed file atomically (`tmp` → `fsync` → `rename`) with an ACL
    /// that keeps the file to the current user.
    pub fn write_atomic(path: &Path, seed: &[u8], password: &str) -> Result<()> {
        let bytes = Self::encrypt(seed, password)?;
        write_bytes_atomic(path, &bytes)
    }

    /// Remove a seed file if it exists.
    pub fn remove(path: &Path) -> Result<()> {
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(FlintError::Io(e)),
        }
    }
}

/// Write `bytes` to `path` through a uniquely named temporary file in the same
/// directory. The seed never exists in a half-written state.
pub(crate) fn write_bytes_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| FlintError::Invalid("путь без каталога".into()))?;
    fs::create_dir_all(dir).map_err(FlintError::Io)?;
    let tmp: PathBuf = dir.join(format!(
        ".{}.tmp-{}-{}",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("seed"),
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));

    let write = || -> Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
            .map_err(FlintError::Io)?;
        // Seed files are encrypted, but the owner-only ACL is part of the
        // storage contract: do not silently write a seed with a broader ACL.
        // A filesystem that cannot apply it is refused instead of downgraded.
        crate::win_acl::restrict(&tmp).map_err(|e| {
            FlintError::Invalid(format!(
                "не удалось ограничить доступ к временному файлу seed: {e}"
            ))
        })?;
        file.write_all(bytes).map_err(FlintError::Io)?;
        file.sync_all().map_err(FlintError::Io)?;
        drop(file);
        rename_durable(&tmp, path).map_err(FlintError::Io)?;
        // On Unix the directory entry itself needs a flush as well. Windows
        // uses MOVEFILE_WRITE_THROUGH in `rename_durable` instead, because
        // FlushFileBuffers on a directory handle is not permitted there.
        #[cfg(not(windows))]
        if let Err(e) = sync_parent(dir) {
            tracing::warn!("не удалось синхронизировать каталог seed: {e}");
        }
        Ok(())
    };

    match write() {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            Err(e)
        }
    }
}

/// Rename with a durability guarantee. On Windows `std::fs::rename` may return
/// before the directory entry reaches the disk; `MoveFileExW` with
/// `MOVEFILE_WRITE_THROUGH` waits for it. On Unix the caller flushes the parent
/// directory after the rename.
fn rename_durable(from: &Path, to: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;

        const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
        const MOVEFILE_WRITE_THROUGH: u32 = 0x8;

        #[link(name = "kernel32")]
        extern "system" {
            fn MoveFileExW(existing: *const u16, new_name: *const u16, flags: u32) -> i32;
        }

        let from_wide: Vec<u16> = from.as_os_str().encode_wide().chain(Some(0)).collect();
        let to_wide: Vec<u16> = to.as_os_str().encode_wide().chain(Some(0)).collect();
        let ok = unsafe {
            MoveFileExW(
                from_wide.as_ptr(),
                to_wide.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        };
        if ok == 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
    #[cfg(not(windows))]
    {
        fs::rename(from, to)
    }
}

/// Flush the directory entry after a rename (Unix). Windows uses
/// [`rename_durable`] instead: `FlushFileBuffers` on a directory handle fails
/// with access denied there.
#[cfg(not(windows))]
fn sync_parent(dir: &Path) -> std::io::Result<()> {
    std::fs::File::open(dir)?.sync_all()
}

fn decode_hex(value: &str, field: &str) -> Result<Vec<u8>> {
    grin_util::from_hex(value)
        .map_err(|_| FlintError::Invalid(format!("wallet.seed: поле {field} — не hex")))
}

/// Local hex helper so the module does not depend on the `ToHex` trait being in
/// scope at every call site.
trait ToHexExt {
    fn to_hex(&self) -> String;
}

impl<T: AsRef<[u8]>> ToHexExt for T {
    fn to_hex(&self) -> String {
        self.as_ref()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed() -> Vec<u8> {
        (0..32u8).collect()
    }

    #[cfg(not(windows))]
    #[test]
    fn parent_directory_can_be_synced() {
        let dir = std::env::temp_dir().join(format!(
            "flint-sync-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let result = sync_parent(&dir);
        let _ = fs::remove_dir_all(&dir);
        assert!(result.is_ok(), "directory fsync failed: {result:?}");
    }

    #[cfg(windows)]
    #[test]
    fn durable_rename_replaces_a_file() {
        let dir = std::env::temp_dir().join(format!(
            "flint-rename-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let from = dir.join("from.tmp");
        let to = dir.join("to");
        fs::write(&from, b"new").unwrap();
        fs::write(&to, b"old").unwrap();
        rename_durable(&from, &to).expect("MoveFileExW with write-through");
        assert_eq!(fs::read(&to).unwrap(), b"new");
        assert!(!from.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn legacy_round_trip() {
        let json = LegacySeed::encrypt(&seed(), "pw").expect("encrypt");
        let back = LegacySeed::decrypt(&json, "pw").expect("decrypt");
        assert_eq!(back.as_slice(), seed().as_slice());
    }

    #[test]
    fn legacy_wrong_password_fails() {
        let json = LegacySeed::encrypt(&seed(), "pw").expect("encrypt");
        assert!(LegacySeed::decrypt(&json, "pw2").is_err());
    }

    #[test]
    fn legacy_rejects_malformed_json_and_hex() {
        assert!(LegacySeed::decrypt(b"{}", "pw").is_err());
        assert!(LegacySeed::decrypt(b"not json", "pw").is_err());
        let mut json = LegacySeed::encrypt(&seed(), "pw").expect("encrypt");
        // Flip one hex digit of the ciphertext: authentication must fail.
        let text = String::from_utf8(json.clone()).unwrap();
        let replaced = text.replacen("encrypted_seed\":\"", "encrypted_seed\":\"0", 1);
        json = replaced.into_bytes();
        assert!(LegacySeed::decrypt(&json, "pw").is_err());
    }

    #[test]
    fn legacy_writes_bytes_no_plaintext_seed() {
        let json = LegacySeed::encrypt(&seed(), "pw").expect("encrypt");
        assert!(!json.windows(4).any(|w| w == &seed()[..4]));
    }

    #[test]
    fn legacy_accepts_all_grin_entropy_sizes() {
        for len in SEED_LENGTHS {
            let s: Vec<u8> = (0..len as u8).collect();
            let json = LegacySeed::encrypt(&s, "pw").expect("encrypt");
            assert_eq!(
                LegacySeed::decrypt(&json, "pw").unwrap().as_slice(),
                s.as_slice()
            );
        }
        assert!(LegacySeed::encrypt(&[1u8; 17], "pw").is_err());
    }

    #[test]
    fn legacy_file_round_trip_and_removal() {
        let dir = std::env::temp_dir().join(format!(
            "flint-legacy-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = dir.join(SEED_FILE);
        LegacySeed::write_atomic(&path, &seed(), "pw").expect("write");
        assert_eq!(
            LegacySeed::read_file(&path, "pw").unwrap().as_slice(),
            seed()
        );
        LegacySeed::remove(&path).expect("remove");
        assert!(!path.exists());
        LegacySeed::remove(&path).expect("remove twice");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
