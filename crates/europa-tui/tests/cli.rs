use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn europa() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_europa"));
    command.stdin(Stdio::null());
    command
}

fn case_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("europa-cli-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create");
    dir
}

fn write_file(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    let mut file = std::fs::File::create(&path).expect("create file");
    file.write_all(bytes).expect("write");
    path
}

fn valid_notebook() -> Vec<u8> {
    br#"{"nbformat":4,"nbformat_minor":5,"metadata":{},"cells":[]}"#.to_vec()
}

type DirSnapshot = (Vec<String>, Vec<(PathBuf, Vec<u8>, std::time::SystemTime)>);

fn snapshot_dir(dir: &Path) -> DirSnapshot {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("list")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    let mut files = Vec::new();
    for name in &names {
        let path = dir.join(name);
        if !path.is_file() {
            continue;
        }
        let bytes = std::fs::read(&path).expect("read");
        let modified = std::fs::metadata(&path)
            .expect("metadata")
            .modified()
            .expect("modified");
        files.push((path.clone(), bytes, modified));
    }
    (names, files)
}

fn assert_unchanged(before: &DirSnapshot, dir: &Path) {
    let (names, files) = snapshot_dir(dir);
    assert_eq!(&names, &before.0, "directory listing changed");
    for ((path, bytes, modified), (before_path, before_bytes, before_modified)) in
        files.iter().zip(before.1.iter())
    {
        assert_eq!(path, before_path);
        assert_eq!(bytes, before_bytes, "{path:?} changed");
        assert_eq!(modified, before_modified, "{path:?} timestamp changed");
    }
}

#[test]
fn help_prints_usage_to_stdout() {
    for args in [["--help"], ["-h"]] {
        let output = europa().args(args).output().expect("run");
        assert_eq!(output.status.code(), Some(0), "{args:?}");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("Usage"), "{stdout:?}");
        assert!(output.stderr.is_empty(), "{:?}", output.stderr);
    }
}

#[test]
fn wrong_argument_counts_print_usage_to_stderr() {
    let dir = case_dir("args");
    let file = write_file(&dir, "a.ipynb", &valid_notebook());
    let cases: Vec<Vec<String>> = vec![
        vec![],
        vec![
            file.to_string_lossy().into_owned(),
            file.to_string_lossy().into_owned(),
        ],
        vec!["--unknown".to_owned(), file.to_string_lossy().into_owned()],
    ];
    for args in cases {
        let output = europa().args(&args).output().expect("run");
        assert_eq!(output.status.code(), Some(64), "{args:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("Usage"), "{stderr:?} for {args:?}");
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn missing_and_broken_inputs_keep_files_unchanged() {
    let dir = case_dir("inputs");
    let valid = write_file(&dir, "ok.ipynb", &valid_notebook());
    let broken = write_file(&dir, "broken.ipynb", b"not json");
    let old = write_file(
        &dir,
        "v3.ipynb",
        br#"{"nbformat":3,"nbformat_minor":0,"metadata":{},"cells":[]}"#,
    );
    let sub = dir.join("sub");
    std::fs::create_dir_all(&sub).expect("subdir");
    let before = snapshot_dir(&dir);
    let missing = dir.join("missing.ipynb");
    let output = europa().arg(&missing).output().expect("run missing");
    assert_eq!(output.status.code(), Some(66));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("missing.ipynb"), "{stderr:?}");
    let output = europa().arg(&sub).output().expect("run dir");
    assert_eq!(output.status.code(), Some(66));
    let output = europa().arg(&broken).output().expect("run broken");
    assert_eq!(output.status.code(), Some(65));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("broken.ipynb"), "{stderr:?}");
    assert!(stderr.contains("invalid JSON at"), "{stderr:?}");
    let output = europa().arg(&old).output().expect("run v3");
    assert_eq!(output.status.code(), Some(65));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("invalid nbformat"), "{stderr:?}");
    let _ = valid;
    assert_unchanged(&before, &dir);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn non_terminal_input_fails_without_touching_files() {
    let dir = case_dir("terminal");
    let valid = write_file(&dir, "ok.ipynb", &valid_notebook());
    let before = snapshot_dir(&dir);
    let output = europa().arg(&valid).output().expect("run");
    assert_eq!(output.status.code(), Some(74), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not a terminal"), "{stderr:?}");
    assert!(output.stdout.is_empty());
    assert!(!output.stdout.contains(&0x1b));
    assert!(!output.stderr.contains(&0x1b));
    assert_unchanged(&before, &dir);
    let json = write_file(&dir, "ok.json", &valid_notebook());
    let before = snapshot_dir(&dir);
    let output = europa().arg(&json).output().expect("run json");
    assert_eq!(output.status.code(), Some(74));
    assert_unchanged(&before, &dir);
    std::fs::remove_dir_all(&dir).ok();
}
