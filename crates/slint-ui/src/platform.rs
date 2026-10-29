//! Windows-only shell integration: native startup error box and the folder
//! picker. Kept out of `main.rs` so the UI event wiring stays platform-neutral.

#[cfg(windows)]
pub fn show_startup_error(message: &str) {
    use std::ffi::c_void;

    #[link(name = "user32")]
    extern "system" {
        fn MessageBoxW(owner: *mut c_void, text: *const u16, caption: *const u16, kind: u32)
            -> i32;
    }

    let text: Vec<u16> = message.encode_utf16().chain(Some(0)).collect();
    let caption: Vec<u16> = "Flint — ошибка конфигурации\0".encode_utf16().collect();
    unsafe {
        MessageBoxW(std::ptr::null_mut(), text.as_ptr(), caption.as_ptr(), 0x10);
    }
}

#[cfg(not(windows))]
pub fn show_startup_error(message: &str) {
    eprintln!("{message}");
}

#[cfg(windows)]
pub fn pick_data_folder(initial: &str) -> Result<Option<String>, String> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms
$dialog = [System.Windows.Forms.FolderBrowserDialog]::new()
$dialog.Description = '__DESCRIPTION__'
$dialog.ShowNewFolderButton = $true
if ($env:FLINT_INITIAL_DATA_DIR) { $dialog.SelectedPath = $env:FLINT_INITIAL_DATA_DIR }
try {
    if ($dialog.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) {
        [Console]::OutputEncoding = [System.Text.Encoding]::UTF8
        Write-Output $dialog.SelectedPath
    }
} finally {
    $dialog.Dispose()
}
"#;
    let description = if crate::i18n::is_english() {
        "Choose the Flint data directory"
    } else {
        "Выберите каталог данных Flint"
    };
    let script = SCRIPT.replace("__DESCRIPTION__", description);

    let output = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-STA", "-Command", &script])
        .env("FLINT_INITIAL_DATA_DIR", initial)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|error| format!("не удалось открыть диалог выбора каталога: {error}"))?;
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "диалог выбора каталога завершился с ошибкой: {}",
            error.trim()
        ));
    }
    let selected = String::from_utf8(output.stdout)
        .map_err(|_| "диалог выбора каталога вернул путь не в UTF-8".to_string())?;
    let selected = selected.trim().to_string();
    Ok((!selected.is_empty()).then_some(selected))
}

#[cfg(not(windows))]
pub fn pick_data_folder(_initial: &str) -> Result<Option<String>, String> {
    Err("на этой платформе введите абсолютный путь вручную".to_string())
}
