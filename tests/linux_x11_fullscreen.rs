#![cfg(target_os = "linux")]

//! Run with an EWMH window manager and xprop/xwininfo installed:
//! cargo test --test linux_x11_fullscreen -- --ignored --nocapture
//! GLX is the default, matching the game. Set MINIQUAD_X11_FULLSCREEN_GL=egl
//! to test EGL on systems with working EGL support. Each case uses a fresh process.
use miniquad::{conf, window, EventHandler};
use std::{
    process::{Command, Stdio},
    time::{Duration, Instant},
};

struct App {
    title: String,
    window_id: Option<String>,
    states: Vec<bool>,
    step: usize,
    requested_at: Instant,
    last_poll: Instant,
    matched_at: Option<Instant>,
}

impl EventHandler for App {
    fn update(&mut self) {
        assert!(
            self.requested_at.elapsed() < Duration::from_secs(5),
            "fullscreen step {} did not settle to {}",
            self.step,
            self.states[self.step]
        );
        if self.last_poll.elapsed() < Duration::from_millis(50) {
            return;
        }
        self.last_poll = Instant::now();
        if self.window_id.is_none() {
            let output = Command::new("xwininfo")
                .args(["-name", &self.title])
                .output()
                .expect("xwininfo is required");
            if !output.status.success() {
                return;
            }
            self.window_id = String::from_utf8(output.stdout)
                .unwrap()
                .split_whitespace()
                .find(|word| word.starts_with("0x"))
                .map(str::to_owned);
        }
        let Some(id) = self.window_id.as_deref() else {
            return;
        };
        let output = Command::new("xprop")
            .args(["-id", id, "_NET_WM_STATE"])
            .output()
            .expect("xprop is required");
        assert!(output.status.success(), "could not inspect the test window");
        let state = String::from_utf8(output.stdout).unwrap();
        if state.contains("_NET_WM_STATE_FULLSCREEN") != self.states[self.step] {
            self.matched_at = None;
            return;
        }
        // Require a stable result, including repeated requests for the same mode.
        let matched_at = self.matched_at.get_or_insert_with(Instant::now);
        if matched_at.elapsed() < Duration::from_millis(250) {
            return;
        }
        println!(
            "{}: step {} fullscreen={}",
            self.title, self.step, self.states[self.step]
        );
        self.step += 1;
        if self.step == self.states.len() {
            window::order_quit();
            return;
        }
        window::set_fullscreen(self.states[self.step]);
        self.requested_at = Instant::now();
        self.matched_at = None;
    }

    fn draw(&mut self) {}
}

#[test]
#[ignore = "requires a running X11/EWMH desktop, OpenGL, xprop and xwininfo"]
fn fullscreen_requests_reach_window_manager() {
    let case = match std::env::var("MINIQUAD_X11_FULLSCREEN_CASE") {
        Ok(case) => case,
        Err(_) => {
            let backend =
                std::env::var("MINIQUAD_X11_FULLSCREEN_GL").unwrap_or_else(|_| "glx".to_owned());
            assert!(matches!(backend.as_str(), "glx" | "egl"));
            for mode in ["windowed", "fullscreen"] {
                let case = format!("{backend}-{mode}");
                let mut child = Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "fullscreen_requests_reach_window_manager",
                        "--ignored",
                        "--nocapture",
                    ])
                    .env("MINIQUAD_X11_FULLSCREEN_CASE", &case)
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap();
                let started = Instant::now();
                while child.try_wait().unwrap().is_none() {
                    if started.elapsed() > Duration::from_secs(30) {
                        let _ = child.kill();
                        let _ = child.wait();
                        panic!("{}: timed out", case);
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                let output = child.wait_with_output().unwrap();
                println!("{}", String::from_utf8_lossy(&output.stdout));
                assert!(
                    output.status.success(),
                    "{case}: status={} {}",
                    output.status,
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            return;
        }
    };
    let fullscreen = case.ends_with("-fullscreen");
    let title = format!("miniquad-fullscreen-{}-{case}", std::process::id());
    miniquad::start(
        conf::Conf {
            window_title: title.clone(),
            fullscreen,
            window_width: 800,
            window_height: 600,
            platform: conf::Platform {
                linux_backend: conf::LinuxBackend::X11Only,
                linux_x11_gl: if case.starts_with("egl-") {
                    conf::LinuxX11Gl::EGLOnly
                } else {
                    conf::LinuxX11Gl::GLXOnly
                },
                ..Default::default()
            },
            ..Default::default()
        },
        move || {
            Box::new(App {
                title,
                window_id: None,
                states: vec![
                    fullscreen,
                    !fullscreen,
                    !fullscreen,
                    fullscreen,
                    fullscreen,
                    !fullscreen,
                ],
                step: 0,
                requested_at: Instant::now(),
                last_poll: Instant::now(),
                matched_at: None,
            })
        },
    );
}
