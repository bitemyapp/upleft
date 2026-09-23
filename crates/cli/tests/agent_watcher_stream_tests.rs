//! The FSEvents stream behind `down watch`, end to end without launching
//! anything: a Markdown write under a watched directory is reported once,
//! other files are not. (Not a port of a Swift test; Downright tests only
//! the planning step. `down watch` itself launches the app, so the
//! conformance suite cannot run it.)

use std::sync::mpsc;
use std::time::Duration;

use upleft_cli::agent_watcher::AgentWatcher;
use upleft_foundation::foundation_io;
use upleft_foundation::url::FileUrl;

#[test]
fn reports_markdown_writes_through_fsevents() {
    let directory = std::env::temp_dir().join(format!("downright-watch-stream-{}", foundation_io::uuid_string()));
    std::fs::create_dir_all(&directory).unwrap();
    // FSEvents reports real paths (`/private/var/...`).
    let directory = directory.canonicalize().unwrap();

    let (sender, receiver) = mpsc::channel::<Vec<String>>();
    let watcher = AgentWatcher::new(vec![FileUrl::from_path(directory.to_str().unwrap())], 0.2, move |urls| {
        let _ = sender.send(urls.iter().map(FileUrl::path).collect());
    });
    assert!(watcher.start());
    // Let the stream settle before writing.
    std::thread::sleep(Duration::from_millis(500));

    std::fs::write(directory.join("ignored.txt"), "x").unwrap();
    std::fs::write(directory.join("notes.md"), "# hi").unwrap();

    let reported = receiver.recv_timeout(Duration::from_secs(10)).expect("the write is reported");
    assert_eq!(reported.len(), 1, "{reported:?}");
    assert!(reported[0].ends_with("/notes.md"), "{reported:?}");

    watcher.stop();
    drop(watcher);
    let _ = std::fs::remove_dir_all(&directory);
}
