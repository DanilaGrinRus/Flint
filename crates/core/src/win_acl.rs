//! Restrict secret files to the current user.
//!
//! On Windows the default inherited ACL of a user profile directory is broader
//! than "only me", so wallet API secrets and Tor hidden-service keys get an
//! explicit owner-only ACL. The resulting DACL is read back and verified: a
//! successful ACL command alone does not prove that other explicit ACEs vanished.
//! Failures are returned to the caller; each caller must apply its security policy.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[cfg(test)]
thread_local! {
    static TEST_ACL_FAILURES: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
}

/// Thread-local fault injection: parallel tests cannot affect one another.
#[cfg(test)]
pub(crate) fn inject_acl_failures(count: u8) {
    TEST_ACL_FAILURES.with(|failures| failures.set(count));
}

#[cfg(test)]
fn take_test_acl_failure() -> bool {
    TEST_ACL_FAILURES.with(|failures| {
        let count = failures.get();
        if count > 0 {
            failures.set(count - 1);
        }
        count > 0
    })
}

/// Atomically replace a sensitive file without ever writing its contents under
/// the directory's inherited ACL. The temporary file is protected while still
/// empty; the final file is checked again after the durable rename.
pub fn write_private_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    write_private_atomic_with(path, bytes, restrict)
}

fn write_private_atomic_with(
    path: &Path,
    bytes: &[u8],
    apply_acl: impl Fn(&Path) -> std::io::Result<()>,
) -> std::io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "у файла нет каталога")
    })?;
    let name = path.file_name().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "у пути нет имени файла")
    })?;
    std::fs::create_dir_all(parent)?;

    let (tmp, file) = create_private_temp(parent, name)?;
    // Close our create handle first: the ACL helper opens the same file to
    // change its DACL, and Windows rejects that while the file is held with
    // this sharing mode. The file is still empty at this point.
    drop(file);
    let result = (|| {
        // This must precede write_all: even the temporary plaintext is sensitive.
        apply_acl(&tmp)?;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        rename_durable(&tmp, path)?;
        apply_acl(path)?;
        #[cfg(not(windows))]
        sync_parent(parent)?;
        Ok(())
    })();
    if result.is_err() {
        // After a successful rename this is only a harmless NotFound. Never
        // remove `path`: a final ACL error must not destroy the new record.
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

fn create_private_temp(
    parent: &Path,
    name: &std::ffi::OsStr,
) -> std::io::Result<(PathBuf, std::fs::File)> {
    for _ in 0..128 {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let tmp = parent.join(format!(
            ".{}.tmp-{}-{sequence}",
            name.to_string_lossy(),
            std::process::id()
        ));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
        {
            Ok(file) => return Ok((tmp, file)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "не удалось выделить уникальное имя временного файла",
    ))
}

/// Replace a file and wait for the directory update where the platform permits.
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
        std::fs::rename(from, to)
    }
}

#[cfg(not(windows))]
fn sync_parent(dir: &Path) -> std::io::Result<()> {
    std::fs::File::open(dir)?.sync_all()
}

#[cfg(windows)]
const OWNER_ONLY_SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
$path = $env:FLINT_ACL_PATH
$sid = [System.Security.Principal.WindowsIdentity]::GetCurrent().User
if (-not $path -or -not $sid) { throw 'ACL path or current-user SID is unavailable' }
$acl = Get-Acl -LiteralPath $path
$acl.SetAccessRuleProtection($true, $false)
foreach ($rule in @($acl.Access)) {
    $acl.RemoveAccessRuleSpecific($rule)
}
$allow = [System.Security.AccessControl.FileSystemAccessRule]::new(
    $sid,
    [System.Security.AccessControl.FileSystemRights]::FullControl,
    [System.Security.AccessControl.AccessControlType]::Allow
)
$acl.AddAccessRule($allow)
# Set-Acl can request SeSecurityPrivilege even for a file owned by the user.
# The file-specific API updates the DACL without that elevation requirement.
[System.IO.File]::SetAccessControl($path, $acl)
$check = Get-Acl -LiteralPath $path
$rules = @($check.Access)
if (-not $check.AreAccessRulesProtected -or $rules.Count -ne 1) {
    throw 'owner-only ACL verification failed'
}
$rule = $rules[0]
if ($rule.IdentityReference.Translate([System.Security.Principal.SecurityIdentifier]).Value -ne $sid.Value -or
    $rule.FileSystemRights -ne [System.Security.AccessControl.FileSystemRights]::FullControl -or
    $rule.AccessControlType -ne [System.Security.AccessControl.AccessControlType]::Allow -or
    $rule.IsInherited) {
    throw 'owner-only ACL content verification failed'
}
"#;

#[cfg(windows)]
fn windows_powershell_module_path() -> std::path::PathBuf {
    std::env::var_os("SystemRoot")
        .map(|root| std::path::PathBuf::from(root).join(r"System32\WindowsPowerShell\v1.0\Modules"))
        .unwrap_or_else(|| {
            std::path::PathBuf::from(r"C:\Windows\System32\WindowsPowerShell\v1.0\Modules")
        })
}

#[cfg(windows)]
fn powershell_command() -> std::process::Command {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut command = std::process::Command::new("powershell");
    command
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
        ])
        // Windows PowerShell 5.1 cannot autoload Microsoft.PowerShell.Security
        // when it inherits PSModulePath from PowerShell 7: Get-Acl fails as
        // "not recognized" and every ACL read/write is rejected. Pin the module
        // path of the Windows PowerShell installation that owns powershell.exe.
        .env("PSModulePath", windows_powershell_module_path())
        .creation_flags(CREATE_NO_WINDOW);
    command
}

#[cfg(windows)]
pub fn restrict(path: &Path) -> std::io::Result<()> {
    #[cfg(test)]
    if take_test_acl_failure() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "injected ACL failure",
        ));
    }
    use std::process::Stdio;

    let status = powershell_command()
        .args(["-Command", OWNER_ONLY_SCRIPT])
        // Environment variables preserve spaces and non-ASCII path bytes without
        // interpolating a path into the PowerShell command line.
        .env("FLINT_ACL_PATH", path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!(
                "не удалось применить и проверить owner-only ACL к {} (нужен NTFS; FAT32/exFAT и некоторые сетевые диски не поддерживают ACL — выберите другую папку данных)",
                path.display()
            ),
        ))
    }
}

#[cfg(not(windows))]
pub fn restrict(path: &Path) -> std::io::Result<()> {
    #[cfg(test)]
    if take_test_acl_failure() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "injected ACL failure",
        ));
    }
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = std::fs::metadata(path)?.permissions();
    permissions.set_mode(0o600);
    std::fs::set_permissions(path, permissions)
}

#[cfg(test)]
mod tests {
    use super::{restrict, write_private_atomic_with};
    use std::cell::Cell;

    fn probe_dir(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "flint-private-write-{label}-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).expect("create probe directory");
        dir
    }

    #[test]
    fn initial_acl_failure_preserves_the_old_file_and_writes_no_secret() {
        let dir = probe_dir("initial-failure");
        let path = dir.join("secret.json");
        std::fs::write(&path, b"old").unwrap();
        let calls = Cell::new(0);
        let result = write_private_atomic_with(&path, b"new secret", |_| {
            calls.set(calls.get() + 1);
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "injected ACL failure",
            ))
        });
        assert!(result.is_err());
        assert_eq!(calls.get(), 1);
        assert_eq!(std::fs::read(&path).unwrap(), b"old");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1, "temp leaked");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn final_acl_failure_is_reported_without_deleting_the_new_file() {
        let dir = probe_dir("final-failure");
        let path = dir.join("secret.json");
        std::fs::write(&path, b"old").unwrap();
        let calls = Cell::new(0);
        let result = write_private_atomic_with(&path, b"new secret", |_| {
            calls.set(calls.get() + 1);
            if calls.get() == 2 {
                Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "injected ACL verification failure",
                ))
            } else {
                Ok(())
            }
        });
        assert!(result.is_err(), "final verification failure was hidden");
        assert_eq!(calls.get(), 2);
        assert_eq!(std::fs::read(&path).unwrap(), b"new secret");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1, "temp leaked");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn restrict_removes_a_pre_existing_explicit_everyone_ace() {
        use std::process::{Command, Stdio};

        let dir = std::env::temp_dir().join(format!(
            "flint-owner-only-acl-проверка {}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).expect("create probe directory");
        let path = dir.join("secret file.tmp");
        std::fs::write(&path, b"disposable ACL test; no secret data").expect("create probe");

        let granted = Command::new("icacls")
            .arg(&path)
            .arg("/inheritance:r")
            .arg("/grant:r")
            .arg("*S-1-1-0:F")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("run icacls");
        assert!(granted.success(), "failed to create the vulnerable fixture");

        restrict(&path).expect("replace and verify DACL");
        // A second call must not duplicate ACEs or restore inheritance.
        restrict(&path).expect("idempotent restrict");

        let output = super::powershell_command()
            .args([
                "-Command",
                "(Get-Acl -LiteralPath $env:FLINT_ACL_PATH).Sddl",
            ])
            .env("FLINT_ACL_PATH", &path)
            .output()
            .expect("read SDDL");
        assert!(output.status.success(), "Get-Acl failed");
        let sddl = String::from_utf8(output.stdout).expect("SDDL is ASCII");
        let dacl = sddl.split("D:").nth(1).expect("DACL in SDDL");
        assert!(dacl.starts_with('P'), "inheritance is not disabled: {sddl}");
        assert_eq!(dacl.matches("(A;;").count(), 1, "unexpected ACEs: {sddl}");
        assert!(!dacl.contains(";;;WD)"), "Everyone survived: {sddl}");

        std::fs::remove_dir_all(&dir).expect("remove ACL probe");
    }
}
