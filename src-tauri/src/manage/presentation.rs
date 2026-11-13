use super::ManageItem;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub(super) fn read_label(key: &str, value: &str) -> Option<String> {
    let raw = crate::regscan::read_string(key, value)?;
    label(&raw)
}

fn label(raw: &str) -> Option<String> {
    let text = raw.trim();
    if text.is_empty() {
        return None;
    }
    if let Some(reference) = text.strip_prefix('@') {
        return resource_string(reference);
    }
    (!text.to_ascii_lowercase().starts_with("ms-resource:")).then(|| text.to_owned())
}

#[cfg(windows)]
fn resource_string(reference: &str) -> Option<String> {
    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::Foundation::FreeLibrary;
    use windows::Win32::System::LibraryLoader::{
        LoadLibraryExW, LOAD_LIBRARY_AS_DATAFILE, LOAD_LIBRARY_AS_IMAGE_RESOURCE,
    };
    use windows::Win32::UI::WindowsAndMessaging::LoadStringW;

    let (module, id) = reference.split(';').next()?.rsplit_once(',')?;
    let id = id.trim().strip_prefix('-')?.parse::<u16>().ok()?;
    let module = expanded_path(module.trim().trim_matches('"'));
    if !matches!(
        extension(Path::new(&module)).as_str(),
        "dll" | "exe" | "mui"
    ) {
        return None;
    }
    let module = Path::new(&module);
    let _pins = crate::fsutil::pin_local_scan_path(module).ok()?;
    let wide: Vec<u16> = module.to_str()?.encode_utf16().chain([0]).collect();
    // Resource mapping gives LoadStringW the Windows MUI fallback and does
    // not run module code or load its imports.
    let handle = unsafe {
        LoadLibraryExW(
            PCWSTR(wide.as_ptr()),
            None,
            LOAD_LIBRARY_AS_DATAFILE | LOAD_LIBRARY_AS_IMAGE_RESOURCE,
        )
    }
    .ok()?;
    let mut buffer = [0; 4096];
    let count = unsafe {
        LoadStringW(
            handle,
            u32::from(id),
            PWSTR(buffer.as_mut_ptr()),
            buffer.len() as i32,
        )
    };
    let _ = unsafe { FreeLibrary(handle) };
    let value = String::from_utf16_lossy(&buffer[..count as usize]);
    let value = value.trim();
    (!value.is_empty() && !value.starts_with('@')).then(|| value.to_owned())
}

#[cfg(not(windows))]
fn resource_string(_: &str) -> Option<String> {
    None
}

fn expanded_path(raw: &str) -> String {
    let system32 = crate::regops::system32_dir();
    let windows = Path::new(&system32).parent().and_then(Path::to_str);
    let lower = raw.to_ascii_lowercase();
    if let Some(windows) = windows {
        for prefix in ["%systemroot%", "%windir%", "\\systemroot"] {
            if lower
                .strip_prefix(prefix)
                .is_some_and(|rest| rest.starts_with(['\\', '/']))
            {
                return format!("{windows}{}", &raw[prefix.len()..]);
            }
        }
        if lower.starts_with("system32\\") {
            return format!("{windows}\\{raw}");
        }
    }
    crate::regops::expand_env_string(raw)
}

fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

fn binary_path(path: &Path) -> Option<String> {
    if !matches!(extension(path).as_str(), "exe" | "dll" | "ico") {
        return None;
    }
    let path = path.to_str()?;
    crate::fsutil::is_local_drive_path(path).then(|| path.to_owned())
}

pub(super) fn startup_file(path: &Path) -> (Option<String>, Option<String>) {
    #[cfg(windows)]
    if extension(path) == "lnk" {
        return shortcut(path).unwrap_or_default();
    }
    let binary = binary_path(path);
    (binary.clone(), binary)
}

#[cfg(windows)]
pub(crate) fn shortcut(path: &Path) -> Option<(Option<String>, Option<String>)> {
    use windows::core::{Interface, PCWSTR};
    use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED, STGM_READ,
    };
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink, SLGP_RAWPATH};

    let _pins = crate::fsutil::pin_local_scan_path(path).ok()?;
    let wide: Vec<u16> = path.to_str()?.encode_utf16().chain([0]).collect();
    struct ComInit(bool);
    impl Drop for ComInit {
        fn drop(&mut self) {
            if self.0 {
                unsafe { CoUninitialize() };
            }
        }
    }
    let init = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    if init.is_err() && init != RPC_E_CHANGED_MODE {
        return None;
    }
    let _com = ComInit(init.is_ok());
    let link: IShellLinkW =
        unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }.ok()?;
    let persisted: IPersistFile = link.cast().ok()?;
    unsafe { persisted.Load(PCWSTR(wide.as_ptr()), STGM_READ) }.ok()?;

    let local_binary = |buffer: &[u16]| {
        let end = buffer.iter().position(|code| *code == 0)?;
        let value = String::from_utf16(&buffer[..end]).ok()?;
        binary_path(Path::new(&expanded_path(&value)))
    };
    let mut buffer = [0; 260];
    // Read the stored path without resolving or activating the target.
    let target = unsafe { link.GetPath(&mut buffer, std::ptr::null_mut(), SLGP_RAWPATH.0 as u32) }
        .ok()
        .and_then(|_| local_binary(&buffer));
    buffer.fill(0);
    let mut index = 0;
    let icon = unsafe { link.GetIconLocation(&mut buffer, &mut index) }
        .ok()
        .and_then(|_| local_binary(&buffer))
        .map(|path| format!("{path},{index}"))
        .or_else(|| target.clone());
    Some((target, icon))
}

pub(super) fn command_path(command: &str) -> Option<String> {
    let command = expanded_path(command.trim());
    let path = crate::scanner::reg_scans::service_image_exe(&command)?;
    binary_path(&path)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TaskAction {
    Program { program: String, script_host: bool },
    Multiple,
}

pub(super) fn fill_task(item: &mut ManageItem, command: &str, comment: &str) {
    item.display_name = item
        .name
        .rsplit('\\')
        .next()
        .filter(|name| !name.is_empty())
        .map(str::to_owned);
    item.detail = if comment.trim().eq_ignore_ascii_case("N/A") {
        String::new()
    } else {
        label(comment)
            .unwrap_or_default()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    };

    let Some(executable) = crate::scanner::reg_scans::image_exe_token(command) else {
        return;
    };
    let executable = expanded_path(executable);
    let target = Path::new(&executable);
    if executable.contains('%') || extension(target) != "exe" {
        return;
    }
    let Some(program) = target.file_name().and_then(|name| name.to_str()) else {
        return;
    };
    if program
        .chars()
        .any(|ch| ch.is_control() || matches!(ch, '"' | '<' | '>' | '|' | '?' | '*' | ':'))
        || (target
            .parent()
            .is_some_and(|parent| !parent.as_os_str().is_empty())
            && !target.is_absolute())
    {
        return;
    }
    let script_host = matches!(
        program.to_ascii_lowercase().as_str(),
        "cmd.exe"
            | "powershell.exe"
            | "pwsh.exe"
            | "wscript.exe"
            | "cscript.exe"
            | "mshta.exe"
            | "rundll32.exe"
            | "python.exe"
            | "pythonw.exe"
            | "py.exe"
            | "node.exe"
    );
    item.task_action = Some(TaskAction::Program {
        program: program.to_owned(),
        script_host,
    });
    item.path = binary_path(target);
    // The shell's logo would identify the interpreter, not what the task runs.
    if !script_host {
        item.display_icon = item.path.clone();
    }
}

pub(super) fn mark_multiple_task_actions(item: &mut ManageItem) {
    item.task_action = Some(TaskAction::Multiple);
    item.display_icon = None;
    item.path = None;
}

fn service_dll(key: &str) -> Option<String> {
    let path = crate::regscan::read_string(&format!("{key}\\Parameters"), "ServiceDll")?;
    let path = expanded_path(path.trim().trim_matches('"'));
    binary_path(Path::new(&path))
}

fn is_system_path(path: &str) -> bool {
    let system = PathBuf::from(crate::regops::system32_dir());
    let Some(windows) = system.parent() else {
        return false;
    };
    let path = path.replace('/', "\\").to_ascii_lowercase();
    let windows = windows.to_string_lossy().to_ascii_lowercase();
    ["system32", "syswow64", "winsxs", "servicing", "systemapps"]
        .iter()
        .any(|directory| path.starts_with(&format!("{windows}\\{directory}\\")))
}

pub(super) fn fill_service(item: &mut ManageItem, key: &str, service_type: u32) {
    let image = crate::regscan::read_string(key, "ImagePath").and_then(|raw| command_path(&raw));
    let dll = service_dll(key);
    let host_only = image.as_deref().is_some_and(|path| {
        Path::new(path)
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("svchost.exe"))
    });
    let binary = if host_only || image.is_none() {
        dll.as_deref().or(image.as_deref())
    } else {
        image.as_deref()
    };
    item.system_service = binary
        .filter(|_| !host_only || dll.is_some())
        .map(is_system_path);
    item.display_icon = binary
        .filter(|_| !host_only || dll.is_some())
        .map(str::to_owned);
    item.path = binary.map(str::to_owned);

    item.display_name = read_label(key, "DisplayName");
    item.detail = read_label(key, "Description").unwrap_or_default();
    // A per-user service instance can omit metadata supplied by its template.
    if service_type & 0x80 != 0 {
        if let Some((base, suffix)) = item.name.rsplit_once('_') {
            if !suffix.is_empty() && suffix.bytes().all(|c| c.is_ascii_hexdigit()) {
                if let Some((parent, _)) = key.rsplit_once('\\') {
                    let template = format!("{parent}\\{base}");
                    if item
                        .display_name
                        .as_deref()
                        .is_none_or(|name| name == item.name)
                    {
                        item.display_name = read_label(&template, "DisplayName");
                    }
                    if item.detail.is_empty() {
                        item.detail = read_label(&template, "Description").unwrap_or_default();
                    }
                    if item.system_service.is_none() {
                        if let Some(dll) = service_dll(&template) {
                            item.system_service = Some(is_system_path(&dll));
                            item.display_icon = Some(dll.clone());
                            item.path = Some(dll);
                        }
                    }
                }
            }
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn task_rows_keep_description_and_action_separate_from_the_registered_name() {
        let mut item = ManageItem {
            name: r"\Vendor\UpdateAgent".into(),
            location: r"\Vendor\UpdateAgent".into(),
            ..Default::default()
        };
        let mut script = item.clone();
        let program = crate::regops::sys_tool("explorer.exe");
        fill_task(
            &mut item,
            &format!(r#""{program}" --flag"#),
            " Check for updates ",
        );
        assert_eq!(item.display_name.as_deref(), Some("UpdateAgent"));
        assert_eq!(item.name, r"\Vendor\UpdateAgent");
        assert_eq!(item.detail, "Check for updates");
        assert_eq!(item.display_icon.as_deref(), Some(program.as_str()));
        assert!(matches!(
            item.task_action,
            Some(TaskAction::Program {
                script_host: false,
                ..
            })
        ));

        fill_task(
            &mut script,
            r#"powershell.exe -File "%UNKNOWN_SCRIPT%""#,
            "N/A",
        );
        assert!(script.detail.is_empty());
        assert!(script.display_icon.is_none());
        assert_eq!(
            script.task_action,
            Some(TaskAction::Program {
                program: "powershell.exe".into(),
                script_host: true
            })
        );
        mark_multiple_task_actions(&mut item);
        assert_eq!(item.task_action, Some(TaskAction::Multiple));
        assert!(item.display_icon.is_none());
        assert!(item.path.is_none());

        let mut unresolved = ManageItem::default();
        fill_task(&mut unresolved, "COM handler", "N/A");
        assert!(unresolved.task_action.is_none());
        assert!(unresolved.display_icon.is_none());
    }

    #[test]
    fn service_resource_labels_never_display_unresolved_references() {
        assert_eq!(
            label("  Vendor Background Agent  ").as_deref(),
            Some("Vendor Background Agent")
        );
        assert_eq!(label(" "), None);
        assert_eq!(
            label("@%SystemRoot%\\System32\\remova-missing.dll,-1"),
            None
        );
        assert_eq!(label("@\\\\server\\share\\audio.dll,-1"), None);
        assert_eq!(label("ms-resource:SomeDisplayName"), None);
    }

    #[test]
    fn windows_audio_resource_resolves_in_services_and_startup() {
        let services = super::super::list_services();
        let startup = super::super::list_auto_services();
        for key in ["AudioEndpointBuilder", "Audiosrv"] {
            let Some(service) = services.iter().find(|item| item.name == key) else {
                continue; // Trimmed Windows images may omit these services.
            };
            let name = service
                .display_name
                .as_deref()
                .expect("Windows audio display name");
            assert!(!name.starts_with('@'));
            assert!(!service.detail.is_empty() && !service.detail.starts_with('@'));
            assert_eq!(service.system_service, Some(true));
            if let Some(entry) = startup.iter().find(|item| item.name == key) {
                assert_eq!(entry.display_name, service.display_name);
                assert_eq!(entry.detail, service.detail);
                assert_eq!(entry.location, format!("SVC::{key}"));
            }
            println!("{key} → {name} · {}", service.detail);
        }
    }

    #[test]
    fn startup_shortcut_reads_the_stored_target_and_icon() {
        use windows::core::{Interface, PCWSTR};
        use windows::Win32::System::Com::{
            CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile, CLSCTX_INPROC_SERVER,
            COINIT_APARTMENTTHREADED,
        };
        use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};

        let fixture = crate::test_support::TempDir::new("startup-icon");
        let shortcut = fixture.join("Startup link.lnk");
        let target = crate::regops::sys_tool("cmd.exe");
        let icon = crate::regops::sys_tool("shell32.dll");
        let wide = |value: &str| value.encode_utf16().chain([0]).collect::<Vec<_>>();
        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().unwrap();
            {
                let link: IShellLinkW =
                    CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).unwrap();
                link.SetPath(PCWSTR(wide(&target).as_ptr())).unwrap();
                link.SetIconLocation(PCWSTR(wide(&icon).as_ptr()), 0)
                    .unwrap();
                let file: IPersistFile = link.cast().unwrap();
                file.Save(PCWSTR(wide(shortcut.to_str().unwrap()).as_ptr()), true)
                    .unwrap();
            }
            CoUninitialize();
        }
        let (actual_target, actual_icon) = startup_file(&shortcut);
        assert!(actual_target.unwrap().eq_ignore_ascii_case(&target));
        let icon_ref = actual_icon.unwrap();
        assert!(icon_ref.eq_ignore_ascii_case(&format!("{icon},0")));
        assert!(crate::icon::extract_icon_png(&icon_ref, true).is_some());
    }
}
