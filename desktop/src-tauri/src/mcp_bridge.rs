//! `LAAB mcp`: what an AI app runs to reach LAAB. It pipes stdin and stdout
//! to the socket the running app listens on (`mcp.rs`), opening LAAB in the
//! background first if it isn't running.

use std::io::{BufRead, BufReader, Write};
#[cfg(unix)]
use std::os::unix::net::UnixStream;
use std::path::Path;
#[cfg(target_os = "macos")]
use std::path::PathBuf;
use std::time::{Duration, Instant};

const WAIT: Duration = Duration::from_secs(30);
const UNREACHABLE: &str = "เชื่อมต่อ LAAB ไม่ได้: เปิด LAAB แล้วเปิด MCP ที่เมนู ⋯ > MCP (Can't reach LAAB: open it and turn on MCP in ⋯ > MCP.)";

/// Answer the first request with an error, so the AI app shows why.
fn fail(first: &str, message: &str) -> i32 {
    eprintln!("LAAB: {message}");
    let id = serde_json::from_str::<serde_json::Value>(first)
        .ok()
        .and_then(|v| v.get("id").cloned())
        .unwrap_or(serde_json::Value::Null);
    let reply = serde_json::json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32000, "message": message}});
    let mut out = std::io::stdout();
    let _ = writeln!(out, "{reply}");
    let _ = out.flush();
    1
}

#[cfg(unix)]
pub fn run(args: &[String]) -> i32 {
    let demo = args.iter().any(|a| a == "--demo") || std::env::var_os("LAAB_DEMO").is_some();
    let socket = match crate::paths::Paths::root_for(demo) {
        Ok(root) => crate::paths::mcp_socket(&root),
        Err(e) => {
            eprintln!("LAAB: {e}");
            return 1;
        }
    };
    let mut input = BufReader::new(std::io::stdin());
    // Connect on the first message, so an error can answer it.
    let mut first = String::new();
    match input.read_line(&mut first) {
        Ok(0) | Err(_) => return 0,
        Ok(_) => {}
    }
    let stream = match connect(&socket) {
        Ok(s) => s,
        Err(message) => return fail(&first, &message),
    };
    let mut to_app = match stream.try_clone() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("LAAB: {e}");
            return 1;
        }
    };
    std::thread::spawn(move || {
        if to_app.write_all(first.as_bytes()).is_ok() {
            let _ = std::io::copy(&mut input, &mut to_app);
        }
        let _ = to_app.shutdown(std::net::Shutdown::Write);
    });
    let mut from_app = stream;
    let mut out = std::io::stdout();
    let _ = std::io::copy(&mut from_app, &mut out);
    let _ = out.flush();
    0
}

#[cfg(unix)]
fn connect(socket: &Path) -> Result<UnixStream, String> {
    if let Ok(s) = UnixStream::connect(socket) {
        return Ok(s);
    }
    let launched = launch();
    if launched {
        let start = Instant::now();
        while start.elapsed() < WAIT {
            std::thread::sleep(Duration::from_millis(300));
            if let Ok(s) = UnixStream::connect(socket) {
                return Ok(s);
            }
        }
    }
    Err(UNREACHABLE.into())
}

/// Open LAAB in the background. True if it was started.
#[cfg(target_os = "macos")]
fn launch() -> bool {
    bundle().is_some_and(|app| {
        std::process::Command::new("/usr/bin/open")
            .arg("-g")
            .arg(&app)
            .status()
            .is_ok_and(|s| s.success())
    })
}

#[cfg(all(unix, not(target_os = "macos")))]
fn launch() -> bool {
    false
}

/// On Windows `LAAB mcp` is LAAB.exe itself: start it without arguments.
#[cfg(windows)]
fn launch() -> bool {
    std::env::current_exe().is_ok_and(|exe| std::process::Command::new(exe).spawn().is_ok())
}

/// `…/LAAB.app` when running from inside an app bundle.
#[cfg(target_os = "macos")]
fn bundle() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let app = contents.parent()?;
    (macos.ends_with("Contents/MacOS") && app.extension().is_some_and(|e| e == "app")).then(|| app.to_path_buf())
}

/// Windows: the same pipe through a named pipe. Overlapped I/O (tokio), so
/// reading and writing at once don't block each other as they would on a
/// synchronous pipe handle.
#[cfg(windows)]
pub fn run(args: &[String]) -> i32 {
    use tokio::io::{AsyncWriteExt, BufReader as AsyncBufReader};
    use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeClient};

    let demo = args.iter().any(|a| a == "--demo") || std::env::var_os("LAAB_DEMO").is_some();
    let pipe = match crate::paths::Paths::root_for(demo) {
        Ok(root) => crate::paths::mcp_socket(&root),
        Err(e) => {
            eprintln!("LAAB: {e}");
            return 1;
        }
    };
    let mut input = BufReader::new(std::io::stdin());
    let mut first = String::new();
    match input.read_line(&mut first) {
        Ok(0) | Err(_) => return 0,
        Ok(_) => {}
    }
    // Whatever the reader took past the first line goes up with it.
    let mut head = first.clone().into_bytes();
    head.extend_from_slice(input.buffer());
    let Ok(rt) = tokio::runtime::Builder::new_multi_thread().enable_all().build() else {
        return fail(&first, "tokio runtime");
    };
    rt.block_on(async move {
        let open = |p: &Path| -> std::io::Result<NamedPipeClient> { ClientOptions::new().open(p) };
        let mut client = open(&pipe);
        if client.is_err() && launch() {
            let start = Instant::now();
            while client.is_err() && start.elapsed() < WAIT {
                tokio::time::sleep(Duration::from_millis(300)).await;
                client = open(&pipe);
            }
        }
        let Ok(client) = client else {
            return fail(&first, UNREACHABLE);
        };
        let (mut from_app, mut to_app) = tokio::io::split(client);
        let up = tokio::spawn(async move {
            if to_app.write_all(&head).await.is_ok() {
                let _ = tokio::io::copy(&mut AsyncBufReader::new(tokio::io::stdin()), &mut to_app).await;
            }
            let _ = to_app.shutdown().await;
        });
        let mut out = tokio::io::stdout();
        let _ = tokio::io::copy(&mut from_app, &mut out).await;
        let _ = out.flush().await;
        up.abort();
        0
    })
}
