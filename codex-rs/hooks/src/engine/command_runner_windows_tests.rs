//! Functional regression of the real hook process-tree fallback.
//!
//! Root and descendant termination must pass before the test-only safety job is
//! closed. Desktop presentation acceptance belongs exclusively to the external
//! generation-aware observer; this regression emits no presentation verdict.

use std::ffi::c_void;
use std::os::windows::io::AsRawHandle;
use std::os::windows::io::FromRawHandle;
use std::os::windows::io::OwnedHandle;
use std::time::Duration;

use codex_utils_pty::JobObject;
use pretty_assertions::assert_eq;
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;
use tokio::time::timeout;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetConsoleWindow() -> *mut c_void;
    fn OpenProcess(access: u32, inherit: i32, process: u32) -> *mut c_void;
    fn WaitForSingleObject(handle: *mut c_void, milliseconds: u32) -> u32;
}

pub(super) async fn cleanup_fallback_functional() -> anyhow::Result<()> {
    if std::env::var_os("CODEX_TEST_BACKGROUND_CHILD").is_none() {
        let output = tokio::process::Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "engine::command_runner::tests::hook_cleanup_fallback_functional_terminates_owned_tree",
                "--nocapture",
            ])
            .env("CODEX_TEST_BACKGROUND_CHILD", "1")
            .creation_flags(0x0000_0008)
            .kill_on_drop(true)
            .output()
            .await?;
        assert!(
            output.status.success() && String::from_utf8_lossy(&output.stdout).contains("1 passed"),
            "{output:?}"
        );
        return Ok(());
    }
    assert!(unsafe { GetConsoleWindow() }.is_null());
    // Independent test-only owner survives until AFTER the fallback assertions. It
    // contains descendants from birth, including setup failures before their PID is read.
    let cleanup = JobObject::create_without_breakaway()?;
    let mut command = tokio::process::Command::new("powershell.exe");
    command.args(["-NoProfile", "-NonInteractive", "-Command",
        "$c = Start-Process powershell.exe -ArgumentList '-NoProfile -NonInteractive -Command Start-Sleep -Seconds 60' -NoNewWindow -PassThru; [Console]::Out.WriteLine($c.Id); $c.WaitForExit()"])
        .stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null());
    let mut child =
        cleanup.spawn_contained_with_flags(&mut command, super::super::CREATE_NO_WINDOW)?;
    let mut line = String::new();
    timeout(
        Duration::from_secs(10),
        BufReader::new(child.stdout.take().expect("child stdout")).read_line(&mut line),
    )
    .await??;
    let root_pid = child.id().expect("owned root PID");
    let descendant_pid = line.trim().parse()?;
    let descendant = unsafe {
        OpenProcess(
            /*access*/ 0x0010_0000,
            /*inherit*/ 0,
            descendant_pid,
        )
    };
    assert!(!descendant.is_null(), "owned descendant must be running");
    let descendant = unsafe { OwnedHandle::from_raw_handle(descendant) };
    assert_eq!(
        unsafe {
            WaitForSingleObject(descendant.as_raw_handle(), /*milliseconds*/ 0)
        },
        258
    );
    drop(super::super::ProcessTreeGuard {
        process_id: child.id(),
        job: None,
    });
    let status = timeout(Duration::from_secs(10), child.wait()).await??;
    assert!(!status.success());
    assert_eq!(
        unsafe {
            WaitForSingleObject(descendant.as_raw_handle(), /*milliseconds*/ 10_000)
        },
        0,
        "fallback must terminate the descendant before test cleanup"
    );
    println!(
        "HOOK_FUNCTIONAL_RESULT={}",
        serde_json::json!({
            "process_tree_guard_job_none": true,
            "root_pid": root_pid,
            "descendant_pid": descendant_pid,
            "root_terminated": true,
            "descendant_terminated": true,
            "asserted_before_safety_cleanup": true,
        })
    );
    // Closing this job cannot contribute to the success assertions above.
    drop(cleanup);
    Ok(())
}
