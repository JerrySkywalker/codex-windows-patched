use pretty_assertions::assert_eq;
use std::ffi::c_void;
use std::os::windows::process::CommandExt;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn AttachConsole(process_id: u32) -> i32;
    fn FreeConsole() -> i32;
    fn GetConsoleWindow() -> *mut c_void;
}

#[test]
fn parser_from_detached_parent_preserves_protocol() -> anyhow::Result<()> {
    if std::env::var_os("CODEX_TEST_BACKGROUND_CHILD").is_none() {
        let output=std::process::Command::new(std::env::current_exe()?)
            .args(["--exact","command_safety::powershell_parser::windows_presentation_tests::parser_from_detached_parent_preserves_protocol","--nocapture"])
            .env("CODEX_TEST_BACKGROUND_CHILD","1").creation_flags(0x0000_0008).output()?;
        assert!(
            output.status.success() && String::from_utf8_lossy(&output.stdout).contains("1 passed"),
            "{output:?}"
        );
        return Ok(());
    }
    assert!(unsafe { GetConsoleWindow() }.is_null());
    let mut parser = super::PowershellParserProcess::spawn("powershell.exe")?;
    for command in ["Get-Date", "Get-Location"] {
        assert_eq!(
            parser.parse(command)?,
            super::PowershellParseOutcome::Commands(vec![vec![command.to_owned()]])
        );
    }
    let attached = unsafe { AttachConsole(parser.child.id()) };
    let error = std::io::Error::last_os_error().raw_os_error();
    let window = unsafe { GetConsoleWindow() };
    if attached != 0 {
        unsafe { FreeConsole() };
    }
    assert!(
        attached != 0 || error == Some(6),
        "AttachConsole failed: {error:?}"
    );
    assert!(
        window.is_null(),
        "background parser allocated a console window"
    );
    Ok(())
}
