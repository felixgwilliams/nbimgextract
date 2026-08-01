#![cfg_attr(test, allow(clippy::expect_used, clippy::unwrap_used, clippy::panic))]

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use predicates::prelude::*;

// Expected output of a default run on example.ipynb. The notebook has 13
// cells, so unlabelled images fall back to 2-digit `img-NN` names numbered by
// 1-based cell position (the gif cell is 2nd, the webp cell 4th). Cell 11
// repeats cell 9's `# label: sine-wave`, so its image is renamed sine-wave-2.
const EXPECTED_FILES: [&str; 7] = [
    "img-02.gif",
    "png-shapes.png",
    "img-04.webp",
    "my-png.jpg",
    "triangle.svg",
    "sine-wave.png",
    "sine-wave-2.png",
];

fn bin() -> Command {
    Command::cargo_bin("nbimgextract").unwrap()
}

fn example_nb() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("example.ipynb")
}

fn dir_file_names(dir: &Path) -> BTreeSet<String> {
    fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect()
}

fn expected_set() -> BTreeSet<String> {
    EXPECTED_FILES.iter().map(|s| (*s).to_owned()).collect()
}

#[test]
fn default_run_extracts_expected_images() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out");
    bin()
        .arg(example_nb())
        .arg("-o")
        .arg(&out)
        .assert()
        .success();
    assert_eq!(dir_file_names(&out), expected_set());

    // gif data is written byte-identical to the original image
    let gif = fs::read(out.join("img-02.gif")).unwrap();
    let original =
        fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("test-imgs/Example_gif.gif")).unwrap();
    assert_eq!(gif, original);

    // svg is written as text with an XML declaration prepended
    let svg = fs::read_to_string(out.join("triangle.svg")).unwrap();
    assert!(svg.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n"));
    assert!(svg.contains("<svg"));

    // the image extracted from HTML is a valid PNG
    let png = fs::read(out.join("sine-wave-2.png")).unwrap();
    assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
}

#[test]
fn default_output_path_next_to_input() {
    let tmp = tempfile::tempdir().unwrap();
    let nb = tmp.path().join("example.ipynb");
    fs::copy(example_nb(), &nb).unwrap();
    bin().arg(&nb).assert().success();
    assert_eq!(
        dir_file_names(&tmp.path().join("example_images")),
        expected_set()
    );
}

#[test]
fn dry_run_writes_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out");
    bin()
        .arg(example_nb())
        .arg("--dry-run")
        .arg("-o")
        .arg(&out)
        .assert()
        .success()
        .stdout(predicate::str::contains("Would write to"))
        .stdout(predicate::str::contains("sine-wave-2"));
    assert!(!out.exists());
}

#[test]
fn quiet_suppresses_output() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out");
    bin()
        .arg(example_nb())
        .arg("-q")
        .arg("-o")
        .arg(&out)
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
    assert_eq!(dir_file_names(&out), expected_set());
}

#[test]
fn prints_written_files_by_default() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out");
    bin()
        .arg(example_nb())
        .arg("-o")
        .arg(&out)
        .assert()
        .success()
        .stdout(predicate::str::contains("Writing to"));
}

#[test]
fn non_empty_dir_errors_by_default() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out");
    fs::create_dir(&out).unwrap();
    fs::write(out.join("dummy.txt"), "x").unwrap();
    bin()
        .arg(example_nb())
        .arg("-o")
        .arg(&out)
        .assert()
        .failure()
        .stderr(predicate::str::contains("Target folder is not empty"));
    assert_eq!(
        dir_file_names(&out),
        BTreeSet::from(["dummy.txt".to_owned()])
    );
}

#[test]
fn non_empty_dir_proceed_keeps_existing_files() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out");
    fs::create_dir(&out).unwrap();
    fs::write(out.join("dummy.txt"), "x").unwrap();
    bin()
        .arg(example_nb())
        .arg("--proceed")
        .arg("-o")
        .arg(&out)
        .assert()
        .success();
    let mut expected = expected_set();
    expected.insert("dummy.txt".to_owned());
    assert_eq!(dir_file_names(&out), expected);
}

#[test]
fn non_empty_dir_clear_removes_existing_files() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out");
    fs::create_dir(&out).unwrap();
    fs::write(out.join("dummy.txt"), "x").unwrap();
    bin()
        .arg(example_nb())
        .arg("--clear-dir")
        .arg("-o")
        .arg(&out)
        .assert()
        .success()
        .stderr(predicate::str::contains("Deleting files in directory"));
    assert_eq!(dir_file_names(&out), expected_set());
}

#[test]
fn custom_tag_prefix_changes_tag_matching() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out");
    bin()
        .arg(example_nb())
        .arg("-t")
        .arg("fig")
        .arg("-o")
        .arg(&out)
        .assert()
        .success();
    // with prefix "fig" the `img-my-png` tag no longer matches, so the jpeg
    // cell (5th) falls back to its positional name
    let mut expected = expected_set();
    expected.remove("my-png.jpg");
    expected.insert("img-05.jpg".to_owned());
    assert_eq!(dir_file_names(&out), expected);
}

#[test]
fn conflicting_dir_action_flags_rejected() {
    bin()
        .arg(example_nb())
        .arg("--proceed")
        .arg("--clear-dir")
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot be used with"));
}

/// Write a minimal notebook with the given cells to `dir` and return its path.
fn write_notebook(dir: &Path, cells: &serde_json::Value) -> PathBuf {
    let nb = serde_json::json!({
        "cells": cells,
        "metadata": {},
        "nbformat": 4,
        "nbformat_minor": 5
    });
    let path = dir.join("crafted.ipynb");
    fs::write(&path, nb.to_string()).unwrap();
    path
}

#[test]
fn notebook_without_images_creates_no_output_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let nb = write_notebook(
        tmp.path(),
        &serde_json::json!([
            {"cell_type": "markdown", "metadata": {}, "source": "# heading"},
            {
                "cell_type": "code",
                "metadata": {},
                "source": "print('hi')",
                "outputs": [{"output_type": "stream", "name": "stdout", "text": ["hi\n"]}],
                "execution_count": 1
            }
        ]),
    );
    let out = tmp.path().join("out");
    bin()
        .arg(&nb)
        .arg("-o")
        .arg(&out)
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
    assert!(!out.exists());
}

#[test]
fn string_array_binary_data_is_ok() {
    let tmp = tempfile::tempdir().unwrap();
    // per nbformat, non-JSON mime data may be a single string or an array of
    // lines; base64 lines are joined before decoding
    let nb = write_notebook(
        tmp.path(),
        &serde_json::json!([{
            "cell_type": "code",
            "metadata": {},
            "source": "plot()",
            "outputs": [{
                "output_type": "display_data",
                "data": {"image/png": ["QUJD"]},
                "metadata": {}
            }],
            "execution_count": 1
        }]),
    );
    let out = tmp.path().join("out");
    bin().arg(&nb).arg("-o").arg(&out).assert().success();
    assert_eq!(fs::read(out.join("img-1.png")).unwrap(), b"ABC");
}

#[test]
fn invalid_base64_errors() {
    let tmp = tempfile::tempdir().unwrap();
    let nb = write_notebook(
        tmp.path(),
        &serde_json::json!([{
            "cell_type": "code",
            "metadata": {},
            "source": "plot()",
            "outputs": [{
                "output_type": "display_data",
                "data": {"image/png": "not!!valid@@base64"},
                "metadata": {}
            }],
            "execution_count": 1
        }]),
    );
    bin()
        .arg(&nb)
        .arg("-o")
        .arg(tmp.path().join("out"))
        .assert()
        .failure();
}

#[test]
fn json_valued_mime_data_skipped_with_warning() {
    let tmp = tempfile::tempdir().unwrap();
    // schema-invalid: non-JSON mimes must hold strings, not JSON values
    let nb = write_notebook(
        tmp.path(),
        &serde_json::json!([{
            "cell_type": "code",
            "metadata": {},
            "source": "plot()",
            "outputs": [{
                "output_type": "display_data",
                "data": {"text/html": 3, "image/png": {"unexpected": true}},
                "metadata": {}
            }],
            "execution_count": 1
        }]),
    );
    let out = tmp.path().join("out");
    bin()
        .arg(&nb)
        .arg("-o")
        .arg(&out)
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "Warning: skipping text/html output with unexpected JSON data",
        ))
        .stderr(predicate::str::contains(
            "Warning: skipping image/png output with unexpected JSON data",
        ));
    assert!(!out.exists());
}

#[test]
fn nonexistent_input_fails() {
    let tmp = tempfile::tempdir().unwrap();
    bin()
        .arg(tmp.path().join("missing.ipynb"))
        .assert()
        .failure();
}
