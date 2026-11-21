//! "Save as PDF" for reports. Rust shows the native Save dialog and writes the
//! file, so the webview never chooses a path. The PDF is the page's print
//! layout (same as Print), rendered by WebView2's PrintToPdf.

use std::path::{Path, PathBuf};
use tauri::{AppHandle, WebviewWindow};
use tauri_plugin_dialog::DialogExt;

use crate::api::CmdError;

/// Keeps a suggested file name to safe characters and adds `.pdf`.
pub fn safe_file_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '-' | '_' | '(' | ')' | '.'))
        .take(80)
        .collect();
    let base = cleaned.trim().trim_end_matches(".pdf").trim_matches('.').trim();
    format!("{}.pdf", if base.is_empty() { "Clinilog report" } else { base })
}

/// Shows the Save dialog and writes the current page as a PDF.
/// Returns the saved path, or `None` if the user cancelled.
#[tauri::command]
pub async fn save_report_pdf(app: AppHandle, window: WebviewWindow, file_name: String) -> Result<Option<String>, CmdError> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title("Save report as PDF")
        .add_filter("PDF document", &["pdf"])
        .set_file_name(safe_file_name(&file_name))
        .set_parent(&window)
        .save_file(move |picked| {
            let _ = tx.send(picked);
        });
    let picked = rx.await.map_err(|_| CmdError::new("server", "The save dialog closed unexpectedly."))?;
    let Some(picked) = picked else { return Ok(None) };

    let mut path: PathBuf = picked
        .into_path()
        .map_err(|_| CmdError::new("invalid", "That location can't be used. Choose a folder on this PC."))?;
    if path.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("pdf")) != Some(true) {
        path.set_extension("pdf");
    }

    print_to_pdf(&window, &path).await?;
    Ok(Some(path.display().to_string()))
}

#[cfg(windows)]
async fn print_to_pdf(window: &WebviewWindow, path: &Path) -> Result<(), CmdError> {
    use std::sync::{Arc, Mutex};
    use webview2_com::{
        Microsoft::Web::WebView2::Win32::{
            ICoreWebView2Environment6, ICoreWebView2_7, COREWEBVIEW2_PRINT_ORIENTATION_PORTRAIT,
        },
        PrintToPdfCompletedHandler,
    };
    use windows_core::{Interface, HSTRING};

    type Done = Result<(), String>;
    let (tx, rx) = tokio::sync::oneshot::channel::<Done>();
    // Shared so either the setup error path or the completion handler can answer.
    let tx = Arc::new(Mutex::new(Some(tx)));
    let send = {
        let tx = tx.clone();
        move |r: Done| {
            if let Some(t) = tx.lock().unwrap().take() {
                let _ = t.send(r);
            }
        }
    };
    let target = HSTRING::from(path.as_os_str());

    window
        .with_webview(move |wv| {
            let send_done = send.clone();
            let started = (|| -> windows_core::Result<()> {
                unsafe {
                    let core: ICoreWebView2_7 = wv.controller().CoreWebView2()?.cast()?;
                    let env: ICoreWebView2Environment6 = wv.environment().cast()?;
                    let settings = env.CreatePrintSettings()?;
                    settings.SetOrientation(COREWEBVIEW2_PRINT_ORIENTATION_PORTRAIT)?;
                    // A4, in inches.
                    settings.SetPageWidth(8.27)?;
                    settings.SetPageHeight(11.69)?;
                    settings.SetShouldPrintBackgrounds(true)?;
                    settings.SetShouldPrintHeaderAndFooter(false)?;
                    let handler = PrintToPdfCompletedHandler::create(Box::new(move |result, ok| {
                        send_done(if result.is_ok() && ok {
                            Ok(())
                        } else {
                            Err("Windows couldn't create the PDF.".into())
                        });
                        Ok(())
                    }));
                    core.PrintToPdf(&target, &settings, &handler)
                }
            })();
            if let Err(e) = started {
                send(Err(format!("Couldn't start PDF export ({e}).")));
            }
        })
        .map_err(|e| CmdError::new("server", format!("Couldn't reach the window: {e}")))?;

    match rx.await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(msg)) => Err(CmdError::new("server", format!("{msg} Check that the file isn't open in another program."))),
        Err(_) => Err(CmdError::new("server", "PDF export stopped unexpectedly.")),
    }
}

#[cfg(not(windows))]
async fn print_to_pdf(_window: &WebviewWindow, _path: &Path) -> Result<(), CmdError> {
    Err(CmdError::new("server", "Save as PDF is only available on Windows. Use Print instead."))
}

#[cfg(test)]
mod tests {
    use super::safe_file_name;

    #[test]
    fn file_names_are_cleaned() {
        assert_eq!(safe_file_name("Clinilog report - 2026-11"), "Clinilog report - 2026-11.pdf");
        assert_eq!(safe_file_name("report.pdf"), "report.pdf");
        assert_eq!(safe_file_name("..\\..\\Windows\\evil"), "Windowsevil.pdf");
        assert_eq!(safe_file_name("a/b:c*d?\"<>|"), "abcd.pdf");
        assert_eq!(safe_file_name("   "), "Clinilog report.pdf");
        assert!(safe_file_name(&"x".repeat(200)).len() <= 84);
    }
}
