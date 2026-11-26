//! Momotus for Windows (plans/windows-app.md): starts the core (mq-bridge.exe beside this exe), which serves
//! the web app on a loopback port it picks, and shows that app in one WebView2 window. The core ends with
//! this process: it holds the core's stdin, and the core exits when that closes.
#![cfg_attr(windows, windows_subsystem = "windows")]

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
    use std::io::{BufRead, BufReader};
    use std::os::windows::process::CommandExt;
    use std::path::{Path, PathBuf};
    use std::process::{Child, ChildStdin, Command, Stdio};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    use tao::dpi::LogicalSize;
    use tao::event::{Event, StartCause, WindowEvent};
    use tao::event_loop::{ControlFlow, EventLoop};
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
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

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
        if focus_running() {
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

    /// Starts mq-bridge.exe and waits for its ready line, which names the port it took.
    fn start_core(dir: &Path, data: &Path, log: &Path) -> Result<Core, String> {
        let err = std::fs::File::create(log)
            .map_err(|e| format!("its log could not be written ({e})"))?;
        let mut child = Command::new(dir.join("mq-bridge.exe"))
            .env("MQ_DATA", data)
            .env("MQ_STATIC", dir.join("web"))
            .env("MQ_BRIDGE_PORT", "0")
            .env("MQ_BRIDGE_EXIT_ON_EOF", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(err)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| format!("the core did not start ({e})"))?;
        let (stdin, stdout) = (child.stdin.take(), child.stdout.take());
        let (Some(stdin), Some(stdout)) = (stdin, stdout) else {
            let _ = child.kill();
            return Err("the core did not start".into());
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
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                Err("the core did not open the library in time".into())
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
        let event_loop = EventLoop::new();
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
        event_loop.run(move |event, _, flow| {
            *flow = closing.map_or(ControlFlow::Wait, ControlFlow::WaitUntil);
            match event {
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
                    *flow = ControlFlow::Exit;
                }
                _ => {}
            }
        });
    }
}
