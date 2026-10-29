//! Process-level failpoints for the explicitly built crash-test binary.
//!
//! Release builds do not enable `crash-injection`; without that feature these
//! functions are no-ops and do not inspect the environment.

/// Stop at a receive durability boundary in an explicitly built crash binary.
///
/// `intent` is after Flint's durable intent but before any wallet mutation.
/// `upstream` is after the patched wallet returned its atomically committed S2,
/// but before Flint's response journal. `journal` is after that journal write,
/// but before Slatepack armoring. The live PowerShell harness additionally
/// requires a ready testnet node and `.crash-test-ok` in every wallet directory.
///
/// The pause mode is for an operator-controlled VM power cut. It first writes
/// and syncs a non-secret marker, then parks forever. It requires a separate
/// destructible-stand marker; neither mode exists in ordinary release builds.
#[cfg(feature = "crash-injection")]
pub fn receive_boundary(stage: &str) {
    if std::env::var("FLINT_CRASH_AFTER_RECEIVE").ok().as_deref() == Some(stage) {
        std::process::exit(9);
    }
    if std::env::var("FLINT_PAUSE_AFTER_RECEIVE").ok().as_deref() == Some(stage) {
        let evidence = std::env::var_os("FLINT_PAUSE_EVIDENCE_DIR").unwrap_or_else(|| {
            eprintln!("F-04 pause: не задан FLINT_PAUSE_EVIDENCE_DIR");
            std::process::exit(90);
        });
        let marker = match write_pause_marker(std::path::Path::new(&evidence), stage) {
            Ok(path) => path,
            Err(error) => {
                eprintln!("F-04 pause: маркер не записан: {error}");
                std::process::exit(90);
            }
        };
        eprintln!("F04_PAUSE_MARKER={}", marker.display());
        loop {
            std::thread::park();
        }
    }
}

#[cfg(feature = "crash-injection")]
fn write_pause_marker(
    evidence: &std::path::Path,
    stage: &str,
) -> std::io::Result<std::path::PathBuf> {
    use std::io::Write;

    if stage != "intent" && stage != "upstream" && stage != "journal" {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "неизвестная граница receive",
        ));
    }
    let stand = std::fs::read_to_string(evidence.join(".destructible-f04-stand"))?;
    if stand.trim() != "F04-DESTRUCTIBLE-TESTNET-VM" {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "каталог evidence не помечен как одноразовый F-04 testnet VM",
        ));
    }
    let marker = evidence.join(format!(
        "receive-{stage}-{}-paused.json",
        std::process::id()
    ));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&marker)?;
    let elapsed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let body = format!(
        "{{\n  \"kind\": \"f04_receive_pause\",\n  \"stage\": \"{stage}\",\n  \"pid\": {},\n  \"unix_time_ms\": {}\n}}\n",
        std::process::id(),
        elapsed.as_millis(),
    );
    file.write_all(body.as_bytes())?;
    file.sync_all()?;
    Ok(marker)
}

#[cfg(not(feature = "crash-injection"))]
pub fn receive_boundary(_stage: &str) {}

/// Simulate a full Flint journal volume after upstream committed S2.
///
/// This is deliberately scoped to the response journal write: intent creation,
/// LMDB and every unrelated file remain writable. ERROR_DISK_FULL (112) is the
/// same Windows error class returned by a real full NTFS volume.
#[cfg(feature = "crash-injection")]
pub fn receive_journal_write() -> std::io::Result<()> {
    if std::env::var("FLINT_SIMULATE_RECEIVE_DISK_FULL")
        .ok()
        .as_deref()
        == Some("1")
    {
        return Err(std::io::Error::from_raw_os_error(112));
    }
    Ok(())
}

#[cfg(not(feature = "crash-injection"))]
pub fn receive_journal_write() -> std::io::Result<()> {
    Ok(())
}

#[cfg(all(test, feature = "crash-injection"))]
mod tests {
    use super::write_pause_marker;

    fn evidence(name: &str) -> std::path::PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "flint-f04-pause-{name}-{}-{unique}",
            std::process::id()
        ))
    }

    #[test]
    fn pause_marker_requires_an_explicit_destructible_stand() {
        let root = evidence("unmarked");
        std::fs::create_dir_all(&root).expect("evidence");
        let error = write_pause_marker(&root, "upstream").expect_err("unmarked stand");
        assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn pause_marker_is_unique_and_contains_no_wallet_material() {
        let root = evidence("marked");
        std::fs::create_dir_all(&root).expect("evidence");
        std::fs::write(
            root.join(".destructible-f04-stand"),
            "F04-DESTRUCTIBLE-TESTNET-VM\n",
        )
        .expect("stand marker");
        let path = write_pause_marker(&root, "journal").expect("pause marker");
        let text = std::fs::read_to_string(&path).expect("marker contents");
        assert!(text.contains("\"stage\": \"journal\""));
        assert!(!text.contains("seed"));
        assert!(!text.contains("password"));
        assert!(write_pause_marker(&root, "journal").is_err());
        std::fs::remove_dir_all(root).expect("cleanup");
    }
}
