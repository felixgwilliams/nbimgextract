//! Failing tests documenting known bugs.
//!
//! Each test asserts the CORRECT behavior and currently fails. Once a bug is
//! fixed, its test here starts passing and serves as a regression test.
//! Note: two existing tests codify the buggy behavior and must be removed
//! along with the fixes they contradict:
//! - `tests/cli.rs::string_array_binary_data_errors` (contradicts bug 5)
//! - `src/main.rs::tests::assign_name_dedup_suffix_can_collide_with_multi_image_suffix`
//!   (contradicts bug 3)
//! - `src/main.rs::tests::comment_label_matches_anywhere_in_comment`
//!   (contradicts bug 13)
//!
//! Depending on how bug 9 is fixed,
//! `src/main.rs::tests::tag_candidate_tag_equal_to_prefix_gives_empty_name`
//! (which documents that a tag equal to the prefix yields an empty name) may
//! also need updating. Likewise for bug 14,
//! `src/main.rs::tests::comment_label_multiple_lines_in_order` contradicts a
//! fix made inside `get_comment_label`, but survives one made in its caller.

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;

fn bin() -> Command {
    Command::cargo_bin("nbimgextract").unwrap()
}

/// Write a minimal notebook with the given cells to `dir` and return its path.
fn write_notebook(dir: &Path, cells: serde_json::Value) -> PathBuf {
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

fn png_cell(label: &str, b64: &str) -> serde_json::Value {
    serde_json::json!({
        "cell_type": "code",
        "metadata": {},
        "source": format!("# label: {label}\nplot()"),
        "outputs": [{
            "output_type": "display_data",
            "data": {"image/png": b64},
            "metadata": {}
        }],
        "execution_count": 1
    })
}

/// Bug 1: `get_image_data` parses HTML line by line (via `to_string_array`),
/// so an `<img>` tag whose attributes span multiple lines of the nbformat
/// string array is never found. The HTML document should be joined and parsed
/// as a whole.
#[test]
fn multiline_html_img_tag_is_extracted() {
    let tmp = tempfile::tempdir().unwrap();
    let nb = write_notebook(
        tmp.path(),
        serde_json::json!([{
            "cell_type": "code",
            "metadata": {},
            "source": "# label: pic\nplot()",
            "outputs": [{
                "output_type": "display_data",
                "data": {"text/html": [
                    "<img\n",
                    "src=\"data:image/png;base64,AAAA\"/>\n"
                ]},
                "metadata": {}
            }],
            "execution_count": 1
        }]),
    );
    let out = tmp.path().join("out");
    bin().arg(&nb).arg("-o").arg(&out).assert().success();
    assert!(
        out.join("pic.png").exists(),
        "img tag split across HTML lines was not extracted"
    );
}

/// Bug 2: the output file name is built with `Path::with_extension`, which
/// truncates a label at its last dot: `# label: fig-v1.2` produces
/// `fig-v1.png` instead of `fig-v1.2.png` (and two labels `a.1`/`a.2` silently
/// collide on `a.png`).
#[test]
fn dotted_label_keeps_full_name() {
    let tmp = tempfile::tempdir().unwrap();
    let nb = write_notebook(
        tmp.path(),
        serde_json::json!([png_cell("fig-v1.2", "AAAA")]),
    );
    let out = tmp.path().join("out");
    bin().arg(&nb).arg("-o").arg(&out).assert().success();
    assert!(
        out.join("fig-v1.2.png").exists(),
        "label containing a dot was truncated by with_extension"
    );
}

/// Bug 3: the `-N` de-duplication suffix is not checked against names produced
/// by multi-image numbering, so distinct images can be assigned the same name
/// and silently overwrite each other. Three cells labelled `x` yielding
/// 1 + 2 + 1 images must produce 4 files; currently the last image is written
/// as `x-2.png`, clobbering the second image of the multi-image cell.
#[test]
fn duplicate_names_never_overwrite() {
    let tmp = tempfile::tempdir().unwrap();
    let two_image_cell = serde_json::json!({
        "cell_type": "code",
        "metadata": {},
        "source": "# label: x\nplot()",
        "outputs": [
            {"output_type": "display_data", "data": {"image/png": "MjIyMg=="}, "metadata": {}},
            {"output_type": "display_data", "data": {"image/png": "MzMzMw=="}, "metadata": {}}
        ],
        "execution_count": 1
    });
    let nb = write_notebook(
        tmp.path(),
        serde_json::json!([
            png_cell("x", "MTExMQ=="),
            two_image_cell,
            png_cell("x", "NDQ0NA==")
        ]),
    );
    let out = tmp.path().join("out");
    bin().arg(&nb).arg("-o").arg(&out).assert().success();
    let n_files = fs::read_dir(&out).unwrap().count();
    assert_eq!(
        n_files, 4,
        "4 images went in, only {n_files} files came out"
    );
}

/// Bug 4: an XML declaration is unconditionally prepended to SVG output, so an
/// SVG that already starts with `<?xml ...?>` (as produced by e.g. matplotlib)
/// is written with two declarations, which is invalid XML.
#[test]
fn svg_with_existing_xml_declaration_not_duplicated() {
    let tmp = tempfile::tempdir().unwrap();
    let nb = write_notebook(
        tmp.path(),
        serde_json::json!([{
            "cell_type": "code",
            "metadata": {},
            "source": "# label: tri\nplot()",
            "outputs": [{
                "output_type": "display_data",
                "data": {"image/svg+xml": [
                    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
                    "<svg xmlns=\"http://www.w3.org/2000/svg\"/>\n"
                ]},
                "metadata": {}
            }],
            "execution_count": 1
        }]),
    );
    let out = tmp.path().join("out");
    bin().arg(&nb).arg("-o").arg(&out).assert().success();
    let svg = fs::read_to_string(out.join("tri.svg")).unwrap();
    assert_eq!(
        svg.matches("<?xml").count(),
        1,
        "XML declaration was duplicated"
    );
}

/// Bug 5: per the nbformat schema, non-JSON mime data is a `multiline_string`
/// (string OR array of strings), so `image/png` stored as an array of base64
/// lines is a legal notebook. The tool currently aborts the whole run with
/// "Expected binary data." instead of joining the lines and decoding.
#[test]
#[ignore = "known bug 5: schema-legal line-array binary data is rejected"]
fn png_stored_as_line_array_is_decoded() {
    let tmp = tempfile::tempdir().unwrap();
    let nb = write_notebook(
        tmp.path(),
        serde_json::json!([{
            "cell_type": "code",
            "metadata": {},
            "source": "# label: pic\nplot()",
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
    assert_eq!(fs::read(out.join("pic.png")).unwrap(), b"ABC");
}

/// Bug 6: a label containing `..` (or any path separator) escapes the output
/// directory: `# label: ../escaped` writes `escaped.png` NEXT TO the chosen
/// output directory. Labels from a (potentially untrusted) notebook must not
/// place files outside the output directory.
#[test]
#[ignore = "known bug 6: labels with .. escape the output directory"]
fn label_cannot_escape_output_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let nb = write_notebook(
        tmp.path(),
        serde_json::json!([png_cell("../escaped", "AAAA")]),
    );
    let out = tmp.path().join("out");
    // whether the tool errors or sanitizes the name, nothing may be written
    // outside the output directory
    let _ = bin().arg(&nb).arg("-o").arg(&out).assert();
    assert!(
        !tmp.path().join("escaped.png").exists(),
        "label with .. wrote a file outside the output directory"
    );
}

/// Bug 7: real Jupyter/nbformat writes base64 image data with a trailing
/// newline (`"iVBORw0K...\n"`), but the data is passed to a strict base64
/// decoder untrimmed, so extracting from a genuine saved notebook aborts the
/// whole run with "Invalid symbol 10". Whitespace should be trimmed (or a
/// forgiving decoder used) before decoding.
#[test]
#[ignore = "known bug 7: trailing newline in base64 data aborts the run"]
fn base64_with_trailing_newline_is_decoded() {
    let tmp = tempfile::tempdir().unwrap();
    let nb = write_notebook(tmp.path(), serde_json::json!([png_cell("pic", "QUJD\n")]));
    let out = tmp.path().join("out");
    bin().arg(&nb).arg("-o").arg(&out).assert().success();
    assert_eq!(fs::read(out.join("pic.png")).unwrap(), b"ABC");
}

/// Bug 8: `image/svg+xml` holds SVG markup as text, and the string-array
/// branch treats it as such — but when the (equally schema-legal) single
/// string form is used, the SVG falls into the generic branch and is fed to
/// the base64 decoder, aborting the run with "Invalid symbol 60" (`<`).
#[test]
fn svg_as_single_string_is_written_as_text() {
    let tmp = tempfile::tempdir().unwrap();
    let nb = write_notebook(
        tmp.path(),
        serde_json::json!([{
            "cell_type": "code",
            "metadata": {},
            "source": "# label: tri\nplot()",
            "outputs": [{
                "output_type": "display_data",
                "data": {"image/svg+xml": "<svg xmlns=\"http://www.w3.org/2000/svg\"/>"},
                "metadata": {}
            }],
            "execution_count": 1
        }]),
    );
    let out = tmp.path().join("out");
    bin().arg(&nb).arg("-o").arg(&out).assert().success();
    let svg = fs::read_to_string(out.join("tri.svg")).unwrap();
    assert!(svg.contains("<svg"), "SVG markup was not written as text");
}

/// Bug 9: an empty label — from a bare `# label:` comment or a cell tag equal
/// to the tag prefix — produces `output_path.join("")`, and `with_extension`
/// then replaces the output DIRECTORY's own name: the image is written to
/// `<output_dir>.png` next to the output directory instead of inside it.
/// An empty label should fall back to the positional `img-NN` name (or be
/// rejected), never resolve to the directory itself.
#[test]
fn empty_label_cannot_write_beside_output_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let nb = write_notebook(
        tmp.path(),
        serde_json::json!([{
            "cell_type": "code",
            "metadata": {},
            "source": "# label:\nplot()",
            "outputs": [{
                "output_type": "display_data",
                "data": {"image/png": "QUJD"},
                "metadata": {}
            }],
            "execution_count": 1
        }]),
    );
    let out = tmp.path().join("out");
    // whether the tool errors or falls back to a positional name, nothing may
    // be written outside the output directory
    let _ = bin().arg(&nb).arg("-o").arg(&out).assert();
    assert!(
        !tmp.path().join("out.png").exists(),
        "empty label wrote a file next to the output directory"
    );
}

/// Bug 9, tag variant: the comment path rejects empty labels, but a cell tag
/// exactly equal to the tag prefix still yields an empty name from
/// `get_image_candidate_tags` (`strip_prefix` leaves ""), so the image is
/// still written to `<output_dir>.png` beside the output directory. The fix
/// contradicts `src/main.rs::tests::tag_candidate_tag_equal_to_prefix_gives_empty_name`,
/// which must be updated to expect `None`.
#[test]
fn empty_label_from_tag_cannot_write_beside_output_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let nb = write_notebook(
        tmp.path(),
        serde_json::json!([{
            "cell_type": "code",
            "metadata": {"tags": ["img"]},
            "source": "plot()",
            "outputs": [{
                "output_type": "display_data",
                "data": {"image/png": "QUJD"},
                "metadata": {}
            }],
            "execution_count": 1
        }]),
    );
    let out = tmp.path().join("out");
    // whether the tool errors or falls back to a positional name, nothing may
    // be written outside the output directory
    let _ = bin().arg(&nb).arg("-o").arg(&out).assert();
    assert!(
        !tmp.path().join("out.png").exists(),
        "empty label from a tag wrote a file next to the output directory"
    );
}

/// Bug 10: same root cause as bug 3 (names generated by `assign_image_name`
/// are never registered as used) seen from the other direction: an explicit
/// label equal to a dedup-generated name collides with it. Cells labelled
/// `foo`, `foo`, `foo-2` must yield 3 files; currently the second `foo` is
/// renamed `foo-2` and then silently overwritten by the third cell.
#[test]
fn explicit_label_never_collides_with_dedup_name() {
    let tmp = tempfile::tempdir().unwrap();
    let nb = write_notebook(
        tmp.path(),
        serde_json::json!([
            png_cell("foo", "MTExMQ=="),
            png_cell("foo", "MjIyMg=="),
            png_cell("foo-2", "MzMzMw==")
        ]),
    );
    let out = tmp.path().join("out");
    bin().arg(&nb).arg("-o").arg(&out).assert().success();
    let n_files = fs::read_dir(&out).unwrap().count();
    assert_eq!(
        n_files, 3,
        "3 images went in, only {n_files} files came out"
    );
}

/// Bug 11: HTML tag and attribute names are case-insensitive, but the
/// `img[src]` query and the `get("src")` attribute lookup only match
/// lowercase, so `<IMG SRC="data:...">` (legal HTML) is silently skipped.
#[test]
#[ignore = "known bug 11: uppercase <IMG SRC=...> in HTML is not extracted"]
fn uppercase_html_img_tag_is_extracted() {
    let tmp = tempfile::tempdir().unwrap();
    let nb = write_notebook(
        tmp.path(),
        serde_json::json!([{
            "cell_type": "code",
            "metadata": {},
            "source": "# label: up\nplot()",
            "outputs": [{
                "output_type": "display_data",
                "data": {"text/html": "<IMG SRC=\"data:image/png;base64,QUJD\">"},
                "metadata": {}
            }],
            "execution_count": 1
        }]),
    );
    let out = tmp.path().join("out");
    bin().arg(&nb).arg("-o").arg(&out).assert().success();
    assert_eq!(
        fs::read(out.join("up.png")).unwrap(),
        b"ABC",
        "uppercase img tag was not extracted"
    );
}

/// Bug 12: the data URL grammar allows mime parameters between the type and
/// the base64 marker (`data:image/png;charset=utf-8;base64,...`), but
/// `parse_data_url` matches the whole `image/png;charset=utf-8` segment
/// against the exact mime strings and silently skips the image. The mime part
/// should be split on `;`, matching the leading type and looking for `base64`
/// among the parameters.
#[test]
#[ignore = "known bug 12: data URL with mime parameters is silently skipped"]
fn data_url_with_mime_parameters_is_extracted() {
    let tmp = tempfile::tempdir().unwrap();
    let nb = write_notebook(
        tmp.path(),
        serde_json::json!([{
            "cell_type": "code",
            "metadata": {},
            "source": "# label: chset\nplot()",
            "outputs": [{
                "output_type": "display_data",
                "data": {"text/html": "<img src=\"data:image/png;charset=utf-8;base64,QUJD\">"},
                "metadata": {}
            }],
            "execution_count": 1
        }]),
    );
    let out = tmp.path().join("out");
    bin().arg(&nb).arg("-o").arg(&out).assert().success();
    assert_eq!(
        fs::read(out.join("chset.png")).unwrap(),
        b"ABC",
        "data URL with a charset parameter was not extracted"
    );
}

/// A code cell with the given source and a single PNG output.
fn png_cell_src(source: &str) -> serde_json::Value {
    serde_json::json!({
        "cell_type": "code",
        "metadata": {},
        "source": source,
        "outputs": [{
            "output_type": "display_data",
            "data": {"image/png": "QUJD"},
            "metadata": {}
        }],
        "execution_count": 1
    })
}

/// Bug 13: `label:` is located by substring search (`split_once`), not
/// anchored to the start of the comment, so unrelated comments like
/// `# xlabel: time (s)` or `# my label: x` hijack the image name. The label
/// convention is modeled on Quarto's `#| label:` directive with the bar made
/// optional: a comment must BEGIN with `label:` (after `#` and an optional
/// `|`) to name the image; anything else falls back to the positional name.
/// The Quarto form itself (`#| label: good`) already works and must keep
/// working after the fix.
#[test]
#[ignore = "known bug 13: unanchored label match lets '# xlabel:' set the name"]
fn label_must_start_the_comment() {
    let tmp = tempfile::tempdir().unwrap();
    let nb = write_notebook(
        tmp.path(),
        serde_json::json!([
            png_cell_src("# xlabel: time (s)\nplot()"),
            png_cell_src("# my label: x\nplot()"),
            png_cell_src("#| label: good\nplot()")
        ]),
    );
    let out = tmp.path().join("out");
    bin().arg(&nb).arg("-o").arg(&out).assert().success();
    assert!(
        out.join("img-1.png").exists(),
        "'# xlabel:' hijacked the image name"
    );
    assert!(
        out.join("img-2.png").exists(),
        "'# my label:' hijacked the image name"
    );
    assert!(
        out.join("good.png").exists(),
        "Quarto-style '#| label:' must keep working"
    );
}

/// Bug 14: a `# label:` comment is honored on any line of the cell, but
/// (following the Quarto convention) only the cell's leading comment block
/// should be scanned: a label comment after the first code line must not
/// name the image.
#[test]
#[ignore = "known bug 14: label comments after code lines are honored"]
fn label_only_in_leading_comment_block() {
    let tmp = tempfile::tempdir().unwrap();
    let nb = write_notebook(
        tmp.path(),
        serde_json::json!([png_cell_src("plot()\n# label: late")]),
    );
    let out = tmp.path().join("out");
    bin().arg(&nb).arg("-o").arg(&out).assert().success();
    assert!(
        out.join("img-1.png").exists(),
        "a label comment below the first code line named the image"
    );
}
