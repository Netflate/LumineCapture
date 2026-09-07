// Entire daemon test suite: in-memory server running in a background thread using temporary directories, fake engines, and mock daemons,
// along with integration tests using real processes (where the test binary re-executes itself as a background daemon).

// DISCLAIMER : VAST majority of these tests were ai generated
use std::fs;
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;

use super::client::{self, Spawner, Timing};
use super::protocol::{self, EngineState, Outgoing, PROTO, Reply, Request, Welcome};
use super::server::{self, BuildError, Engine, Factory, Outcome};
use super::{
    Paths, Strike, daemon_blocked, gpu_ruled_out, lock_free, model_name, read_strikes, record_strike,
    spawn_detached, unix_now,
};
use crate::ocr::models::ModelFiles;
use crate::ocr::testing::{self, Fake, label_of, wait_until};
use crate::ocr::{CANCELLED, OcrBackend};

const BUILD: &str = "test-build";
const LONG: Duration = Duration::from_secs(10);

type Server = JoinHandle<Result<Outcome, String>>;
type Servers = Arc<Mutex<Vec<Server>>>;

fn never() -> bool {
    false
}

fn factory(fake: Fake) -> Factory {
    Arc::new(move |_: &ModelFiles| {
        Ok(Engine {
            backend: Box::new(fake.clone()),
            device: "fake".into(),
        })
    })
}

fn delayed(fake: Fake, delay: Duration) -> Factory {
    Arc::new(move |_: &ModelFiles| {
        thread::sleep(delay);
        Ok(Engine {
            backend: Box::new(fake.clone()),
            device: "fake".into(),
        })
    })
}

fn config(dir: &Path, factory: Factory) -> server::Config {
    server::Config {
        io_timeout: Duration::from_millis(300),
        unusable_grace: Duration::from_millis(200),
        lock_wait: Duration::from_millis(100),
        ..server::Config::new(
            Paths::in_dir(dir.to_path_buf()),
            BUILD.into(),
            Some(Duration::from_secs(60)),
            factory,
        )
    }
}

fn start(config: server::Config) -> Server {
    let socket = config.paths.socket.clone();
    let server = thread::spawn(move || server::serve(config));
    assert!(
        wait_until(LONG, || UnixStream::connect(&socket).is_ok()),
        "the server did not come up"
    );
    server
}

fn finish(server: Server) -> Outcome {
    assert!(wait_until(LONG, || server.is_finished()), "the server did not leave");
    server.join().unwrap().unwrap()
}

fn finish_all(servers: &Servers) -> Vec<Outcome> {
    let drained: Vec<Server> = servers.lock().unwrap().drain(..).collect();
    drained.into_iter().map(finish).collect()
}

struct Conn(UnixStream);

impl Conn {
    fn open(paths: &Paths) -> Self {
        let stream = UnixStream::connect(&paths.socket).unwrap();
        stream.set_read_timeout(Some(LONG)).unwrap();
        Self(stream)
    }

    fn greeted(paths: &Paths) -> (Self, Welcome) {
        let conn = Self::open(paths);
        match conn.ask(Outgoing::Hello(BUILD)) {
            Reply::Welcome(welcome) => (conn, welcome),
            other => panic!("expected welcome, got {other:?}"),
        }
    }

    fn send(&self, request: Outgoing) {
        protocol::write_request(&mut &self.0, request).unwrap();
    }

    fn reply(&self) -> Reply {
        protocol::read_reply(&mut &self.0).unwrap()
    }

    fn ask(&self, request: Outgoing) -> Reply {
        self.send(request);
        self.reply()
    }
}

/// Does not panic: the daemon might be in the middle of shutting down.
fn state(paths: &Paths) -> Option<EngineState> {
    let stream = UnixStream::connect(&paths.socket).ok()?;
    stream.set_read_timeout(Some(LONG)).ok()?;
    protocol::write_request(&mut &stream, Outgoing::Hello(BUILD)).ok()?;
    match protocol::read_reply(&mut &stream).ok()? {
        Reply::Welcome(welcome) => Some(welcome.state),
        _ => None,
    }
}

fn load(paths: &Paths, files: &ModelFiles) {
    assert_eq!(Conn::greeted(paths).0.ask(Outgoing::Load(files)), Reply::Ok);
}

fn wait_ready(paths: &Paths) {
    assert!(
        wait_until(LONG, || matches!(state(paths), Some(EngineState::Ready { .. }))),
        "the engine never became ready: {:?}",
        state(paths)
    );
}

fn shutdown(paths: &Paths) {
    if let Ok(stream) = UnixStream::connect(&paths.socket) {
        let _ = stream.set_read_timeout(Some(LONG));
        let _ = protocol::write_request(&mut &stream, Outgoing::Shutdown);
        let _ = protocol::read_reply(&mut &stream);
    }
}

fn dir_and_paths(tag: &str) -> (testing::TempDir, Paths) {
    let dir = testing::temp_dir(tag);
    let paths = Paths::in_dir(dir.to_path_buf());
    (dir, paths)
}

fn img() -> crate::ocr::OcrImage {
    testing::image(8, 6)
}

#[derive(Default, Clone)]
struct Probe {
    spawns: Arc<AtomicUsize>,
    locals: Arc<AtomicUsize>,
    kills: Arc<Mutex<Vec<i32>>>,
}

impl Probe {
    fn spawns(&self) -> usize {
        self.spawns.load(Ordering::SeqCst)
    }

    fn locals(&self) -> usize {
        self.locals.load(Ordering::SeqCst)
    }

    fn kills(&self) -> Vec<i32> {
        self.kills.lock().unwrap().clone()
    }
}

fn timing() -> Timing {
    Timing {
        connect: Duration::from_secs(3),
        retry: Duration::from_millis(5),
        reply_base: Duration::from_secs(5),
        reply_per_megapixel: Duration::ZERO,
        slice: Duration::from_millis(10),
        io: Duration::from_secs(3),
    }
}

fn client_config(paths: &Paths, spawn: Spawner, probe: &Probe) -> client::Config {
    let (locals, kills) = (probe.locals.clone(), probe.kills.clone());
    client::Config {
        paths: paths.clone(),
        build: BUILD.into(),
        spawn,
        kill: Arc::new(move |pid| kills.lock().unwrap().push(pid)),
        local: Arc::new(move |_: &ModelFiles| {
            locals.fetch_add(1, Ordering::SeqCst);
            Ok(Box::new(Fake::new("local")) as Box<dyn OcrBackend>)
        }),
        timing: timing(),
    }
}

fn thread_spawner(dir: &Path, factory: Factory, probe: &Probe, servers: &Servers) -> Spawner {
    let (dir, spawns, servers) = (dir.to_path_buf(), probe.spawns.clone(), servers.clone());
    Arc::new(move || {
        spawns.fetch_add(1, Ordering::SeqCst);
        let config = config(&dir, factory.clone());
        servers
            .lock()
            .unwrap()
            .push(thread::spawn(move || server::serve(config)));
        Ok(())
    })
}

fn refusing_spawner(probe: &Probe) -> Spawner {
    let spawns = probe.spawns.clone();
    Arc::new(move || {
        spawns.fetch_add(1, Ordering::SeqCst);
        Err(io::Error::other("spawning is not expected here"))
    })
}

fn read(backend: &dyn OcrBackend) -> String {
    label_of(&backend.recognize(img(), &never).unwrap())
}

/// Mock "daemon": accepts `n` incoming connections and executes prescribed scenario steps for each.
fn fake_daemon(paths: &Paths, connections: usize, script: impl Fn(UnixStream) + Send + 'static) -> JoinHandle<()> {
    fs::create_dir_all(&paths.dir).unwrap();
    let listener = UnixListener::bind(&paths.socket).unwrap();
    thread::spawn(move || {
        for stream in listener.incoming().take(connections) {
            script(stream.unwrap());
        }
    })
}

fn answer_hello(mut stream: &UnixStream, state: EngineState) {
    assert!(matches!(protocol::read_request(&mut stream), Ok(Request::Hello { .. })));
    let welcome = Welcome {
        proto: PROTO,
        build: BUILD.into(),
        state,
    };
    protocol::write_reply(&mut stream, &Reply::Welcome(welcome)).unwrap();
}

fn ready(files: &ModelFiles) -> EngineState {
    EngineState::Ready {
        device: "fake".into(),
        model: model_name(files),
    }
}

// ---------------------------------------------------------------- server

#[test]
fn second_daemon_leaves_the_first_one_serving() {
    let (dir, paths) = dir_and_paths("second");
    let first = start(config(&dir, factory(Fake::new("d"))));
    let second = thread::spawn({
        let config = config(&dir, factory(Fake::new("d")));
        move || server::serve(config)
    });
    assert_eq!(finish(second), Outcome::AlreadyRunning);
    assert_eq!(Conn::greeted(&paths).1.build, BUILD);

    shutdown(&paths);
    assert_eq!(finish(first), Outcome::Shutdown);
    assert!(!paths.socket.exists());
    assert!(lock_free(&paths.lock));
}

#[test]
fn socket_left_by_a_dead_daemon_is_replaced() {
    let (dir, paths) = dir_and_paths("stale");
    drop(UnixListener::bind(&paths.socket).unwrap());
    assert!(paths.socket.exists());
    assert!(UnixStream::connect(&paths.socket).is_err());

    let server = start(config(&dir, factory(Fake::new("d"))));
    assert_eq!(state(&paths), Some(EngineState::Idle));
    shutdown(&paths);
    assert_eq!(finish(server), Outcome::Shutdown);
}

#[test]
fn silent_client_is_dropped_without_blocking_others() {
    let (dir, paths) = dir_and_paths("silent");
    let server = start(config(&dir, factory(Fake::new("d"))));
    let silent = UnixStream::connect(&paths.socket).unwrap();
    silent.set_read_timeout(Some(LONG)).unwrap();

    let started = Instant::now();
    Conn::greeted(&paths);
    assert!(started.elapsed() < Duration::from_millis(250));
    let mut byte = [0u8; 1];
    assert_eq!((&silent).read(&mut byte).unwrap(), 0, "the silent connection should be closed");
    assert!(started.elapsed() < Duration::from_secs(3));

    shutdown(&paths);
    assert_eq!(finish(server), Outcome::Shutdown);
}

#[test]
fn garbage_and_requests_before_hello_close_only_that_connection() {
    let (dir, paths) = dir_and_paths("garbage");
    let server = start(config(&dir, factory(Fake::new("d"))));

    let junk = UnixStream::connect(&paths.socket).unwrap();
    junk.set_read_timeout(Some(LONG)).unwrap();
    (&junk).write_all(b"GET / HTTP/1.1\r\nHost: lumine\r\n\r\n").unwrap();
    let mut buf = [0u8; 16];
    assert!(matches!((&junk).read(&mut buf), Ok(0) | Err(_)));

    let rude = Conn::open(&paths);
    rude.send(Outgoing::Load(&testing::files("m")));
    assert!(protocol::read_reply(&mut &rude.0).is_err());
    assert_eq!(state(&paths), Some(EngineState::Idle), "a load without hello must be ignored");

    shutdown(&paths);
    assert_eq!(finish(server), Outcome::Shutdown);
}

#[test]
fn client_that_leaves_cancels_its_scan() {
    let (dir, paths) = dir_and_paths("leave");
    let fake = Fake::new("d").slow(60, Duration::from_millis(5));
    let server = start(config(&dir, factory(fake.clone())));
    load(&paths, &testing::files("m"));
    wait_ready(&paths);

    let (scan, _) = Conn::greeted(&paths);
    scan.send(Outgoing::Recognize(&img()));
    thread::sleep(Duration::from_millis(60));
    drop(scan);
    assert!(wait_until(LONG, || fake.cancels() == 1), "the daemon kept scanning for nobody");

    let (next, _) = Conn::greeted(&paths);
    match next.ask(Outgoing::Recognize(&img())) {
        Reply::Lines(text) => assert_eq!(label_of(&text), "d"),
        other => panic!("expected lines, got {other:?}"),
    }
    shutdown(&paths);
    assert_eq!(finish(server), Outcome::Shutdown);
}

#[test]
fn hello_and_status_answer_while_loading_and_scanning() {
    let (dir, paths) = dir_and_paths("status");
    let fake = Fake::new("d").slow(80, Duration::from_millis(5));
    let server = start(config(&dir, delayed(fake, Duration::from_millis(400))));
    let (conn, welcome) = Conn::greeted(&paths);
    assert_eq!(welcome.state, EngineState::Idle);
    assert_eq!(conn.ask(Outgoing::Load(&testing::files("m"))), Reply::Ok);

    let quick_status = |what: &str| {
        let started = Instant::now();
        let reply = Conn::greeted(&paths).0.ask(Outgoing::Status);
        assert!(started.elapsed() < Duration::from_millis(200), "{what} took {:?}", started.elapsed());
        match reply {
            Reply::Status(status) => status,
            other => panic!("expected status, got {other:?}"),
        }
    };
    let loading = quick_status("status while loading");
    assert!(matches!(loading.state, EngineState::Loading { .. }));
    assert_eq!(loading.pid, std::process::id());

    wait_ready(&paths);
    let (scan, _) = Conn::greeted(&paths);
    scan.send(Outgoing::Recognize(&img()));
    thread::sleep(Duration::from_millis(50));
    let scanning = quick_status("status while scanning");
    assert_eq!(scanning.served, 0);
    assert_eq!(scanning.idle_left_secs, Some(60), "a scan keeps the idle timer full");
    assert!(matches!(scan.reply(), Reply::Lines(_)));
    assert_eq!(quick_status("status after").served, 1);

    shutdown(&paths);
    assert_eq!(finish(server), Outcome::Shutdown);
}

#[test]
fn idle_daemon_leaves_and_cleans_up() {
    let (dir, paths) = dir_and_paths("idle");
    let server = start(server::Config {
        idle: Some(Duration::from_millis(200)),
        ..config(&dir, factory(Fake::new("d")))
    });
    load(&paths, &testing::files("m"));
    let started = Instant::now();
    assert_eq!(finish(server), Outcome::Idle);
    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(!paths.socket.exists());
    assert!(lock_free(&paths.lock));
}

#[test]
fn idle_timer_does_not_fire_during_a_scan() {
    let (dir, paths) = dir_and_paths("idle-scan");
    let fake = Fake::new("d").slow(160, Duration::from_millis(5));
    let server = start(server::Config {
        idle: Some(Duration::from_millis(300)),
        ..config(&dir, factory(fake))
    });
    load(&paths, &testing::files("m"));
    wait_ready(&paths);

    let (scan, _) = Conn::greeted(&paths);
    scan.send(Outgoing::Recognize(&img()));
    thread::sleep(Duration::from_millis(600));
    assert!(!server.is_finished(), "left in the middle of a scan");
    assert!(matches!(scan.reply(), Reply::Lines(_)));
    assert_eq!(finish(server), Outcome::Idle);
}

#[test]
fn idle_exit_waits_for_an_open_connection() {
    let (dir, paths) = dir_and_paths("idle-conn");
    let server = start(server::Config {
        idle: Some(Duration::from_millis(200)),
        io_timeout: Duration::from_millis(900),
        ..config(&dir, factory(Fake::new("d")))
    });
    let held = Conn::open(&paths);
    assert!(matches!(held.ask(Outgoing::Hello(BUILD)), Reply::Welcome(_)));
    thread::sleep(Duration::from_millis(500));
    assert!(!server.is_finished(), "left while a client was mid-conversation");

    drop(held);
    assert_eq!(finish(server), Outcome::Idle);
}

#[test]
fn shutdown_does_not_wait_for_a_long_scan() {
    let (dir, paths) = dir_and_paths("shutdown");
    let fake = Fake::new("d").slow(400, Duration::from_millis(5)).deaf();
    let server = start(config(&dir, factory(fake)));
    load(&paths, &testing::files("m"));
    wait_ready(&paths);

    let (scan, _) = Conn::greeted(&paths);
    scan.send(Outgoing::Recognize(&img()));
    thread::sleep(Duration::from_millis(50));
    let started = Instant::now();
    shutdown(&paths);
    assert_eq!(finish(server), Outcome::Shutdown);
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[test]
fn engine_that_cannot_run_makes_the_daemon_leave() {
    let cases: [(Factory, EngineState); 2] = [
        (Arc::new(|_: &ModelFiles| Err(BuildError::NoGpu)), EngineState::NoGpu),
        (
            Arc::new(|_: &ModelFiles| Err(BuildError::Failed("boom".into()))),
            EngineState::Failed("boom".into()),
        ),
    ];
    for (make, expected) in cases {
        let (dir, paths) = dir_and_paths("unusable");
        let server = start(config(&dir, make));
        load(&paths, &testing::files("m"));
        assert!(wait_until(LONG, || state(&paths).as_ref() == Some(&expected)));
        assert_eq!(finish(server), Outcome::Unusable);
        assert!(lock_free(&paths.lock));
    }
}

#[test]
fn panicking_loader_is_reported_as_failed() {
    let (dir, paths) = dir_and_paths("loader-panic");
    let make: Factory = Arc::new(|_: &ModelFiles| panic!("loader panicked on purpose"));
    let server = start(config(&dir, make));
    load(&paths, &testing::files("m"));
    assert!(wait_until(LONG, || matches!(state(&paths), Some(EngineState::Failed(_)))));
    assert_eq!(finish(server), Outcome::Unusable);
}

#[test]
fn crashing_engine_is_dropped_and_the_daemon_leaves() {
    let (dir, paths) = dir_and_paths("crash");
    let server = start(config(&dir, factory(Fake::new("d").panicking())));
    load(&paths, &testing::files("m"));
    wait_ready(&paths);
    let (conn, _) = Conn::greeted(&paths);
    assert!(matches!(conn.ask(Outgoing::Recognize(&img())), Reply::Error(_)));
    assert_eq!(finish(server), Outcome::Unusable);
}

#[test]
fn too_long_socket_path_is_an_error_not_a_panic() {
    let root = testing::temp_dir("long");
    let dir = root.join("x".repeat(110));
    let result = server::serve(config(&dir, factory(Fake::new("d"))));
    assert!(result.is_err(), "{result:?}");
    assert!(lock_free(&Paths::in_dir(dir).lock));
}

#[test]
fn only_the_latest_requested_model_is_kept() {
    let (dir, paths) = dir_and_paths("switch");
    let make: Factory = Arc::new(|files: &ModelFiles| {
        if model_name(files).contains("slow") {
            thread::sleep(Duration::from_millis(300));
        }
        Ok(Engine {
            backend: Box::new(Fake::new("d")),
            device: "fake".into(),
        })
    });
    let server = start(config(&dir, make));
    let fast = testing::files("fast");
    load(&paths, &testing::files("slow"));
    load(&paths, &fast);
    let wanted = ready(&fast);
    assert!(wait_until(LONG, || state(&paths).as_ref() == Some(&wanted)));
    thread::sleep(Duration::from_millis(450));
    assert_eq!(state(&paths), Some(wanted), "the slow, older build overwrote the newer model");

    shutdown(&paths);
    assert_eq!(finish(server), Outcome::Shutdown);
}

#[test]
fn switching_models_during_a_scan_keeps_the_answer() {
    let (dir, paths) = dir_and_paths("switch-scan");
    let make: Factory = Arc::new(|files: &ModelFiles| {
        let label = if model_name(files).contains("second") { "second" } else { "first" };
        Ok(Engine {
            backend: Box::new(Fake::new(label).slow(60, Duration::from_millis(5))),
            device: "fake".into(),
        })
    });
    let server = start(config(&dir, make));
    let (first, second) = (testing::files("first"), testing::files("second"));
    load(&paths, &first);
    wait_ready(&paths);

    let (scan, _) = Conn::greeted(&paths);
    scan.send(Outgoing::Recognize(&img()));
    thread::sleep(Duration::from_millis(50));
    load(&paths, &second);
    match scan.reply() {
        Reply::Lines(text) => assert_eq!(label_of(&text), "first", "the running scan kept its engine"),
        other => panic!("expected lines, got {other:?}"),
    }
    let wanted = ready(&second);
    assert!(wait_until(LONG, || state(&paths).as_ref() == Some(&wanted)));

    shutdown(&paths);
    assert_eq!(finish(server), Outcome::Shutdown);
}

#[test]
fn scan_before_the_engine_is_ready_is_refused_quickly() {
    let (dir, paths) = dir_and_paths("not-ready");
    let server = start(config(&dir, delayed(Fake::new("d"), Duration::from_millis(500))));
    let (conn, _) = Conn::greeted(&paths);
    assert!(matches!(conn.ask(Outgoing::Recognize(&img())), Reply::Error(_)), "idle");
    assert_eq!(conn.ask(Outgoing::Load(&testing::files("m"))), Reply::Ok);
    let started = Instant::now();
    assert!(matches!(conn.ask(Outgoing::Recognize(&img())), Reply::Error(_)), "loading");
    assert!(started.elapsed() < Duration::from_millis(200));

    shutdown(&paths);
    assert_eq!(finish(server), Outcome::Shutdown);
}

#[test]
fn status_and_stop_commands() {
    let (dir, paths) = dir_and_paths("stop");
    let server = start(config(&dir, factory(Fake::new("d"))));
    let status = client::status(&paths).expect("a running daemon answers status");
    assert_eq!(status.pid, std::process::id());
    assert!(status.idle_left_secs.is_some_and(|secs| secs <= 60));

    let obeys = client::stop(&paths, Duration::from_secs(2), &|_| panic!("an obedient daemon must not be killed"));
    assert_eq!(obeys, Ok(true));
    assert_eq!(finish(server), Outcome::Shutdown);
    assert!(client::status(&paths).is_none());
    assert_eq!(client::stop(&paths, Duration::from_millis(200), &|_| panic!("nothing to kill")), Ok(false));
}

// ---------------------------------------------------------------- client

#[test]
fn client_starts_the_daemon_and_moves_to_it_when_warm() {
    let (dir, paths) = dir_and_paths("warm");
    let probe = Probe::default();
    let servers = Servers::default();
    let spawn = thread_spawner(&dir, delayed(Fake::new("daemon"), Duration::from_millis(300)), &probe, &servers);

    let backend = client::connect_with(client_config(&paths, spawn, &probe), &testing::files("m")).unwrap();
    assert_eq!(probe.spawns(), 1);
    assert_eq!(read(&*backend), "local", "a cold daemon must not make the user wait");
    assert!(wait_until(LONG, || read(&*backend) == "daemon"));
    assert_eq!(probe.spawns(), 1);
    assert_eq!(probe.locals(), 1);
    assert!(read_strikes(&paths).is_empty());

    shutdown(&paths);
    assert_eq!(finish_all(&servers), vec![Outcome::Shutdown]);
}

#[test]
fn daemon_dying_mid_scan_means_a_local_read_and_a_strike() {
    let (_dir, paths) = dir_and_paths("dies");
    let files = testing::files("m");
    let state = ready(&files);
    let daemon = fake_daemon(&paths, 2, move |stream| {
        answer_hello(&stream, state.clone());
        let mut reader = &stream;
        // Accept the image and hang up without sending a reply.
        let _ = protocol::read_request(&mut reader);
    });
    let probe = Probe::default();
    let backend = client::connect_with(client_config(&paths, refusing_spawner(&probe), &probe), &files).unwrap();

    assert_eq!(read(&*backend), "local");
    assert!(read_strikes(&paths).contains("failed"));
    daemon.join().unwrap();
    assert_eq!(read(&*backend), "local");
    assert_eq!(probe.spawns(), 0, "a daemon that broke must not be retried in the same session");
    assert_eq!(probe.locals(), 1);
}

#[test]
fn hung_daemon_is_killed_and_the_scan_read_locally() {
    let (_dir, paths) = dir_and_paths("hung");
    let files = testing::files("m");
    let state = ready(&files);
    let (release, released) = std::sync::mpsc::channel::<()>();
    let released = Mutex::new(released);
    let daemon = fake_daemon(&paths, 2, move |stream| {
        answer_hello(&stream, state.clone());
        let mut reader = &stream;
        if protocol::read_request(&mut reader).is_ok() {
            let _ = released.lock().unwrap().recv_timeout(LONG);
        }
    });
    let probe = Probe::default();
    let config = client::Config {
        timing: Timing {
            reply_base: Duration::from_millis(300),
            ..timing()
        },
        ..client_config(&paths, refusing_spawner(&probe), &probe)
    };
    let backend = client::connect_with(config, &files).unwrap();

    let started = Instant::now();
    assert_eq!(read(&*backend), "local");
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(probe.kills(), vec![std::process::id() as i32]);
    assert!(read_strikes(&paths).contains("failed"));
    drop(release);
    daemon.join().unwrap();
}

#[test]
fn handshake_with_a_leaving_daemon_is_retried_without_a_strike() {
    let (dir, paths) = dir_and_paths("leaving");
    let probe = Probe::default();
    let servers = Servers::default();
    let spawn = thread_spawner(&dir, factory(Fake::new("fresh")), &probe, &servers);
    let socket = paths.socket.clone();
    let leaving = fake_daemon(&paths, 1, move |stream| {
        let _ = fs::remove_file(&socket);
        drop(stream);
    });

    let backend = client::connect_with(client_config(&paths, spawn, &probe), &testing::files("m")).unwrap();
    leaving.join().unwrap();
    assert!(read_strikes(&paths).is_empty(), "a daemon on its way out is not a failure");
    assert_eq!(probe.spawns(), 1);
    assert!(wait_until(LONG, || read(&*backend) == "fresh"));

    shutdown(&paths);
    assert_eq!(finish_all(&servers), vec![Outcome::Shutdown]);
}

#[test]
fn a_successful_scan_clears_old_strikes() {
    let (dir, paths) = dir_and_paths("clear-strikes");
    record_strike(&paths, BUILD, Strike::Failed);
    record_strike(&paths, BUILD, Strike::Failed);
    let probe = Probe::default();
    let servers = Servers::default();
    let spawn = thread_spawner(&dir, factory(Fake::new("daemon")), &probe, &servers);

    let backend = client::connect_with(client_config(&paths, spawn, &probe), &testing::files("m")).unwrap();
    assert!(wait_until(LONG, || read(&*backend) == "daemon"));
    assert!(read_strikes(&paths).is_empty(), "a working daemon forgets old failures");

    shutdown(&paths);
    assert_eq!(finish_all(&servers), vec![Outcome::Shutdown]);
}

#[test]
fn daemon_from_another_build_is_replaced_exactly_once() {
    let (dir, paths) = dir_and_paths("foreign");
    let old = start(server::Config {
        build: "old-build".into(),
        ..config(&dir, factory(Fake::new("old")))
    });
    let probe = Probe::default();
    let servers = Servers::default();
    let spawn = thread_spawner(&dir, factory(Fake::new("new")), &probe, &servers);

    let backend = client::connect_with(client_config(&paths, spawn, &probe), &testing::files("m")).unwrap();
    assert_eq!(finish(old), Outcome::Shutdown);
    assert_eq!(probe.spawns(), 1);
    assert_eq!(Conn::greeted(&paths).1.build, BUILD);
    assert!(wait_until(LONG, || read(&*backend) == "new"));
    assert!(probe.kills().is_empty(), "the old daemon obeyed, nobody should be killed");

    shutdown(&paths);
    assert_eq!(finish_all(&servers), vec![Outcome::Shutdown]);
}

#[test]
fn concurrent_clients_start_a_single_daemon() {
    let (dir, paths) = dir_and_paths("race");
    let probe = Probe::default();
    let servers = Servers::default();
    let spawn = thread_spawner(&dir, factory(Fake::new("daemon")), &probe, &servers);

    let clients: Vec<_> = (0..6)
        .map(|_| {
            let config = client_config(&paths, spawn.clone(), &probe);
            thread::spawn(move || {
                client::connect_with(config, &testing::files("m"))
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            })
        })
        .collect();
    for client in clients {
        client.join().unwrap().unwrap();
    }
    assert!(read_strikes(&paths).is_empty(), "a client gave up: {}", read_strikes(&paths));
    assert!(probe.spawns() >= 1);

    // Unsuccessful lock competitors wait for `lock_wait`: allow them to exit before issuing `Shutdown` for the winner.
    thread::sleep(Duration::from_millis(250));
    shutdown(&paths);
    let outcomes = finish_all(&servers);
    let serving = outcomes.iter().filter(|o| **o != Outcome::AlreadyRunning).count();
    assert_eq!(serving, 1, "{outcomes:?}");
}

#[test]
fn strike_rules() {
    let now = 10_000;
    let failed = |ago: u64| format!("{} failed {BUILD}\n", now - ago);
    assert!(!daemon_blocked("", BUILD, now));

    let two = failed(10) + &failed(20);
    assert!(!daemon_blocked(&two, BUILD, now));
    assert!(daemon_blocked(&(two.clone() + &failed(30)), BUILD, now));
    assert!(!daemon_blocked(&(two.clone() + &failed(601)), BUILD, now), "old failures expire");
    assert!(!daemon_blocked(&(two + &format!("{now} failed other-build\n")), BUILD, now));

    let no_gpu = format!("{now} no-gpu {BUILD}\n");
    assert!(daemon_blocked(&no_gpu, BUILD, now + 10_000_000), "no GPU does not expire for a build");
    assert!(gpu_ruled_out(&no_gpu, BUILD));
    assert!(!gpu_ruled_out(&no_gpu, "next-build"));
    assert!(!daemon_blocked("junk\n\n1 2 3\nnan failed test-build\n42 exploded test-build\n", BUILD, now));
}

#[test]
fn repeated_failures_stop_spawning_for_a_while() {
    let (_dir, paths) = dir_and_paths("strikes");
    for _ in 0..20 {
        record_strike(&paths, BUILD, Strike::Failed);
    }
    assert_eq!(read_strikes(&paths).lines().count(), 8, "the strike file must stay small");
    assert!(daemon_blocked(&read_strikes(&paths), BUILD, unix_now()));

    let probe = Probe::default();
    let backend =
        client::connect_with(client_config(&paths, refusing_spawner(&probe), &probe), &testing::files("m")).unwrap();
    assert_eq!(read(&*backend), "local");
    assert_eq!(probe.spawns(), 0);
}

#[test]
fn daemon_without_gpu_is_remembered_and_not_started_again() {
    let (dir, paths) = dir_and_paths("no-gpu");
    let probe = Probe::default();
    let servers = Servers::default();
    let spawn = thread_spawner(&dir, Arc::new(|_: &ModelFiles| Err(BuildError::NoGpu)), &probe, &servers);

    let backend = client::connect_with(client_config(&paths, spawn, &probe), &testing::files("m")).unwrap();
    assert!(wait_until(LONG, || {
        read(&*backend) == "local" && gpu_ruled_out(&read_strikes(&paths), BUILD)
    }));
    assert_eq!(probe.spawns(), 1);
    assert_eq!(finish_all(&servers), vec![Outcome::Unusable]);

    let again =
        client::connect_with(client_config(&paths, refusing_spawner(&probe), &probe), &testing::files("m")).unwrap();
    assert_eq!(read(&*again), "local");
    assert_eq!(probe.spawns(), 1);
}

#[test]
fn cancelling_while_the_daemon_scans_returns_at_once() {
    let (dir, paths) = dir_and_paths("cancel");
    let fake = Fake::new("daemon").slow(400, Duration::from_millis(5));
    let probe = Probe::default();
    let servers = Servers::default();
    let spawn = thread_spawner(&dir, factory(fake.clone()), &probe, &servers);
    let backend = client::connect_with(client_config(&paths, spawn, &probe), &testing::files("m")).unwrap();
    wait_ready(&paths);

    let flag = Arc::new(AtomicBool::new(false));
    let setter = {
        let flag = flag.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(80));
            flag.store(true, Ordering::SeqCst);
        })
    };
    let started = Instant::now();
    let result = backend.recognize(img(), &|| flag.load(Ordering::SeqCst));
    assert_eq!(result.unwrap_err().to_string(), CANCELLED);
    assert!(started.elapsed() < Duration::from_millis(600));
    assert!(wait_until(LONG, || fake.cancels() == 1), "the daemon did not notice the cancel");
    assert!(read_strikes(&paths).is_empty(), "a cancel is not a failure");
    assert_eq!(probe.locals(), 0);

    setter.join().unwrap();
    shutdown(&paths);
    assert_eq!(finish_all(&servers), vec![Outcome::Shutdown]);
}

// ---------------------------------------------------------------- real processes

const CHILD_DIR: &str = "LUMINE_TEST_DAEMON_DIR";
const CHILD_SCAN_MS: &str = "LUMINE_TEST_DAEMON_SCAN_MS";
const CHILD_FD: &str = "LUMINE_TEST_DAEMON_FD";
const CHILD_CRASH: &str = "LUMINE_TEST_DAEMON_CRASH";

#[test]
#[ignore = "entry point of the daemon processes started by the tests below"]
fn child_daemon() {
    let Some(dir) = std::env::var_os(CHILD_DIR).map(PathBuf::from) else {
        return;
    };
    let _ = fs::write(dir.join(format!("pid-{}", std::process::id())), "");
    if let Some(fd) = std::env::var(CHILD_FD).ok().and_then(|v| v.parse::<i32>().ok()) {
        // SAFETY: Only queries flags for the provided descriptor number without opening or closing file handles.
        let open = unsafe { nix::libc::fcntl(fd, nix::libc::F_GETFD) } != -1;
        let _ = fs::write(dir.join("fd-inherited"), if open { "yes" } else { "no" });
    }
    let scan_ms: usize = std::env::var(CHILD_SCAN_MS)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let fake = Fake::new("child").slow(scan_ms / 5, Duration::from_millis(5));
    let factory = match std::env::var_os(CHILD_CRASH) {
        Some(_) => Arc::new(|_: &ModelFiles| -> Result<Engine, BuildError> { std::process::abort() }),
        None => factory(fake),
    };
    let _ = server::serve(server::Config {
        idle: Some(Duration::from_secs(30)),
        ..config(&dir, factory)
    });
    std::process::exit(0);
}

fn child_command(dir: &Path, scan_ms: u64) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "ocr::daemon::tests::child_daemon",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD_DIR, dir)
        .env(CHILD_SCAN_MS, scan_ms.to_string());
    command
}

fn child_spawner(dir: &Path, scan_ms: u64, probe: &Probe) -> Spawner {
    let (dir, spawns) = (dir.to_path_buf(), probe.spawns.clone());
    Arc::new(move || {
        spawns.fetch_add(1, Ordering::SeqCst);
        spawn_detached(child_command(&dir, scan_ms), Some(&dir.join("child.log")))
    })
}

fn alive(pid: i32) -> bool {
    fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| {
        stat.rsplit_once(')')
            .is_some_and(|(_, rest)| !rest.trim_start().starts_with('Z'))
    })
}

fn children(dir: &Path) -> Vec<i32> {
    let mut pids: Vec<i32> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| entry.file_name().to_str()?.strip_prefix("pid-")?.parse().ok())
        .filter(|&pid| alive(pid))
        .collect();
    pids.sort_unstable();
    pids
}

fn ready_pid(paths: &Paths) -> i32 {
    assert!(wait_until(LONG, || {
        client::status(paths).is_some_and(|status| matches!(status.state, EngineState::Ready { .. }))
    }));
    client::status(paths).unwrap().pid as i32
}

#[test]
fn real_daemon_processes_do_not_stack() {
    let (dir, paths) = dir_and_paths("proc-stack");
    let probe = Probe::default();
    let spawn = child_spawner(&dir, 0, &probe);

    let clients: Vec<_> = (0..5)
        .map(|_| {
            let config = client_config(&paths, spawn.clone(), &probe);
            thread::spawn(move || {
                client::connect_with(config, &testing::files("m"))
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            })
        })
        .collect();
    for client in clients {
        client.join().unwrap().unwrap();
    }
    assert!(read_strikes(&paths).is_empty(), "a client gave up: {}", read_strikes(&paths));
    assert!(
        wait_until(LONG, || children(&dir).len() == 1),
        "live daemons: {:?}",
        children(&dir)
    );
    let pid = ready_pid(&paths);
    assert_eq!(children(&dir), vec![pid]);

    assert_eq!(client::stop(&paths, Duration::from_secs(2), &client::kill_hard), Ok(true));
    assert!(wait_until(LONG, || children(&dir).is_empty()));
}


#[test]
fn killed_daemon_is_survived_and_replaced() {
    let (dir, paths) = dir_and_paths("proc-kill");
    let probe = Probe::default();
    let spawn = child_spawner(&dir, 1500, &probe);
    let files = testing::files("m");

    let backend = client::connect_with(client_config(&paths, spawn.clone(), &probe), &files).unwrap();
    let pid = ready_pid(&paths);
    let killer = thread::spawn(move || {
        thread::sleep(Duration::from_millis(300));
        kill(Pid::from_raw(pid), Signal::SIGKILL)
    });
    assert_eq!(read(&*backend), "local");
    killer.join().unwrap().unwrap();
    assert!(wait_until(LONG, || !alive(pid)));
    assert!(read_strikes(&paths).contains("failed"));

    let fresh = client::connect_with(client_config(&paths, spawn, &probe), &files).unwrap();
    let new_pid = ready_pid(&paths);
    assert_ne!(new_pid, pid);
    assert_eq!(read(&*fresh), "child");
    assert_eq!(probe.spawns(), 2);

    assert_eq!(client::stop(&paths, Duration::from_secs(2), &client::kill_hard), Ok(true));
    assert!(wait_until(LONG, || !alive(new_pid)));
}

#[test]
fn hung_real_daemon_is_killed_by_the_watchdog() {
    let (dir, paths) = dir_and_paths("proc-hung");
    let probe = Probe::default();
    let config = client::Config {
        kill: Arc::new(client::kill_hard),
        timing: Timing {
            reply_base: Duration::from_millis(400),
            ..timing()
        },
        ..client_config(&paths, child_spawner(&dir, 20_000, &probe), &probe)
    };
    let backend = client::connect_with(config, &testing::files("m")).unwrap();
    let pid = ready_pid(&paths);

    let started = Instant::now();
    assert_eq!(read(&*backend), "local");
    assert!(started.elapsed() < Duration::from_secs(3));
    assert!(wait_until(LONG, || !alive(pid)), "the hung daemon is still alive");
    assert!(wait_until(LONG, || lock_free(&paths.lock)));
}

#[test]
fn daemon_that_dies_while_loading_is_not_respawned_forever() {
    let (dir, paths) = dir_and_paths("proc-crash");
    let probe = Probe::default();
    let spawns = probe.spawns.clone();
    let root = dir.to_path_buf();
    let spawn: Spawner = Arc::new(move || {
        spawns.fetch_add(1, Ordering::SeqCst);
        let mut command = child_command(&root, 0);
        command.env(CHILD_CRASH, "1");
        spawn_detached(command, Some(&root.join("child.log")))
    });
    let backend = client::connect_with(client_config(&paths, spawn, &probe), &testing::files("m")).unwrap();
    for _ in 0..6 {
        assert!(wait_until(LONG, || children(&dir).is_empty()));
        assert_eq!(read(&*backend), "local");
    }
    assert_eq!(probe.spawns(), 1, "every scan started a daemon that died");
    assert!(read_strikes(&paths).contains("failed"));
    assert!(wait_until(LONG, || children(&dir).is_empty()));
}

#[test]
fn a_daemon_that_died_on_the_gpu_is_not_followed_onto_it() {
    if !crate::ocr::settings::GPU_BUILD {
        return;
    }
    let (_dir, paths) = dir_and_paths("gpu-marker");
    fs::write(paths.dir.join("ocr.gpu-loading"), "1").unwrap();
    let factory = super::engine_factory(crate::ocr::settings::Device::Gpu, paths.clone());
    assert!(matches!(factory(&testing::files("m")), Err(BuildError::NoGpu)));
    assert!(!paths.dir.join("ocr.gpu-loading").exists());
}

#[test]
fn stop_kills_a_frozen_daemon() {
    let (dir, paths) = dir_and_paths("proc-frozen");
    let probe = Probe::default();
    (child_spawner(&dir, 0, &probe))().unwrap();
    assert!(wait_until(LONG, || client::status(&paths).is_some()));
    let pid = client::status(&paths).unwrap().pid as i32;

    kill(Pid::from_raw(pid), Signal::SIGSTOP).unwrap();
    assert!(client::status(&paths).is_none(), "a frozen daemon cannot answer");
    assert_eq!(client::stop(&paths, Duration::from_millis(500), &client::kill_hard), Ok(true));
    assert!(wait_until(LONG, || !alive(pid)));
}

#[test]
fn daemon_does_not_inherit_descriptors() {
    let (dir, paths) = dir_and_paths("proc-fd");
    let file = fs::File::open("/proc/self/stat").unwrap();
    // dup without CLOEXEC — simulates an fd left open by a third-party library
    let leaked = nix::unistd::dup(&file).unwrap();
    let mut command = child_command(&dir, 0);
    command.env(CHILD_FD, leaked.as_raw_fd().to_string());
    spawn_detached(command, Some(&dir.join("child.log"))).unwrap();

    let marker = dir.join("fd-inherited");
    assert!(wait_until(LONG, || fs::read_to_string(&marker).is_ok_and(|s| !s.is_empty())));
    assert_eq!(fs::read_to_string(&marker).unwrap(), "no");
    assert!(wait_until(LONG, || client::status(&paths).is_some()));
    assert_eq!(client::stop(&paths, Duration::from_secs(2), &client::kill_hard), Ok(true));
}

use super::calibrate::{self, Record, Verdict};

fn measured(verdict: Verdict, cpu_ms: Option<u32>, gpu_ms: Option<u32>) -> Record {
    Record {
        verdict,
        cpu_ms,
        gpu_ms,
        at: 1_700_000_000,
    }
}

#[test]
fn a_measurement_survives_a_save_and_a_reload() {
    let dir = testing::temp_dir("calib-roundtrip");
    let path = dir.join("ocr-device");
    let record = measured(Verdict::Gpu, Some(1650), Some(680));
    calibrate::store(&path, &record, "card-a");
    assert_eq!(calibrate::stored(&path, "card-a"), Some(record));
}

#[test]
fn a_measurement_from_other_hardware_is_ignored() {
    let dir = testing::temp_dir("calib-foreign");
    let path = dir.join("ocr-device");
    calibrate::store(&path, &measured(Verdict::Gpu, Some(1650), Some(680)), "card-a");
    assert_eq!(calibrate::stored(&path, "card-b"), None);
    assert_eq!(calibrate::stored(&dir.join("nothing-here"), "card-a"), None);
}

#[test]
fn a_damaged_measurement_is_ignored() {
    for text in [
        "",
        "garbage",
        "verdict = gpu\n",
        "format = 1\nkey = k\nverdict = maybe\n",
        "format = 99\nkey = k\nverdict = gpu\n",
        "format = 1\nkey = other\nverdict = gpu\n",
    ] {
        assert_eq!(calibrate::parse(text, "k"), None, "accepted {text:?}");
    }
}

#[test]
fn an_engine_that_could_not_be_measured_is_kept_as_failed() {
    let dir = testing::temp_dir("calib-failed");
    let path = dir.join("ocr-device");
    let record = measured(Verdict::Cpu, Some(1500), None);
    calibrate::store(&path, &record, "card-a");
    assert!(fs::read_to_string(&path).unwrap().contains("gpu_ms = failed"));
    assert_eq!(calibrate::stored(&path, "card-a"), Some(record));
}

#[test]
fn the_gpu_has_to_win_by_a_margin() {
    assert_eq!(calibrate::decide(Some(1650), Some(680)), Verdict::Gpu);
    assert_eq!(calibrate::decide(Some(1000), Some(800)), Verdict::Gpu);
    assert_eq!(calibrate::decide(Some(1000), Some(801)), Verdict::Cpu);
    assert_eq!(calibrate::decide(Some(2000), Some(40_000)), Verdict::Cpu);
    assert_eq!(calibrate::decide(Some(1000), None), Verdict::Cpu);
    assert_eq!(calibrate::decide(None, Some(500)), Verdict::Gpu);
    assert_eq!(calibrate::decide(None, None), Verdict::Cpu);
}

#[test]
fn the_probe_image_is_the_same_every_time() {
    let first = calibrate::probe_image();
    let again = calibrate::probe_image();
    assert_eq!(first.rgb, again.rgb);
    assert_eq!(first.rgb.len(), (first.width as usize) * (first.height as usize) * 3);
    assert!(first.rgb.iter().any(|&value| value < 0x40), "no dark bars to detect");
    assert!(first.rgb.iter().any(|&value| value > 0xC0), "no light background");
}

#[test]
fn the_machine_key_does_not_drift() {
    let key = calibrate::machine_key();
    assert_eq!(key, calibrate::machine_key());
    assert!(key.starts_with(&format!("v{}", calibrate::FORMAT)), "{key}");
    assert!(key.contains(env!("CARGO_PKG_VERSION")), "{key}");
}

#[test]
fn a_summary_names_the_winner_and_the_numbers() {
    let gpu = measured(Verdict::Gpu, Some(1650), Some(680)).summary();
    assert!(gpu.contains("GPU is 2.4x faster"), "{gpu}");
    assert!(gpu.contains("CPU 1650 ms") && gpu.contains("GPU 680 ms"), "{gpu}");

    let cpu = measured(Verdict::Cpu, Some(2000), Some(40_000)).summary();
    assert!(cpu.contains("CPU is 20.0x faster"), "{cpu}");

    let broken = measured(Verdict::Cpu, Some(1500), None).summary();
    assert!(broken.contains("GPU failed"), "{broken}");
}

#[test]
#[ignore]
fn auto_lands_on_the_device_this_machine_measured() {
    let models = crate::ocr::models::OcrModels::load();
    let files = models
        .active()
        .and_then(|idx| models.files(idx))
        .expect("install an OCR model first");
    let record = super::measurement().expect("run --ocr-daemon calibrate first");

    let (dir, paths) = dir_and_paths("auto-real");
    let factory = super::engine_factory(crate::ocr::settings::Device::Auto, paths.clone());
    let server = start(config(&dir, factory));
    load(&paths, &files);
    assert!(wait_until(Duration::from_secs(300), || matches!(
        state(&paths),
        Some(EngineState::Ready { .. } | EngineState::NoGpu)
    )));

    match (record.verdict, state(&paths)) {
        (Verdict::Gpu, Some(EngineState::Ready { device, .. })) => {
            assert_eq!(device, "webgpu");
            shutdown(&paths);
        }
        (Verdict::Cpu, Some(EngineState::NoGpu)) => {}
        (verdict, reached) => panic!("measured {verdict:?} but the daemon reached {reached:?}"),
    }
    finish(server);
}

fn gpu_traces() -> Vec<String> {
    let maps = fs::read_to_string("/proc/self/maps").unwrap_or_default();
    let mut traces: Vec<String> = maps
        .lines()
        .filter_map(|line| line.split_whitespace().nth(5))
        .filter(|lib| ["libwebgpu_dawn", "libvulkan", "libnvidia", "libGLX", "libEGL", "libdrm", "_dri"].iter().any(|name| lib.contains(name)))
        .map(str::to_owned)
        .collect();
    for fd in fs::read_dir("/proc/self/fd").into_iter().flatten().filter_map(Result::ok) {
        if let Ok(target) = fs::read_link(fd.path())
            && (target.starts_with("/dev/dri") || target.to_string_lossy().starts_with("/dev/nvidia"))
        {
            traces.push(target.display().to_string());
        }
    }
    traces.sort();
    traces.dedup();
    traces
}

fn read_probe(gpu: bool) -> Vec<String> {
    let models = crate::ocr::models::OcrModels::load();
    let files = models
        .active()
        .and_then(|idx| models.files(idx))
        .expect("install an OCR model first");
    let backend: Box<dyn OcrBackend> = if gpu {
        Box::new(crate::ocr::paddle_backend::PaddleBackend::on_gpu(&files).expect("gpu engine"))
    } else {
        crate::ocr::build_backend(crate::ocr::settings::Mode::OnDemand, &files).expect("local engine")
    };
    backend.recognize(super::calibrate::probe_image(), &never).expect("scan");
    gpu_traces()
}

#[test]
#[ignore]
fn the_local_engine_never_touches_the_gpu() {
    let traces = read_probe(false);
    assert!(traces.is_empty(), "on-demand OCR opened the GPU: {traces:?}");
}

#[test]
#[ignore]
fn the_gpu_engine_is_seen_by_the_check_above() {
    let traces = read_probe(true);
    assert!(!traces.is_empty(), "the GPU engine left no trace, so the check proves nothing");
}

#[test]
#[ignore]
fn local_engine_cost() {
    let models = crate::ocr::models::OcrModels::load();
    let files = models.active().and_then(|idx| models.files(idx)).expect("install an OCR model first");
    let started = Instant::now();
    let backend = crate::ocr::build_backend(crate::ocr::settings::Mode::OnDemand, &files).unwrap();
    let build_ms = started.elapsed().as_millis();
    let mut scans = Vec::new();
    for _ in 0..5 {
        let started = Instant::now();
        backend.recognize(super::calibrate::probe_image(), &never).unwrap();
        scans.push(started.elapsed().as_millis());
    }
    let status = fs::read_to_string("/proc/self/status").unwrap();
    let field = |name: &str| status.lines().find(|l| l.starts_with(name)).unwrap_or_default().to_owned();
    println!("COST build {build_ms} ms, scans {scans:?} ms, {}, {}", field("VmHWM"), field("Threads"));
}
