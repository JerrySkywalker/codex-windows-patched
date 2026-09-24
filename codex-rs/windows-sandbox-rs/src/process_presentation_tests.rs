use pretty_assertions::assert_eq;
use std::io::Read;
use std::os::windows::io::AsRawHandle;
use std::os::windows::io::FromRawHandle;
use std::os::windows::io::OwnedHandle;
use std::os::windows::process::CommandExt;
use windows_sys::Win32::System::Threading::GetExitCodeProcess;
use windows_sys::Win32::System::Threading::WaitForSingleObject;

#[test]
fn captured_process_creation_from_detached_parent() -> anyhow::Result<()> {
    if std::env::var_os("CODEX_TEST_BACKGROUND_CHILD").is_none() {
        let output=std::process::Command::new(std::env::current_exe()?)
            .args(["--exact","process::windows_presentation_tests::captured_process_creation_from_detached_parent","--nocapture"])
            .env("CODEX_TEST_BACKGROUND_CHILD","1").creation_flags(0x0000_0008).output()?;
        assert!(
            output.status.success() && String::from_utf8_lossy(&output.stdout).contains("1 passed"),
            "{output:?}"
        );
        return Ok(());
    }
    // Exercise the real CreateProcessAsUser boundary with this test's own token.
    // No sandbox account setup, ACL grants, shared desktop mutation, or production home.
    let token = unsafe {
        OwnedHandle::from_raw_handle(crate::token::get_current_token_for_restriction()? as _)
    };
    let cwd = tempfile::tempdir()?;
    let script = r#"Add-Type -Name Probe -Namespace PipePresentation -MemberDefinition '[System.Runtime.InteropServices.DllImport("kernel32.dll")] public static extern System.IntPtr GetConsoleWindow();'; [Console]::Out.Write('console={0};stdin={1}',[PipePresentation.Probe]::GetConsoleWindow().ToInt64(),[Console]::In.Read()); [Console]::Error.Write('captured stderr'); exit 23"#;
    let handles = super::spawn_process_with_pipes(
        token.as_raw_handle() as _,
        &[
            "powershell.exe".to_owned(),
            "-NoProfile".to_owned(),
            "-NonInteractive".to_owned(),
            "-Command".to_owned(),
            script.to_owned(),
        ],
        cwd.path(),
        &std::env::vars().collect(),
        super::StdinMode::Closed,
        super::StderrMode::Separate,
        super::ConsoleMode::NoWindow,
        crate::desktop::LaunchDesktop::prepare(/*logs_base_dir*/ None)?,
        /*logs_base_dir*/ None,
    )?;
    let process = unsafe { OwnedHandle::from_raw_handle(handles.process.hProcess as _) };
    let _thread = unsafe { OwnedHandle::from_raw_handle(handles.process.hThread as _) };
    let mut stdout = unsafe { std::fs::File::from_raw_handle(handles.stdout_read as _) };
    let mut stderr = unsafe {
        std::fs::File::from_raw_handle(handles.stderr_read.expect("separate stderr") as _)
    };
    assert_eq!(
        unsafe { WaitForSingleObject(process.as_raw_handle() as _, 10_000) },
        0
    );
    let mut code = 0;
    assert_ne!(
        unsafe { GetExitCodeProcess(process.as_raw_handle() as _, &mut code) },
        0
    );
    let mut out = String::new();
    let mut err = String::new();
    stdout.read_to_string(&mut out)?;
    stderr.read_to_string(&mut err)?;
    assert_eq!(
        (code, out, err),
        (
            23,
            "console=0;stdin=-1".to_owned(),
            "captured stderr".to_owned()
        )
    );
    drop(handles); // The job remains an independent cleanup owner through all assertions.
    Ok(())
}
