//! Lootwise's app id on Linux, `io.github.hawasman.Lootwise`, which the desktop portal needs
//! before it lets Lootwise register Ctrl+D.
//!
//! The GlobalShortcuts portal refuses an app it can't name ("An app id is required"). For an
//! unsandboxed app it takes the id from the process's systemd scope
//! (`app-io.github.hawasman.Lootwise-<n>.scope`) and wants a desktop file of that id. A launch
//! from a terminal or `cargo run` has neither, so at start, before Rune registers the shortcut,
//! Lootwise installs its desktop file into `$XDG_DATA_HOME/applications` and moves itself into
//! such a scope through systemd's user bus.

use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rune::ShortcutError;

pub const APP_ID: &str = "io.github.hawasman.Lootwise";
const DESKTOP_FILE: &str = "io.github.hawasman.Lootwise.desktop";
/// The desktop file as shipped; its `Exec` is replaced by the running binary when installed.
const DESKTOP_TEMPLATE: &str =
    include_str!("../../../packaging/linux/io.github.hawasman.Lootwise.desktop");
/// How long systemd gets to put Lootwise in its new scope.
const SCOPE_WAIT: Duration = Duration::from_secs(2);
const SCOPE_POLL: Duration = Duration::from_millis(20);
const DBUS_TIMEOUT: Duration = Duration::from_secs(5);

/// Gives Lootwise its app id: reports it to Rune (so its overlays carry it as their Wayland
/// `app_id`), installs the desktop file and enters the scope, logging what happened. Failures
/// are logged and startup continues; registering Ctrl+D then fails and
/// [`explain_shortcut_errors`] says why.
pub fn establish() {
    rune::set_app_id(APP_ID);
    let installed = desktop_dir()
        .ok_or_else(|| "neither XDG_DATA_HOME nor HOME is set".to_owned())
        .and_then(|dir| {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            Ok((ensure_desktop_file(&dir, &exe)?, dir.join(DESKTOP_FILE)))
        });
    match installed {
        Ok((DesktopFile::Current, _)) => {}
        Ok((DesktopFile::Installed, path)) => eprintln!("lootwise: installed {}", path.display()),
        Ok((DesktopFile::Updated, path)) => eprintln!("lootwise: updated {}", path.display()),
        Err(e) => eprintln!("lootwise: can't install the desktop file: {e}"),
    }
    match ensure_scope(
        std::process::id(),
        || std::fs::read_to_string("/proc/self/cgroup"),
        start_transient_scope,
        || std::thread::sleep(SCOPE_POLL),
    ) {
        Ok(Scope::Already) => {}
        Ok(Scope::Entered(name)) => eprintln!("lootwise: running in systemd scope {name}"),
        Err(e) => {
            eprintln!("lootwise: not in an app scope, so the portal can't name Lootwise: {e}")
        }
    }
}

/// Says what to do about Rune's failures to register Ctrl+D, which it has already printed.
pub fn explain_shortcut_errors(errors: &[ShortcutError]) {
    if errors.contains(&ShortcutError::PermissionDenied) {
        eprintln!(
            "lootwise: Ctrl+D isn't registered. If the desktop showed no shortcut dialog, its \
             portal couldn't tell which app was asking: it needs Lootwise in a systemd scope \
             named app-{APP_ID}-<n>.scope and {DESKTOP_FILE} in $XDG_DATA_HOME/applications \
             (~/.local/share/applications by default), which Lootwise sets up at start (see the \
             lines above). If you declined the dialog, allow Lootwise in the desktop's shortcut \
             settings (KDE: System Settings > Shortcuts), or restart Lootwise to be asked again."
        );
    }
}

/// `$XDG_DATA_HOME/applications`, falling back to `~/.local/share/applications`.
fn desktop_dir() -> Option<PathBuf> {
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
    Some(data.join("applications"))
}

/// What [`ensure_desktop_file`] did.
#[derive(Debug, PartialEq, Eq)]
enum DesktopFile {
    Installed,
    /// One was there, launching something else or out of date.
    Updated,
    Current,
}

/// Writes the desktop file into `dir` unless an identical one is there.
fn ensure_desktop_file(dir: &Path, exe: &Path) -> Result<DesktopFile, String> {
    let exe = exe
        .to_str()
        .ok_or_else(|| format!("the binary's path isn't UTF-8: {}", exe.display()))?;
    let wanted = desktop_entry(exe);
    let path = dir.join(DESKTOP_FILE);
    let outcome = match std::fs::read_to_string(&path) {
        Ok(existing) if existing == wanted => return Ok(DesktopFile::Current),
        Ok(_) => DesktopFile::Updated,
        Err(e) if e.kind() == io::ErrorKind::NotFound => DesktopFile::Installed,
        // Unreadable (not UTF-8, say): replace it.
        Err(_) => DesktopFile::Updated,
    };
    let write = || -> io::Result<()> {
        std::fs::create_dir_all(dir)?;
        // Through a temporary file, so the desktop never reads half a file.
        let partial = dir.join(format!(".{DESKTOP_FILE}.partial"));
        std::fs::write(&partial, &wanted)?;
        std::fs::rename(&partial, &path)
    };
    write().map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(outcome)
}

/// The shipped desktop file, launching `exe`.
fn desktop_entry(exe: &str) -> String {
    DESKTOP_TEMPLATE
        .lines()
        .map(|line| {
            if line.starts_with("Exec=") {
                format!("Exec={}\n", exec_program(exe))
            } else {
                format!("{line}\n")
            }
        })
        .collect()
}

/// `path` as the program of a desktop file's `Exec` key: quoted when it holds a reserved
/// character, then escaped as a desktop file string value.
fn exec_program(path: &str) -> String {
    const RESERVED: &[char] = &[
        ' ', '\t', '\n', '"', '\'', '\\', '>', '<', '~', '|', '&', ';', '$', '*', '?', '#', '(',
        ')', '`', '%',
    ];
    let arg = if path.contains(RESERVED) {
        let mut quoted = String::from("\"");
        for c in path.chars() {
            match c {
                '"' | '`' | '$' | '\\' => {
                    quoted.push('\\');
                    quoted.push(c);
                }
                // A field code even inside quotes.
                '%' => quoted.push_str("%%"),
                _ => quoted.push(c),
            }
        }
        quoted.push('"');
        quoted
    } else {
        path.to_owned()
    };
    // The key's value is itself a string, which escapes backslashes and control characters.
    arg.replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\t', "\\t")
}

/// Where Lootwise runs, from [`ensure_scope`].
#[derive(Debug, PartialEq, Eq)]
enum Scope {
    /// Already in a scope or service of Lootwise's app id, e.g. launched from the desktop.
    Already,
    /// Moved into this new scope.
    Entered(String),
}

/// Whether a `/proc/<pid>/cgroup` puts the process in a unit of Lootwise's app id:
/// `app-io.github.hawasman.Lootwise-<n>.scope`, `app-io.github.hawasman.Lootwise@<n>.service`
/// and the like.
fn in_app_scope(cgroup: &str) -> bool {
    let prefix = format!("app-{APP_ID}");
    cgroup.lines().any(|line| {
        line.rsplit('/')
            .next()
            .and_then(|unit| unit.strip_prefix(&prefix))
            .is_some_and(|rest| rest.starts_with(['-', '@', '.']))
    })
}

fn scope_name(pid: u32) -> String {
    format!("app-{APP_ID}-{pid}.scope")
}

/// Moves process `pid` into a new scope of Lootwise's app id unless it is in one already.
/// `cgroup` reads the process's cgroup file, `start` asks systemd for the scope, and `pause`
/// waits between checks for systemd to have moved it.
fn ensure_scope(
    pid: u32,
    cgroup: impl Fn() -> io::Result<String>,
    start: impl FnOnce(&str, u32) -> Result<(), String>,
    pause: impl Fn(),
) -> Result<Scope, String> {
    let read = || cgroup().map_err(|e| format!("can't read the process's cgroup: {e}"));
    if in_app_scope(&read()?) {
        return Ok(Scope::Already);
    }
    let name = scope_name(pid);
    start(&name, pid)?;
    let tries = (SCOPE_WAIT.as_millis() / SCOPE_POLL.as_millis()).max(1);
    for _ in 0..tries {
        if in_app_scope(&read()?) {
            return Ok(Scope::Entered(name));
        }
        pause();
    }
    Err(format!(
        "systemd started {name} but didn't move Lootwise into it"
    ))
}

/// `StartTransientUnit` on the user's systemd: a scope holding `pid`.
fn start_transient_scope(name: &str, pid: u32) -> Result<(), String> {
    use zbus::{Connection, OwnedObjectPath, Proxy, Variant};
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("no runtime for D-Bus: {e}"))?;
    runtime.block_on(async {
        let call = async {
            let conn = Connection::session()
                .await
                .map_err(|e| format!("no session bus: {e}"))?;
            let systemd = Proxy::new(
                &conn,
                "org.freedesktop.systemd1",
                "/org/freedesktop/systemd1",
                "org.freedesktop.systemd1.Manager",
            )
            .await
            .map_err(|e| format!("no user systemd: {e}"))?;
            let properties = vec![
                ("PIDs", Variant::from(vec![pid])),
                ("Description", Variant::from("Lootwise")),
            ];
            let aux: Vec<(&str, Vec<(&str, Variant)>)> = Vec::new();
            let _job: OwnedObjectPath = systemd
                .call("StartTransientUnit", &(name, "fail", properties, aux))
                .await
                .map_err(|e| format!("systemd refused the scope: {e}"))?;
            Ok(())
        };
        tokio::time::timeout(DBUS_TIMEOUT, call)
            .await
            .unwrap_or_else(|_| Err("user systemd didn't answer".into()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    const TERMINAL: &str = "0::/user.slice/user-1000.slice/user@1000.service/app.slice/app-org.kde.konsole-4242.scope\n";

    fn scoped(unit: &str) -> String {
        format!("0::/user.slice/user-1000.slice/user@1000.service/app.slice/{unit}\n")
    }

    #[test]
    fn a_unit_of_lootwises_app_id_counts_as_its_scope() {
        assert!(in_app_scope(&scoped(
            "app-io.github.hawasman.Lootwise-77.scope"
        )));
        assert!(in_app_scope(&scoped(
            "app-io.github.hawasman.Lootwise@3f2a.service"
        )));
        assert!(!in_app_scope(TERMINAL), "a terminal's scope");
        assert!(
            !in_app_scope(&scoped("app-io.github.hawasman.LootwiseBeta-1.scope")),
            "another app id with the same start"
        );
        assert!(!in_app_scope(""));
    }

    #[test]
    fn already_in_its_scope_lootwise_asks_systemd_nothing() {
        let started = Cell::new(false);
        let outcome = ensure_scope(
            7,
            || Ok(scoped("app-io.github.hawasman.Lootwise-7.scope")),
            |_, _| {
                started.set(true);
                Ok(())
            },
            || {},
        );
        assert_eq!(outcome, Ok(Scope::Already));
        assert!(!started.get());
    }

    #[test]
    fn outside_its_scope_lootwise_moves_itself_into_a_new_one() {
        let cgroup = RefCell::new(TERMINAL.to_owned());
        let asked = RefCell::new(None);
        let outcome = ensure_scope(
            4321,
            || Ok(cgroup.borrow().clone()),
            |name, pid| {
                *asked.borrow_mut() = Some((name.to_owned(), pid));
                *cgroup.borrow_mut() = scoped(name);
                Ok(())
            },
            || {},
        );
        let name = "app-io.github.hawasman.Lootwise-4321.scope".to_owned();
        assert_eq!(asked.into_inner(), Some((name.clone(), 4321)));
        assert_eq!(outcome, Ok(Scope::Entered(name)));
    }

    #[test]
    fn without_systemd_lootwise_says_why_and_carries_on() {
        let outcome = ensure_scope(
            1,
            || Ok(TERMINAL.to_owned()),
            |_, _| Err("no session bus".into()),
            || {},
        );
        assert_eq!(outcome, Err("no session bus".into()));
    }

    #[test]
    fn a_scope_systemd_never_moves_lootwise_into_is_an_error() {
        let pauses = Cell::new(0);
        let outcome = ensure_scope(
            1,
            || Ok(TERMINAL.to_owned()),
            |_, _| Ok(()),
            || pauses.set(pauses.get() + 1),
        );
        assert!(outcome.is_err());
        assert!(pauses.get() > 0, "it waited for systemd first");
    }

    fn installed(dir: &Path) -> String {
        std::fs::read_to_string(dir.join(DESKTOP_FILE)).unwrap()
    }

    #[test]
    fn a_missing_desktop_file_is_installed_launching_the_running_binary() {
        let dir = tempfile::tempdir().unwrap();
        let apps = dir.path().join("applications");
        let outcome = ensure_desktop_file(&apps, Path::new("/opt/lootwise/lootwise"));
        assert_eq!(outcome, Ok(DesktopFile::Installed));
        let file = installed(&apps);
        assert!(file.starts_with("[Desktop Entry]\n"));
        assert!(file.contains("\nName=Lootwise\n"));
        assert!(file.contains("\nExec=/opt/lootwise/lootwise\n"), "{file}");
    }

    #[test]
    fn a_stale_desktop_file_is_replaced() {
        let dir = tempfile::tempdir().unwrap();
        ensure_desktop_file(dir.path(), Path::new("/old/build/lootwise")).unwrap();
        let outcome = ensure_desktop_file(dir.path(), Path::new("/new/build/lootwise"));
        assert_eq!(outcome, Ok(DesktopFile::Updated));
        assert!(installed(dir.path()).contains("\nExec=/new/build/lootwise\n"));
    }

    #[test]
    fn a_current_desktop_file_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let exe = Path::new("/opt/lootwise/lootwise");
        ensure_desktop_file(dir.path(), exe).unwrap();
        assert_eq!(
            ensure_desktop_file(dir.path(), exe),
            Ok(DesktopFile::Current)
        );
    }

    #[test]
    fn a_binary_path_with_spaces_is_quoted_in_exec() {
        assert_eq!(
            exec_program("/opt/lootwise/lootwise"),
            "/opt/lootwise/lootwise"
        );
        assert_eq!(
            exec_program("/home/p/My Games/lootwise"),
            "\"/home/p/My Games/lootwise\""
        );
        assert_eq!(
            exec_program("/home/p/50% $off/lootwise"),
            "\"/home/p/50%% \\\\$off/lootwise\"",
            "a field code and a shell character escaped, then the backslash for the string"
        );
    }
}
