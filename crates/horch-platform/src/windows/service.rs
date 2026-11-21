use std::ffi::OsString;
use std::io;
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::Security::TOKEN_QUERY;
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows_sys::Win32::UI::Shell::GetUserProfileDirectoryW;

use crate::service::{invalid, utf8, xml_escape, Spec, Step};
use crate::NotSupported;

pub const MANAGER: &str = "schtasks";

pub fn default_folder(home: &Path) -> PathBuf {
    home.join("AppData")
        .join("Local")
        .join("horch")
        .join("service")
}

const HEAD: &str = "<?xml version=\"1.0\" encoding=\"UTF-16\"?>\n\
<Task version=\"1.2\" xmlns=\"http://schemas.microsoft.com/windows/2004/02/mit/task\">\n";

const PRINCIPALS: &str = "  <Principals>
    <Principal id=\"Author\">
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>LeastPrivilege</RunLevel>
    </Principal>
  </Principals>
";

const AGENT_TRIGGERS: &str = "  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
    </LogonTrigger>
  </Triggers>
";

const AGENT_SETTINGS: &str = "  <Settings>
    <MultipleInstancesPolicy>StopExisting</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <StartWhenAvailable>true</StartWhenAvailable>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <RestartOnFailure>
      <Interval>PT1M</Interval>
      <Count>999</Count>
    </RestartOnFailure>
  </Settings>
";

const CHECK_TRIGGERS: &str = "  <Triggers>
    <TimeTrigger>
      <Enabled>true</Enabled>
      <StartBoundary>2000-01-01T00:00:00</StartBoundary>
      <Repetition>
        <Interval>PT1M</Interval>
      </Repetition>
    </TimeTrigger>
    <TimeTrigger>
      <Enabled>true</Enabled>
      <StartBoundary>2000-01-01T00:00:30</StartBoundary>
      <Repetition>
        <Interval>PT1M</Interval>
      </Repetition>
    </TimeTrigger>
  </Triggers>
";

const CHECK_SETTINGS: &str = "  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <ExecutionTimeLimit>PT1M</ExecutionTimeLimit>
  </Settings>
";

pub fn definitions(spec: &Spec) -> io::Result<Vec<(PathBuf, String)>> {
    if !spec.env.is_empty() {
        return Err(invalid(
            "a Windows task carries no environment: the service mode has the default ledger \
             and no FLEET_ROOT, so its env is empty"
                .to_string(),
        ));
    }
    let exe = utf8(&spec.exe)?;
    Ok(vec![
        (
            xml_path(spec, &spec.label),
            task(AGENT_TRIGGERS, AGENT_SETTINGS, exe, "fleetd"),
        ),
        (
            xml_path(spec, &check_name(spec)),
            task(CHECK_TRIGGERS, CHECK_SETTINGS, exe, "fleetd check"),
        ),
    ])
}

pub fn write_definition(path: &Path, text: &str) -> io::Result<()> {
    let mut bytes = vec![0xFF, 0xFE];
    for unit in text.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    std::fs::write(path, bytes)
}

pub fn ensure_steps(spec: &Spec) -> Vec<Step> {
    let mut commands: Vec<Vec<OsString>> = [spec.label.clone(), check_name(spec)]
        .into_iter()
        .map(|name| {
            vec![
                "/Create".into(),
                "/XML".into(),
                xml_path(spec, &name).into_os_string(),
                "/TN".into(),
                name.into(),
                "/F".into(),
            ]
        })
        .collect();
    commands.extend(restart_commands(spec));
    commands
        .into_iter()
        .map(|args| Step {
            args,
            nothing_to_do: &[],
        })
        .collect()
}

pub fn restart_commands(spec: &Spec) -> Vec<Vec<OsString>> {
    vec![vec!["/Run".into(), "/TN".into(), spec.label.clone().into()]]
}

pub fn watchdog_ping() -> Result<io::Result<()>, NotSupported> {
    Err(NotSupported::new("service", "windows"))
}

pub fn os_user_home() -> io::Result<PathBuf> {
    let mut token: HANDLE = std::ptr::null_mut();
    // SAFETY: GetCurrentProcess gives a pseudo handle that needs no close,
    // and `token` is a valid place for the handle that the call opens.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let home = profile_dir(token);
    // SAFETY: `token` is the handle that OpenProcessToken opened, and it is
    // closed once, here.
    unsafe { CloseHandle(token) };
    home
}

fn profile_dir(token: HANDLE) -> io::Result<PathBuf> {
    let mut len: u32 = 0;
    // SAFETY: a null buffer of size 0 asks for the size, which the call
    // writes to `len`.
    unsafe { GetUserProfileDirectoryW(token, std::ptr::null_mut(), &mut len) };
    if len == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut buf = vec![0u16; len as usize];
    // SAFETY: `buf` has room for `len` UTF-16 units, and `len` states it.
    if unsafe { GetUserProfileDirectoryW(token, buf.as_mut_ptr(), &mut len) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Ok(PathBuf::from(OsString::from_wide(&buf[..end])))
}

fn check_name(spec: &Spec) -> String {
    format!("{}.check", spec.label)
}

fn xml_path(spec: &Spec, name: &str) -> PathBuf {
    spec.folder.join(format!("{name}.xml"))
}

fn task(triggers: &str, settings: &str, exe: &str, arguments: &str) -> String {
    format!(
        "{HEAD}  <RegistrationInfo>
    <Description>horch {arguments}</Description>
  </RegistrationInfo>
{triggers}{PRINCIPALS}{settings}  <Actions Context=\"Author\">
    <Exec>
      <Command>{}</Command>
      <Arguments>{arguments}</Arguments>
    </Exec>
  </Actions>
</Task>
",
        xml_escape(exe)
    )
}
