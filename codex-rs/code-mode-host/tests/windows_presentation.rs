#![cfg(windows)]

use std::ffi::c_void;
use std::os::windows::io::AsRawHandle;
use std::os::windows::io::FromRawHandle;
use std::os::windows::io::OwnedHandle;
use std::sync::Arc;
use std::time::Duration;

use codex_code_mode::CodeModeSessionProvider;
use codex_code_mode::ExecuteRequest;
use codex_code_mode::FunctionCallOutputContentItem;
use codex_code_mode::NoopCodeModeSessionDelegate;
use codex_code_mode::ProcessOwnedCodeModeSessionProvider;
use codex_code_mode::RuntimeResponse;
use pretty_assertions::assert_eq;
use tokio::process::Command;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn AttachConsole(process_id: u32) -> i32;
    fn FreeConsole() -> i32;
    fn GetConsoleWindow() -> *mut c_void;
    fn OpenProcess(access: u32, inherit: i32, process_id: u32) -> *mut c_void;
    fn WaitForSingleObject(handle: *mut c_void, milliseconds: u32) -> u32;
    fn TerminateProcess(handle: *mut c_void, exit_code: u32) -> i32;
}

struct HostProcess(OwnedHandle);

impl Drop for HostProcess {
    fn drop(&mut self) {
        // Only a safety net: normal teardown is asserted before this owner is dropped.
        unsafe {
            if WaitForSingleObject(self.0.as_raw_handle(), /*milliseconds*/ 0) == 258 {
                TerminateProcess(self.0.as_raw_handle(), /*exit_code*/ 1);
                WaitForSingleObject(self.0.as_raw_handle(), /*milliseconds*/ 10_000);
            }
        }
    }
}

#[tokio::test]
async fn process_owned_host_has_no_console_window_and_preserves_ipc() -> anyhow::Result<()> {
    if std::env::var_os("CODEX_TEST_CODE_MODE_DETACHED").is_none() {
        let output = Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "process_owned_host_has_no_console_window_and_preserves_ipc",
                "--nocapture",
            ])
            .env("CODEX_TEST_CODE_MODE_DETACHED", "1")
            .creation_flags(0x0000_0008) // DETACHED_PROCESS: reproduce a console-less parent.
            .kill_on_drop(true)
            .output()
            .await?;
        assert!(
            output.status.success() && String::from_utf8_lossy(&output.stdout).contains("1 passed"),
            "detached test did not pass: {output:?}"
        );
        return Ok(());
    }
    assert!(unsafe { GetConsoleWindow() }.is_null());
    let provider = ProcessOwnedCodeModeSessionProvider::with_host_program(
        codex_utils_cargo_bin::cargo_bin("codex-code-mode-host")?,
    );
    let session = provider
        .create_session()
        .await
        .map_err(anyhow::Error::msg)?;

    // Discover only this test's direct host child, without adding a product PID API.
    let parent_id = std::process::id();
    let output = Command::new("powershell.exe")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &format!(
                "Get-CimInstance Win32_Process -Filter \"ParentProcessId = {parent_id} AND Name = 'codex-code-mode-host.exe'\" | ForEach-Object {{ $_.ProcessId }}"
            ),
        ])
        .creation_flags(0x0800_0000)
        .output()
        .await?;
    assert!(output.status.success(), "host discovery failed: {output:?}");
    let host_id: u32 = String::from_utf8(output.stdout)?.trim().parse()?;
    let process = unsafe {
        OpenProcess(/*access*/ 0x0010_0001, /*inherit*/ 0, host_id)
    };
    assert!(!process.is_null(), "host must still be running");
    let process = HostProcess(unsafe { OwnedHandle::from_raw_handle(process) });

    // Inspect the host's console window from this detached test process. Windows
    // can permit attachment to a no-window console, or report ERROR_INVALID_HANDLE.
    let attached = unsafe { AttachConsole(host_id) };
    let error = std::io::Error::last_os_error().raw_os_error();
    let host_console_window = unsafe { GetConsoleWindow() };
    if attached != 0 {
        unsafe { FreeConsole() };
    }
    assert!(
        attached != 0 || error == Some(6),
        "AttachConsole failed: {error:?}"
    );
    assert!(
        host_console_window.is_null(),
        "host allocated a console window"
    );

    let actual = session
        .execute(
            ExecuteRequest {
                tool_call_id: "presentation-probe".to_owned(),
                enabled_tools: Vec::new(),
                source: "text('framed-ipc-ok');".to_owned(),
                yield_time_ms: Some(5_000),
                max_output_tokens: None,
            },
            Arc::new(NoopCodeModeSessionDelegate),
        )
        .await
        .map_err(anyhow::Error::msg)?;
    let cell_id = actual.cell_id.clone();
    let actual = actual
        .initial_response()
        .await
        .map_err(anyhow::Error::msg)?;
    assert_eq!(
        actual,
        RuntimeResponse::Result {
            code_mode_host_duration: actual.code_mode_host_duration(),
            cell_id,
            content_items: vec![FunctionCallOutputContentItem::InputText {
                text: "framed-ipc-ok".to_owned(),
            }],
            error_text: None,
        }
    );
    session.shutdown().await.map_err(anyhow::Error::msg)?;
    drop(session);
    drop(provider);
    let exit = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let status = unsafe {
                WaitForSingleObject(process.0.as_raw_handle(), /*milliseconds*/ 0)
            };
            if status != 258 {
                break status;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await;
    assert_eq!(exit?, 0, "host did not exit after its owner was dropped");
    Ok(())
}
