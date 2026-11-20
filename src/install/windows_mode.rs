//! Switch shell integrations while retaining binaries, preferences and search indexes.
use super::*;
use std::os::windows::fs::OpenOptionsExt;

fn controller() -> PathBuf {
    program_files().join(r"WinRust\WinRust-setup.exe")
}

pub(super) fn install_controller() -> Result<(), String> {
    let dest = controller();
    fs::create_dir_all(dest.parent().unwrap()).map_err(|e| e.to_string())?;
    let src = env::current_exe().map_err(|e| e.to_string())?;
    if src.canonicalize().ok() != dest.canonicalize().ok() {
        put(&src, &dest)?;
    }
    Ok(())
}

fn remove_if_present(path: &Path) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

pub(super) fn remove_link(name: &str) -> Result<(), String> {
    remove_if_present(&start_lnk(name))
}

/// The persisted list is restricted to known integrations. Publish it before changing any registrations.
fn save_plan(dir: &Path) -> Result<(), String> {
    if dir.is_dir() {
        return Ok(());
    }
    let stage = dir.with_extension(format!("new-{}", std::process::id()));
    fs::create_dir(&stage).map_err(|e| e.to_string())?;
    for i in ready() {
        let p = &PLUGINS[i];
        if (p.installed)() && (p.name != "wiztree" || start_lnk("WinRust Disk").is_file()) {
            fs::write(stage.join(p.name), []).map_err(|e| e.to_string())?;
        }
    }
    if reg_str(RUN_KEY, "RTaskMgr").is_some_and(|s| s.to_lowercase().contains("rtaskmgr-win32.exe")) {
        fs::write(stage.join("taskmgr-autostart"), []).map_err(|e| e.to_string())?;
    }
    fs::rename(stage, dir).map_err(|e| e.to_string())
}

fn disable(name: &str) -> Result<(), String> {
    match name {
        "rust-explorer" => rx_release(true),
        "topbar" => bar_release(true),
        "rust-search" => search_release(true),
        "rtaskmgr-win32" => {
            if reg_str(RUN_KEY, "RTaskMgr").is_some_and(|s| s.to_lowercase().contains("rtaskmgr-win32.exe")) {
                run("reg", &["delete", RUN_KEY, "/v", "RTaskMgr", "/f"])?;
            }
            tm_release(true)
        }
        "photoview" => pv_release(true),
        "wiztree" => remove_link("WinRust Disk"),
        _ => Err(format!("Unknown integration: {name}")),
    }
}

// Check the taskbar's owner, since a folder window also runs explorer.exe. Launch the real shell
// with the signed-in user's limited token if the normal handover hasn't already started it.
fn restore_shell() -> Result<(), String> {
    let script = task_user()
        + r#"
$ErrorActionPreference='Stop'
Add-Type -Namespace WinRustMode -Name Shell -MemberDefinition '[System.Runtime.InteropServices.DllImport("user32.dll",CharSet=System.Runtime.InteropServices.CharSet.Unicode)] public static extern System.IntPtr FindWindow(string c, System.IntPtr t); [System.Runtime.InteropServices.DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(System.IntPtr h,out uint p);'
function NativeBar {
    $w=[WinRustMode.Shell]::FindWindow('Shell_TrayWnd',[IntPtr]::Zero)
    $owner=0u
    [void][WinRustMode.Shell]::GetWindowThreadProcessId($w,[ref]$owner)
    $owner -ne 0 -and (Get-Process -Id $owner -ErrorAction SilentlyContinue).Path -eq (Join-Path $env:windir 'explorer.exe')
}
if(NativeBar){exit 0}
$name='WinRust-restore-'+[guid]::NewGuid().ToString('N')
$action=New-ScheduledTaskAction -Execute (Join-Path $env:windir 'explorer.exe')
$principal=New-ScheduledTaskPrincipal -UserId $u -LogonType Interactive -RunLevel Limited
Register-ScheduledTask -TaskName $name -Action $action -Principal $principal | Out-Null
try {
    Start-ScheduledTask -TaskName $name
    for($i=0; $i -lt 80; $i++){ if(NativeBar){return}; Start-Sleep -Milliseconds 250 }
    throw 'The Windows taskbar did not appear. Start explorer.exe from Task Manager or sign out and back in.'
} finally { Unregister-ScheduledTask -TaskName $name -Confirm:$false }
"#;
    run("powershell", &["-NoProfile", "-NonInteractive", "-Command", &script])
}

/// Check the registrations that could take over a Windows tool again at next sign-in.
fn verify_disabled(plan: &Path) -> Result<(), String> {
    let active = |plugin: &str| plan.join(plugin).is_file();
    let mut remaining = Vec::new();
    let mut task_names = Vec::new();
    for (plugin, task, runs) in [
        ("rust-explorer", "rs-overlay", &["rs-overlay"][..]),
        ("topbar", "topbar", &["topbar", "reshell"][..]),
        ("rtaskmgr-win32", "rtaskmgr", &["RTaskMgr"][..]),
        ("rust-search", "", &["rs-launcher"][..]),
    ] {
        if !active(plugin) {
            continue;
        }
        if !task.is_empty() {
            task_names.push(task);
        }
        for value in runs {
            if reg_str(RUN_KEY, value).is_some_and(|s| *value != "RTaskMgr" || s.to_lowercase().contains("rtaskmgr-win32.exe")) {
                remaining.push((*value).to_string());
            }
        }
    }
    for task in task_names {
        if run("schtasks", &["/query", "/tn", task]).is_ok() {
            remaining.push(format!("scheduled task {task}"));
        }
    }
    if active("rust-explorer") {
        if reg_dword(RX_SWITCH, "ReplaceExplorer") == Some(1) {
            remaining.push("desktop replacement".into());
        }
        for key in RX_FOLDERS {
            if out("reg", &["query", &format!(r"{key}\command"), "/ve"]).is_ok_and(|s| s.to_lowercase().contains("rust-explorer.exe")) {
                remaining.push(key.to_owned());
            }
        }
    }
    if active("rtaskmgr-win32") && tm_installed() {
        remaining.push("Task Manager debugger".into());
    }
    if active("rust-search") && reg_str(SH_KEY, "Debugger").is_some_and(|v| v.eq_ignore_ascii_case("cmd.exe /c exit")) {
        remaining.push("Windows Search redirect".into());
    }
    if active("photoview") && reg_str(r"HKLM\SOFTWARE\RegisteredApplications", PV_APP).is_some() {
        remaining.push(PV_APP.into());
    }
    if remaining.is_empty() {
        Ok(())
    } else {
        Err(format!("Still active: {}", remaining.join(", ")))
    }
}

fn switch(enable: bool) -> Result<String, String> {
    let session = init()?;
    let root = controller().parent().unwrap().to_owned();
    let _lock = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .share_mode(0)
        .open(root.join("mode.lock"))
        .map_err(|e| format!("Could not start the switch: {e}"))?;
    let plan = root.join(format!("paused-{}", session.sid));
    let retry = "Restore Windows tools";
    let resume = "Enable WinRust";
    if enable && !plan.is_dir() {
        return Ok("WinRust has no disabled integrations.".into());
    }
    if !enable {
        save_plan(&plan)?;
        shortcut(&start_lnk(resume), &controller(), "--enable-winrust")?;
        shortcut(&start_lnk(retry), &controller(), "--windows-tools")?;
    }
    let shell = plan.join("rust-explorer").is_file() || plan.join("topbar").is_file();
    let choose_photo = !enable
        && plan.join("photoview").is_file()
        && PV_EXTS.iter().any(|ext| {
            reg_str(
                &format!(r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts\.{ext}\UserChoice"),
                "ProgId",
            )
            .as_deref()
                == Some(PV_PROG)
        });
    let mut order: Vec<_> = ready().collect();
    if !enable {
        order.reverse();
    }
    let mut notes = Vec::new();
    let mut errors = Vec::new();
    let mut shell_dependency_failed = false;
    for i in order {
        let p = &PLUGINS[i];
        let flag = plan.join(p.name);
        if !flag.is_file() {
            continue;
        }
        if shell_dependency_failed && ((!enable && p.name == "topbar") || (enable && p.name == "rust-explorer")) {
            continue;
        }
        WARN.lock().unwrap().clear();
        let result = if enable {
            if p.name == "wiztree" {
                shortcut(&start_lnk("WinRust Disk"), &wz_dir().join("wiztree.exe"), "")
            } else {
                (p.actions.unwrap().0)()
            }
        } else {
            disable(p.name)
        };
        if let Err(e) = result {
            if p.name == "rust-explorer" || p.name == "topbar" {
                shell_dependency_failed = true;
            }
            errors.push(format!("{}: {e}", p.name));
            continue;
        }
        notes.extend(WARN.lock().unwrap().drain(..));
        if enable {
            remove_if_present(&flag)?;
        }
    }
    if !enable {
        if shell && !shell_dependency_failed {
            if let Err(e) = restore_shell() {
                errors.push(e);
            }
        }
        if let Err(e) = verify_disabled(&plan) {
            errors.push(e);
        }
    }
    if !errors.is_empty() {
        return Err(errors.join("\n"));
    }
    if enable {
        let flag = plan.join("taskmgr-autostart");
        if flag.is_file() {
            run(
                "reg",
                &[
                    "add",
                    RUN_KEY,
                    "/v",
                    "RTaskMgr",
                    "/t",
                    "REG_SZ",
                    "/d",
                    &format!("\"{}\" --tray", tm_dir().join("rtaskmgr-win32.exe").display()),
                    "/f",
                ],
            )?;
            remove_if_present(&flag)?;
        }
        fs::remove_dir(&plan).map_err(|e| e.to_string())?;
        remove_link(resume)?;
    }
    remove_link(retry)?;
    let mut message = if enable {
        "WinRust is enabled.".to_owned()
    } else {
        "Windows tools are active. To switch back, open Enable WinRust from Start.".to_owned()
    };
    if choose_photo {
        message.push_str("\nChoose your preferred photo viewer in Windows Settings > Apps > Default apps.");
    }
    if !notes.is_empty() {
        message.push_str("\n\n");
        message.push_str(&notes.join("\n"));
    }
    Ok(message)
}

pub(crate) fn dialog(enable: bool) -> i32 {
    let prompt = if enable {
        t!(
            "Снова включить ранее отключённые компоненты WinRust?",
            "Enable the WinRust tools that were active before you switched to Windows?"
        )
    } else {
        t!(
            "Закрыть инструменты WinRust и вернуть рабочий стол, панель задач, Проводник, диспетчер задач и поиск Windows? Настройки WinRust сохранятся.",
            "Switch to the Windows desktop, taskbar, File Explorer, Task Manager and Search? WinRust file windows will close. Your WinRust preferences will be kept."
        )
    };
    let title = wide("WinRust");
    unsafe {
        if MessageBoxW(
            null_mut(),
            wide(prompt).as_ptr(),
            title.as_ptr(),
            MB_YESNO | MB_ICONQUESTION | MB_DEFBUTTON2 | MB_SETFOREGROUND,
        ) != IDYES
        {
            return 0;
        }
    }
    let result = switch(enable);
    let (code, text, icon) = match result {
        Ok(message) => (0, message, MB_ICONINFORMATION),
        Err(e) => (
            1,
            format!("The switch did not finish.\n\n{e}\n\nRun WinRust Setup again to retry this switch."),
            MB_ICONERROR,
        ),
    };
    log(&text);
    unsafe {
        MessageBoxW(null_mut(), wide(&text).as_ptr(), title.as_ptr(), icon | MB_SETFOREGROUND);
    }
    code
}
