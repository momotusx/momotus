//! Momotus for Windows (plans/windows-app.md): starts the core (mq-bridge.exe beside this exe), which serves
//! the web app on a loopback port it picks, and shows that app in one WebView2 window. The core ends with
//! this process: it holds the core's stdin, and the core exits when that closes.
#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg_attr(not(windows), allow(dead_code))]
mod feed;

#[cfg(windows)]
fn main() {
    win::main()
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Momotus's Windows app runs on Windows only (apps/windows).");
}

#[cfg(windows)]
mod win {
    use crate::feed;
    use std::io::{BufRead, BufReader};
    use std::os::windows::process::CommandExt;
    use std::path::{Path, PathBuf};
    use std::process::{Child, ChildStdin, Command, Stdio};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    use tao::dpi::LogicalSize;
    use tao::event::{Event, StartCause, WindowEvent};
    use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
    use tao::platform::windows::{IconExtWindows, WindowBuilderExtWindows};
    use tao::window::{Icon, WindowBuilder};
    use wry::{NewWindowResponse, WebContext, WebViewBuilder, WebViewBuilderExtWindows};

    /// The window class, which a second launch and the installer look for.
    const CLASS: &str = "Momotus";
    /// wry's own defaults, which `with_additional_browser_args` replaces, plus timers that keep their pace
    /// while the window is minimised (a stream keeps loading behind another window).
    const BROWSER_ARGS: &str = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection \
         --autoplay-policy=no-user-gesture-required --disable-background-timer-throttling";
    /// How long the core may take to open the library before the app says it could not start.
    const CORE_START: Duration = Duration::from_secs(60);
    /// How often the window checks its core is still running.
    const CORE_CHECK: Duration = Duration::from_secs(2);
    /// This build's version (apps/web/package.json, through build.rs).
    const VERSION: &str = env!("MOMOTUS_VERSION");
    /// The signed update feed and the key it must be signed with (plans/auto-update.md).
    const FEED: &str = "https://github.com/momotusx/momotus/releases/download/updates/latest.json";
    const FEED_KEY: &str = include_str!("../../shared/update-feed-key.pub.pem");
    /// Windows's own curl (Windows 10 1803 and later), named by path so no other curl on PATH is run.
    const CURL: &str = r"C:\Windows\System32\curl.exe";
    const DETACHED_PROCESS: u32 = 0x0000_0008;

    /// A newer version, downloaded and checked, that installs when the window closes.
    struct Ready {
        version: String,
        setup: PathBuf,
    }
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    /// The port the core asks for first. The page's storage belongs to its origin, port included, so the same
    /// port every launch keeps it; another program on it moves the core to a free one, remembered in `data\port`.
    const PORT: u16 = 47811;
    /// What the core would read from the user's environment and must not: a token the page cannot send,
    /// other origins, another profile or device id.
    const NOT_PASSED: [&str; 5] = [
        "MQ_BRIDGE_TOKEN",
        "MQ_BRIDGE_ORIGINS",
        "MQ_PROFILE",
        "MQ_DEVICE_ID",
        "MQ_BRIDGE_HEARTBEAT_MS",
    ];

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain([0]).collect()
    }

    /// The core while the window lives: its stdin held open (closing it ends the core) and its process.
    struct Core {
        _stdin: ChildStdin,
        child: Child,
        port: u16,
    }

    impl Drop for Core {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    pub fn main() {
        // One Momotus at a time, held from launch (the window only exists once the core is up).
        use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
        use windows_sys::Win32::System::Threading::CreateMutexW;
        // SAFETY: a NUL-terminated name and no security attributes; the handle lives as long as the process.
        let _one = unsafe { CreateMutexW(std::ptr::null(), 0, wide("Local\\Momotus").as_ptr()) };
        // SAFETY: read right after the call it reports on.
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            focus_running();
            return;
        }
        let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
        let data = env("MOMOTUS_DATA")
            .unwrap_or_else(|| env("LOCALAPPDATA").unwrap_or_default().join("Momotus"));
        let _ = std::fs::create_dir_all(data.join("logs"));
        let log = data.join("logs").join("core.log");
        let Some(dir) = std::env::current_exe()
            .ok()
            .and_then(|e| Some(e.parent()?.to_path_buf()))
        else {
            return alert(
                "Momotus could not find its own folder. Install it again from github.com/momotusx/momotus.",
            );
        };
        let core = match start_core(&dir, &data, &log) {
            Ok(core) => core,
            Err(why) => {
                return alert(&format!(
                    "Momotus could not start: {why}. The details are in {}.",
                    log.display()
                ));
            }
        };
        run(core, &data, &log);
    }

    /// The core on the remembered port (or PORT), else on a free one, which is remembered.
    fn start_core(dir: &Path, data: &Path, log: &Path) -> Result<Core, String> {
        let saved = data.join("port");
        let first = std::fs::read_to_string(&saved)
            .ok()
            .and_then(|p| p.trim().parse().ok())
            .unwrap_or(PORT);
        let core = match start_on(dir, data, log, first) {
            Err(Start::Ended) => start_on(dir, data, log, 0),
            other => other,
        }
        .map_err(|e| e.words().to_owned())?;
        let _ = std::fs::write(&saved, core.port.to_string());
        Ok(core)
    }

    enum Start {
        Spawn,
        /// The core ended before it was ready (its port taken, or a fault: the log says).
        Ended,
        Slow,
    }

    impl Start {
        fn words(&self) -> &'static str {
            match self {
                Start::Spawn => "its core could not be started",
                Start::Ended => "its core stopped while starting",
                Start::Slow => "its core did not open the library in time",
            }
        }
    }

    /// Starts mq-bridge.exe on `port` and waits for its ready line, which names the port it took.
    fn start_on(dir: &Path, data: &Path, log: &Path, port: u16) -> Result<Core, Start> {
        let err = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log)
            .map_err(|_| Start::Spawn)?;
        let mut command = Command::new(dir.join("mq-bridge.exe"));
        for name in NOT_PASSED {
            command.env_remove(name);
        }
        let mut child = command
            .env("MQ_DATA", data)
            .env("MQ_STATIC", dir.join("web"))
            .env("MQ_BRIDGE_PORT", port.to_string())
            .env("MQ_BRIDGE_EXIT_ON_EOF", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(err)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|_| Start::Spawn)?;
        let (stdin, stdout) = (child.stdin.take(), child.stdout.take());
        let (Some(stdin), Some(stdout)) = (stdin, stdout) else {
            let _ = child.kill();
            return Err(Start::Spawn);
        };
        // The ready line arrives on a reader thread, so a core that never answers cannot hold the app.
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if let Some(port) = ready_port(&line) {
                    let _ = tx.send(port);
                }
            }
        });
        match rx.recv_timeout(CORE_START) {
            Ok(port) => Ok(Core {
                _stdin: stdin,
                child,
                port,
            }),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                Err(match e {
                    mpsc::RecvTimeoutError::Timeout => Start::Slow,
                    mpsc::RecvTimeoutError::Disconnected => Start::Ended,
                })
            }
        }
    }

    /// The port in mq-bridge's `{"event":"bridge_ready","url":"ws://127.0.0.1:<port>/ws",...}`.
    fn ready_port(line: &str) -> Option<u16> {
        let v: serde_json::Value = serde_json::from_str(line).ok()?;
        if v["event"] != "bridge_ready" {
            return None;
        }
        let url = v["url"].as_str()?.strip_prefix("ws://127.0.0.1:")?;
        url.strip_suffix("/ws")?.parse().ok()
    }

    /// A Momotus window already open is restored and brought to the front, and this copy ends.
    fn focus_running() -> bool {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            FindWindowW, SW_RESTORE, SetForegroundWindow, ShowWindow,
        };
        let class = wide(CLASS);
        // SAFETY: a NUL-terminated class name and no title.
        let found = unsafe { FindWindowW(class.as_ptr(), std::ptr::null()) };
        if found.is_null() {
            return false;
        }
        // SAFETY: a window handle the system just returned.
        unsafe {
            ShowWindow(found, SW_RESTORE);
            SetForegroundWindow(found);
        }
        true
    }

    fn alert(text: &str) {
        use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
        // SAFETY: NUL-terminated wide strings and no owner window.
        unsafe {
            MessageBoxW(
                std::ptr::null_mut(),
                wide(text).as_ptr(),
                wide("Momotus").as_ptr(),
                MB_OK | MB_ICONERROR,
            )
        };
    }

    /// WebView2 is missing: Microsoft's download page or Quit.
    fn webview2_missing() {
        use windows_sys::Win32::UI::Controls::{
            TASKDIALOG_BUTTON, TASKDIALOGCONFIG, TD_ERROR_ICON, TDF_ALLOW_DIALOG_CANCELLATION,
            TaskDialogIndirect,
        };
        const GET: i32 = 100;
        let (get, quit) = (wide("Get WebView2"), wide("Quit"));
        let buttons = [
            TASKDIALOG_BUTTON {
                nButtonID: GET,
                pszButtonText: get.as_ptr(),
            },
            TASKDIALOG_BUTTON {
                nButtonID: 2,
                pszButtonText: quit.as_ptr(),
            },
        ];
        let (title, heading) = (
            wide("Momotus"),
            wide("Momotus needs Microsoft Edge WebView2"),
        );
        let body = wide(
            "This PC does not have the WebView2 Runtime, which Momotus uses to show its window. Get it from Microsoft (free), then open Momotus again.",
        );
        // SAFETY: all-zero is a valid empty config; every field set below points at a buffer alive for the call.
        let mut config: TASKDIALOGCONFIG = unsafe { std::mem::zeroed() };
        config.cbSize = size_of::<TASKDIALOGCONFIG>() as u32;
        config.dwFlags = TDF_ALLOW_DIALOG_CANCELLATION;
        config.pszWindowTitle = title.as_ptr();
        config.pszMainInstruction = heading.as_ptr();
        config.pszContent = body.as_ptr();
        config.Anonymous1.pszMainIcon = TD_ERROR_ICON;
        config.cButtons = buttons.len() as u32;
        config.pButtons = buttons.as_ptr();
        config.nDefaultButton = GET;
        let mut chosen = 0;
        // SAFETY: a filled config and a writable result; the radio and checkbox outputs are not asked for.
        unsafe {
            TaskDialogIndirect(
                &config,
                &mut chosen,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if chosen == GET {
            open_outside("https://go.microsoft.com/fwlink/p/?LinkId=2124703");
        }
    }

    /// An http(s) link in the default browser.
    fn open_outside(url: &str) {
        use windows_sys::Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL};
        if !(url.starts_with("https://") || url.starts_with("http://")) {
            return;
        }
        // SAFETY: NUL-terminated wide strings; no owner window, parameters or folder.
        unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                wide("open").as_ptr(),
                wide(url).as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            )
        };
    }

    fn run(core: Core, data: &Path, log: &Path) {
        let origin = format!("http://127.0.0.1:{}", core.port);
        let event_loop = EventLoopBuilder::<Ready>::with_user_event().build();
        watch_updates(data.join("update"), event_loop.create_proxy());
        let window = WindowBuilder::new()
            .with_title("Momotus")
            .with_window_classname(CLASS)
            .with_inner_size(LogicalSize::new(1280.0, 800.0))
            .with_min_inner_size(LogicalSize::new(800.0, 560.0))
            .with_window_icon(Icon::from_resource(1, None).ok())
            .build(&event_loop);
        let window = match window {
            Ok(w) => w,
            Err(_) => {
                return alert(&format!(
                    "Momotus could not open its window. The details are in {}.",
                    log.display()
                ));
            }
        };
        let mut context = WebContext::new(Some(data.join("WebView2")));
        let ours = {
            let origin = origin.clone();
            move |url: &str| url == origin || url.starts_with(&format!("{origin}/"))
        };
        // The page's core socket is on the port the core took; only our own origin is told it.
        let socket = format!(
            "if (location.origin === {origin:?}) window.__MQ_BRIDGE_URL__ = \"ws://127.0.0.1:{}/ws\";",
            core.port
        );
        let webview = WebViewBuilder::new_with_web_context(&mut context)
            .with_url(format!("{origin}/"))
            .with_autoplay(true)
            .with_additional_browser_args(BROWSER_ARGS)
            .with_initialization_script(&socket)
            .with_devtools(cfg!(debug_assertions))
            // Our own pages load; an off-origin link opens in the browser; anything else (a dropped file among
            // them) is cancelled.
            .with_navigation_handler({
                let ours = ours.clone();
                move |url| {
                    if ours(&url) || url == "about:blank" {
                        return true;
                    }
                    open_outside(&url);
                    false
                }
            })
            .with_new_window_req_handler(move |url, _| {
                if !ours(&url) {
                    open_outside(&url);
                }
                NewWindowResponse::Deny
            })
            .build(&window);
        let webview = match webview {
            Ok(w) => w,
            Err(_) => return webview2_missing(),
        };
        let mut core = Some(core);
        let mut closing: Option<Instant> = None;
        let mut staged: Option<PathBuf> = None;
        let log = log.to_path_buf();
        event_loop.run(move |event, _, flow| {
            // The core is looked at every 2 s: if it ended, the window says so and closes.
            *flow = ControlFlow::WaitUntil(closing.unwrap_or_else(|| Instant::now() + CORE_CHECK));
            match event {
                // The title says it, so the user knows a close brings the new version (nothing pops up).
                Event::UserEvent(ready) => {
                    window.set_title(&format!(
                        "Momotus (version {} installs when you close it)",
                        ready.version
                    ));
                    staged = Some(ready.setup);
                }
                Event::NewEvents(StartCause::ResumeTimeReached { .. }) if closing.is_none() => {
                    if core
                        .as_mut()
                        .is_some_and(|c| !matches!(c.child.try_wait(), Ok(None)))
                    {
                        window.set_visible(false);
                        alert(&format!(
                            "Momotus stopped: its core ended. The details are in {}.",
                            log.display()
                        ));
                        *flow = ControlFlow::Exit;
                    }
                }
                // The player saves where it stopped on `pagehide`, given up to 500 ms; then the core ends.
                Event::WindowEvent {
                    event: WindowEvent::CloseRequested,
                    ..
                } if closing.is_none() => {
                    let _ =
                        webview.evaluate_script("window.dispatchEvent(new Event(\"pagehide\"))");
                    window.set_visible(false);
                    let at = Instant::now() + Duration::from_millis(500);
                    closing = Some(at);
                    *flow = ControlFlow::WaitUntil(at);
                }
                Event::NewEvents(StartCause::ResumeTimeReached { .. }) if closing.is_some() => {
                    drop(core.take());
                    // The installer waits for this window to go, replaces the app and keeps the library.
                    if let Some(setup) = staged.take() {
                        let _ = Command::new(setup)
                            .arg("/S")
                            .creation_flags(DETACHED_PROCESS | CREATE_NO_WINDOW)
                            .spawn();
                    }
                    *flow = ControlFlow::Exit;
                }
                _ => {}
            }
        });
    }

    /// Looks for a newer version 10 s after launch, then every 6 h, until one is ready.
    fn watch_updates(dir: PathBuf, proxy: EventLoopProxy<Ready>) {
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(10));
            loop {
                if let Some(ready) = check_update(&dir) {
                    let _ = proxy.send_event(ready);
                    return;
                }
                std::thread::sleep(Duration::from_secs(6 * 3600));
            }
        });
    }

    /// The feed's newer installer, downloaded into `dir` and its SHA-256 checked, or None.
    fn check_update(dir: &Path) -> Option<Ready> {
        let envelope = curl(&["--max-filesize", "65536", FEED])?;
        let offer = feed::offer(&envelope, FEED_KEY, VERSION)?;
        std::fs::create_dir_all(dir).ok()?;
        let setup = dir.join(format!("momotus-windows-x64-{}-setup.exe", offer.version));
        let good = |path: &Path| {
            std::fs::read(path)
                .is_ok_and(|b| b.len() as u64 == offer.size && feed::sha256_hex(&b) == offer.sha256)
        };
        if !good(&setup) {
            let part = setup.with_extension("part");
            let cap = (offer.size + 1).to_string();
            let out = part.to_string_lossy().into_owned();
            curl(&["--max-filesize", &cap, "-o", &out, &offer.url])?;
            if !good(&part) {
                let _ = std::fs::remove_file(&part);
                return None;
            }
            std::fs::rename(&part, &setup).ok()?;
        }
        Some(Ready {
            version: offer.version,
            setup,
        })
    }

    /// `curl -fsSL <args>`, its output when it succeeds.
    fn curl(args: &[&str]) -> Option<Vec<u8>> {
        let out = Command::new(CURL)
            .args([
                "-fsSL",
                "--proto",
                "=https",
                "--proto-redir",
                "=https",
                "--max-time",
                "600",
            ])
            .args(args)
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .ok()?;
        out.status.success().then_some(out.stdout)
    }
}
