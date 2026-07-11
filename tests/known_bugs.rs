//! Regression tests for previously-known bugs — all fixed.
//!
//! Each test asserts the CORRECT behavior and is numbered for the bug it
//! guards against; the doc comments describe the original buggy behavior
//! and the shape of the fix. Should a new bug be documented here before its
//! fix lands, mark its test `#[ignore = "known bug N: ..."]` until then.

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

/// Bug 1 (fixed): `get_image_data` parsed HTML line by line (via
/// `to_string_array`), so an `<img>` tag whose attributes spanned multiple
/// lines of the nbformat string array was never found. The HTML document is
/// now joined and parsed as a whole.
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

/// Bug 2 (fixed): the output file name was built with `Path::with_extension`,
/// which truncated a label at its last dot: `# label: fig-v1.2` produced
/// `fig-v1.png` instead of `fig-v1.2.png` (and two labels `a.1`/`a.2` silently
/// collided on `a.png`).
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

/// Bug 3 (fixed): the `-N` de-duplication suffix was not checked against
/// names produced by multi-image numbering, so distinct images could be
/// assigned the same name and silently overwrite each other. Three cells
/// labelled `x` yielding 1 + 2 + 1 images must produce 4 files; the last
/// image used to be written as `x-2.png`, clobbering the second image of the
/// multi-image cell.
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

/// Bug 4 (fixed): an XML declaration was unconditionally prepended to SVG
/// output, so an SVG that already starts with `<?xml ...?>` (as produced by
/// e.g. matplotlib) was written with two declarations, which is invalid XML.
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

/// Bug 5 (fixed): per the nbformat schema, non-JSON mime data is a
/// `multiline_string` (string OR array of strings), so `image/png` stored as
/// an array of base64 lines is a legal notebook. The tool used to abort the
/// whole run with "Expected binary data." instead of joining the lines and
/// decoding.
#[test]
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

/// Bug 6 (fixed): a label containing `..` (or any path separator) escaped
/// the output directory: `# label: ../escaped` wrote `escaped.png` NEXT TO
/// the chosen output directory. Unsafe labels (anything but a single normal
/// path component) are now ignored with a warning, falling back to the
/// positional name, and a write-time guard rejects any output path that is
/// not a direct child of the output directory.
#[test]
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

/// Bug 7 (fixed): real Jupyter/nbformat writes base64 image data with a
/// trailing newline (`"iVBORw0K...\n"`), but the data was passed to a strict
/// base64 decoder untrimmed, so extracting from a genuine saved notebook
/// aborted the whole run with "Invalid symbol 10". Whitespace is now trimmed
/// before decoding.
#[test]
fn base64_with_trailing_newline_is_decoded() {
    let tmp = tempfile::tempdir().unwrap();
    let nb = write_notebook(tmp.path(), serde_json::json!([png_cell("pic", "QUJD\n")]));
    let out = tmp.path().join("out");
    bin().arg(&nb).arg("-o").arg(&out).assert().success();
    assert_eq!(fs::read(out.join("pic.png")).unwrap(), b"ABC");
}

/// Bug 8 (fixed): `image/svg+xml` holds SVG markup as text, and the
/// string-array branch treated it as such — but when the (equally
/// schema-legal) single string form was used, the SVG fell into the generic
/// branch and was fed to the base64 decoder, aborting the run with
/// "Invalid symbol 60" (`<`).
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

/// Bug 9 (fixed): an empty label — from a bare `# label:` comment or a cell
/// tag equal to the tag prefix — produced `output_path.join("")`, and
/// `with_extension` then replaced the output DIRECTORY's own name: the image
/// was written to `<output_dir>.png` next to the output directory instead of
/// inside it. An empty label now falls back to the positional `img-NN` name.
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

/// Bug 9 (fixed), tag variant: a cell tag exactly equal to the tag prefix
/// yielded an empty name from `get_image_candidate_tags` (`strip_prefix`
/// leaves ""), so the image was written to `<output_dir>.png` beside the
/// output directory even once the comment path rejected empty labels.
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

/// Bug 10 (fixed): same root cause as bug 3 (names generated by
/// `assign_image_name` were never registered as used) seen from the other
/// direction: an explicit label equal to a dedup-generated name collided with
/// it. Cells labelled `foo`, `foo`, `foo-2` must yield 3 files; the second
/// `foo` used to be renamed `foo-2` and then silently overwritten by the
/// third cell.
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

/// Bug 11 (fixed): HTML tag and attribute names are case-insensitive, but
/// the `img[src]` query and the `get("src")` attribute lookup only matched
/// lowercase, so `<IMG SRC="data:...">` (legal HTML) was silently skipped.
/// Tag and attribute names are now compared with `eq_ignore_ascii_case`.
#[test]
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

/// Bug 12 (fixed): the data URL grammar allows mime parameters between the
/// type and the base64 marker (`data:image/png;charset=utf-8;base64,...`),
/// but `parse_data_url` used to match the whole `image/png;charset=utf-8`
/// segment against the exact mime strings and silently skipped the image.
/// The mime part is now split on `;` and only the leading type is matched.
#[test]
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

/// Bug 13 (fixed): `label:` was located by substring search (`split_once`),
/// not anchored to the start of the comment, so unrelated comments like
/// `# xlabel: time (s)` or `# my label: x` hijacked the image name. The label
/// convention is modeled on Quarto's `#| label:` directive with the bar made
/// optional: a comment must BEGIN with `label:` (after `#` and an optional
/// `|`) to name the image; anything else falls back to the positional name.
/// The Quarto form itself (`#| label: good`) worked all along and must keep
/// working.
#[test]
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

/// Bug 14 (fixed): a `# label:` comment was honored on any line of the cell,
/// but (following the Quarto convention) only the cell's leading comment
/// block may name the image: labels are now taken from the lines before the
/// first non-comment line only.
#[test]
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
