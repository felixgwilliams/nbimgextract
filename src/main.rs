mod cli;
mod schema;
use crate::{
    cli::NonEmptyDirAction,
    schema::{CodeCell, MimeBundle, RawNotebook, SourceValue},
};
use anyhow::{anyhow, bail};
use base64::prelude::*;
use clap::Parser;
use colored::Colorize;
use serde_json::Value;
use std::{
    collections::HashMap,
    fs::{create_dir_all, remove_dir_all, File},
    io::{BufReader, BufWriter, Write},
    path::Path,
};
static TO_TRIM: &[char] = &['-', ' ', '_'];
#[derive(Debug, Clone)]
struct ToWrite<'a> {
    image_type: ImageType,
    image_json_data: SourceValueWrap<'a>,
    name: String,
}
fn main() -> anyhow::Result<()> {
    let cli = cli::Cli::parse();
    let file = File::open(&cli.file)?;
    let nb: RawNotebook = serde_json::from_reader(BufReader::new(file))?;
    let tag_prefix = cli
        .tag_prefix
        .as_deref()
        .unwrap_or("img")
        .trim_end_matches(TO_TRIM)
        .to_owned();
    let output_path = match cli.output_path {
        Some(ref output_path) => output_path.clone(),
        None => {
            let file_stem = cli
                .file
                .file_stem()
                .ok_or_else(|| anyhow!("Input filename is empty"))?
                .to_str()
                .ok_or_else(|| anyhow!("Bad file name"))?; // can't see how to manipulate OsStr themselves
            let Some(parent) = cli.file.parent() else {
                bail!("Invalid output path".red());
            };
            parent.join(file_stem.to_owned() + "_images")
        }
    };
    let n_cells = nb.cells.len();
    // https://stackoverflow.com/a/69298721
    let n_digits = n_cells.checked_ilog10().unwrap_or(0) + 1;
    let mut to_write = Vec::new();
    let mut used_names: HashMap<String, usize> = HashMap::new();

    // let mut cell_images: Vec::new();
    for (i, cell) in nb
        .cells
        .iter()
        .enumerate()
        .filter_map(|(j, cc)| cc.get_code_cell().map(|c| (j, c)))
    {
        let image_name = get_image_candidate(cell, &tag_prefix)
            .unwrap_or_else(|| format!("img-{:0width$}", i + 1, width = n_digits as usize));

        let cell_images: Vec<_> = cell
            .get_output_data()
            .iter()
            .flat_map(|mb| get_image_data(mb))
            .collect();
        let n_cell_images = cell_images.len();
        to_write.extend(cell_images.into_iter().enumerate().map(
            |(j, (image_type, image_json_data))| ToWrite {
                image_type,
                image_json_data,
                name: assign_image_name(&image_name, j, n_cell_images, &mut used_names),
            },
        ));
    }
    if !to_write.is_empty() {
        checked_create_dir(&output_path, cli.non_empty_action.get_action(), cli.dry_run)?;
    }
    for item in to_write {
        let file_name = output_path
            .join(item.name)
            .with_extension(item.image_type.get_extension());
        if !cli.quiet {
            make_write_message(&cli, &file_name);
        }
        if cli.dry_run {
            continue;
        }
        match item.image_json_data.to_ref() {
            SourceValueRef::String(b64_data) => {
                let image_bytes = BASE64_STANDARD.decode(b64_data)?;
                BufWriter::new(File::create(file_name)?).write_all(&image_bytes)?;
            }
            SourceValueRef::StringArray(arr) => {
                if item.image_type != ImageType::Svg {
                    bail!("Expected binary data.".red())
                }
                let svg_data = String::from_iter(arr.iter().map(|e| e.as_str()));
                let mut buf = BufWriter::new(File::create(file_name)?);
                buf.write_all(b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n")?;
                buf.write_all(svg_data.as_bytes())?;
            }
            // just ignore the json type data
            SourceValueRef::JsonData(_) => {}
        }
    }

    Ok(())
}
/// Determine the final file stem for an image, numbering images within a
/// multi-image cell and de-duplicating names already used by earlier cells.
fn assign_image_name(
    image_name: &str,
    image_index: usize,
    n_cell_images: usize,
    used_names: &mut HashMap<String, usize>,
) -> String {
    let name_stem = {
        if n_cell_images > 1 {
            let n_img_digits = n_cell_images.checked_ilog10().unwrap_or(0) + 1;
            format!(
                "{}-{:0width$}",
                image_name,
                image_index + 1,
                width = n_img_digits as usize
            )
        } else {
            image_name.to_owned()
        }
    };
    let name_count = used_names
        .entry(name_stem.clone())
        .and_modify(|e| *e += 1)
        .or_insert(1);
    if *name_count <= 1 {
        name_stem
    } else {
        let n_dups_digits = name_count.checked_ilog10().unwrap_or(0) + 1;
        format!(
            "{}-{:0width$}",
            name_stem,
            *name_count,
            width = n_dups_digits as usize
        )
    }
}

fn make_write_message(cli: &cli::Cli, file_name: &Path) {
    if cli.dry_run {
        println!("Would write to {}", file_name.display());
    } else {
        println!("Writing to {}", file_name.display());
    }
}
static LABEL: &str = "label:";

pub fn get_comment_label(source: &str) -> Vec<&str> {
    let mut comments = Vec::new();
    for line in source.lines() {
        let trim_line = line.trim();
        if !trim_line.starts_with('#') {
            continue;
        }
        if let Some((_, identifier)) = trim_line.split_once(LABEL) {
            comments.push(identifier.trim())
        }
    }

    comments
}

fn get_image_candidate(cell: &CodeCell, tag_prefix: &str) -> Option<String> {
    get_image_candidate_comment(cell)
        .or_else(|| get_image_candidate_tags(&cell.metadata.tags, tag_prefix))
}
fn get_image_candidate_comment(cell: &CodeCell) -> Option<String> {
    if let Some(sa) = cell.source.to_string_array() {
        let label = sa.iter().flat_map(|&s| get_comment_label(s)).next();
        label.map(|s| s.to_string())
    } else {
        None
    }
}
fn get_image_candidate_tags<S: AsRef<str>>(
    tags: &Option<Vec<S>>,
    tag_prefix: &str,
) -> Option<String> {
    if let Some(tags) = tags {
        let candidates: Vec<_> = tags
            .iter()
            .filter_map(|t| t.as_ref().strip_prefix(tag_prefix))
            .map(|s| s.trim_start_matches(TO_TRIM))
            .collect();
        if candidates.len() > 1 {
            eprintln!("Warning: Multiple tag candidates: {candidates:?}")
        }
        candidates.first().map(|s| (*s).to_owned())
    } else {
        None
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ImageType {
    Gif,
    Jpg,
    Png,
    Webp,
    Svg,
}
impl ImageType {
    fn get_extension(self) -> &'static str {
        use ImageType::*;
        match self {
            Gif => "gif",
            Jpg => "jpg",
            Png => "png",
            Webp => "webp",
            Svg => "svg",
        }
    }
}
fn get_image_type(mime: &str) -> Option<ImageType> {
    use ImageType::*;
    match mime {
        "image/png" => Some(Png),
        "image/jpeg" => Some(Jpg),
        "image/gif" => Some(Gif),
        "image/webp" => Some(Webp),
        "image/svg+xml" => Some(Svg),
        _ => None,
    }
}

fn parse_data_url(data_url: &str) -> Option<(ImageType, &str)> {
    let strip = data_url.strip_prefix("data:")?;
    let (mime_str_base64, body_str) = strip.split_once(',')?;
    let mime_str = mime_str_base64.strip_suffix(";base64")?;
    let image_type = get_image_type(mime_str)?;
    Some((image_type, body_str))
}

#[derive(Debug, Clone, Copy)]
enum SourceValueRef<'a> {
    String(&'a str),
    StringArray(&'a [String]),
    #[allow(dead_code)]
    JsonData(&'a Value),
}

// This is a way to make an object which references the content of a SourceValue. A reference to
// a thing which is either a string or array is converted to a thing which is either a reference to
// a string or a reference to an array. This is kind of like as_deref for Options in a way
impl SourceValue {
    fn to_ref<'a>(&'a self) -> SourceValueRef<'a> {
        match self {
            Self::JsonData(jd) => SourceValueRef::JsonData(jd),
            Self::String(s) => SourceValueRef::String(s),
            Self::StringArray(sa) => SourceValueRef::StringArray(sa),
        }
    }
}

// when extracting an image from a cell, the image data can either be contained in a String,
// a StringArray or embedded in some HTML.  If it is in a String or StringArray, then we can return
// a reference, which has the lifetime of the original cell data ('a). If it's in HTML, I rely on
// the tl parser, which can return a string slice, but the slice has the lifetime of the parser and
// not the original string of HTML. Therefore, I have no choice but to clone the string. This is why
// it can either be owned or borrowed. I don't want to clone by default because mostly we don't need
// to, so I have to do this.
#[derive(Debug, Clone)]
enum SourceValueWrap<'a> {
    Owned(SourceValue),
    Borrowed(SourceValueRef<'a>),
}
// I can't use a cow because I can't borrow SourceValue to SourceRef because the signature does not
// allow the lifetimes I need
impl<'a> SourceValueWrap<'a> {
    fn to_ref(&'a self) -> SourceValueRef<'a> {
        match self {
            Self::Borrowed(svr) => *svr,
            Self::Owned(sv) => sv.to_ref(),
        }
    }
}
fn get_image_data<'a>(data: &'a MimeBundle) -> Vec<(ImageType, SourceValueWrap<'a>)> {
    let mut out = Vec::new();
    for (mime, val) in data {
        if mime == "text/html" {
            let sa = val
                .to_string_array()
                .ok_or_else(|| anyhow::format_err!("Should be a string array"))
                .unwrap();
            for line in sa {
                let Ok(frag2) = tl::parse(line, tl::ParserOptions::default()) else {
                    continue;
                };
                let parser = frag2.parser();
                let Some(img) = frag2.query_selector("img[src]") else {
                    continue;
                };
                let img_iter = img
                    .flat_map(|x| x.get(parser).and_then(|x| x.as_tag()))
                    .flat_map(|x| x.attributes().get("src"))
                    .flatten()
                    .flat_map(|x| x.try_as_utf8_str())
                    .flat_map(parse_data_url)
                    .map(|(img_type, s)| {
                        (
                            img_type,
                            SourceValueWrap::Owned(SourceValue::String(s.to_string())),
                        )
                    });
                out.extend(img_iter);
            }
            // let cell_doc = Html::parse_document(val)
        } else if let Some(image_type) = get_image_type(mime) {
            // don't push the json data here so we don't have to process it later
            match val {
                SourceValue::String(_) | SourceValue::StringArray(_) => {
                    out.push((image_type, SourceValueWrap::Borrowed(val.to_ref())))
                }

                _ => {}
            };
        }
    }
    out
}
fn checked_create_dir<P: AsRef<Path>>(
    path: P,
    exist_action: NonEmptyDirAction,
    dry_run: bool,
) -> anyhow::Result<()> {
    use NonEmptyDirAction::*;
    let path = path.as_ref();
    if !path.exists() || exist_action == Proceed {
        if !dry_run {
            create_dir_all(path)?;
        }

        return Ok(());
    }
    if path.read_dir()?.next().is_none() {
        Ok(())
    } else if exist_action == Error {
        bail!("Target folder is not empty!".red());
    } else {
        if dry_run {
            eprintln!("Would delete files in directory");
        } else {
            eprintln!("Deleting files in directory");
        }
        if !dry_run {
            remove_dir_all(path)?;
            create_dir_all(path)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn code_cell(v: Value) -> CodeCell {
        serde_json::from_value(v).unwrap()
    }

    fn mime_bundle(v: Value) -> MimeBundle {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn comment_label_basic() {
        assert_eq!(get_comment_label("# label: foo"), vec!["foo"]);
    }

    #[test]
    fn comment_label_trims_whitespace() {
        assert_eq!(
            get_comment_label("   #  label:   my name  "),
            vec!["my name"]
        );
    }

    #[test]
    fn comment_label_requires_comment_line() {
        assert_eq!(get_comment_label("x = 'label: nope'"), Vec::<&str>::new());
    }

    #[test]
    fn comment_label_multiple_lines_in_order() {
        assert_eq!(
            get_comment_label("# label: first\ny = 1\n# label: second"),
            vec!["first", "second"]
        );
    }

    #[test]
    fn comment_label_empty_label() {
        assert_eq!(get_comment_label("# label:"), vec![""]);
    }

    #[test]
    fn comment_label_no_spaces() {
        assert_eq!(get_comment_label("#label:foo"), vec!["foo"]);
    }

    #[test]
    fn comment_label_none_present() {
        assert_eq!(get_comment_label("x = 1\n# a comment"), Vec::<&str>::new());
    }

    #[test]
    fn comment_label_matches_anywhere_in_comment() {
        // `label:` does not need to start the comment
        assert_eq!(get_comment_label("# my label: x"), vec!["x"]);
    }

    #[test]
    fn tag_candidate_strips_prefix_and_trim_chars() {
        let tags = Some(vec!["img-my-png"]);
        assert_eq!(
            get_image_candidate_tags(&tags, "img"),
            Some("my-png".to_owned())
        );
        let tags = Some(vec!["img_ -name"]);
        assert_eq!(
            get_image_candidate_tags(&tags, "img"),
            Some("name".to_owned())
        );
        let tags = Some(vec!["img name"]);
        assert_eq!(
            get_image_candidate_tags(&tags, "img"),
            Some("name".to_owned())
        );
    }

    #[test]
    fn tag_candidate_first_of_multiple_wins() {
        let tags = Some(vec!["img-a", "img-b"]);
        assert_eq!(get_image_candidate_tags(&tags, "img"), Some("a".to_owned()));
    }

    #[test]
    fn tag_candidate_no_match() {
        let tags = Some(vec!["hello", "world"]);
        assert_eq!(get_image_candidate_tags(&tags, "img"), None);
        assert_eq!(get_image_candidate_tags(&None::<Vec<String>>, "img"), None);
    }

    #[test]
    fn tag_candidate_tag_equal_to_prefix_gives_empty_name() {
        // degenerate edge: documents current behavior
        let tags = Some(vec!["img"]);
        assert_eq!(get_image_candidate_tags(&tags, "img"), Some(String::new()));
    }

    #[test]
    fn tag_candidate_empty_prefix_matches_everything() {
        let tags = Some(vec!["-first", "second"]);
        assert_eq!(
            get_image_candidate_tags(&tags, ""),
            Some("first".to_owned())
        );
    }

    #[test]
    fn image_candidate_comment_wins_over_tag() {
        let cell = code_cell(json!({
            "metadata": {"tags": ["img-fromtag"]},
            "outputs": [],
            "source": "# label: fromcomment\nplot()"
        }));
        assert_eq!(
            get_image_candidate(&cell, "img"),
            Some("fromcomment".to_owned())
        );
    }

    #[test]
    fn image_candidate_falls_back_to_tag() {
        let cell = code_cell(json!({
            "metadata": {"tags": ["img-fromtag"]},
            "outputs": [],
            "source": "plot()"
        }));
        assert_eq!(
            get_image_candidate(&cell, "img"),
            Some("fromtag".to_owned())
        );
    }

    #[test]
    fn image_candidate_json_source_falls_through_to_tag() {
        let cell = code_cell(json!({
            "metadata": {"tags": ["img-fromtag"]},
            "outputs": [],
            "source": 3
        }));
        assert_eq!(get_image_candidate_comment(&cell), None);
        assert_eq!(
            get_image_candidate(&cell, "img"),
            Some("fromtag".to_owned())
        );
    }

    #[test]
    fn image_candidate_none() {
        let cell = code_cell(json!({
            "metadata": {},
            "outputs": [],
            "source": "plot()"
        }));
        assert_eq!(get_image_candidate(&cell, "img"), None);
    }

    #[test]
    fn image_type_from_mime() {
        use ImageType::*;
        assert_eq!(get_image_type("image/png"), Some(Png));
        assert_eq!(get_image_type("image/jpeg"), Some(Jpg));
        assert_eq!(get_image_type("image/gif"), Some(Gif));
        assert_eq!(get_image_type("image/webp"), Some(Webp));
        assert_eq!(get_image_type("image/svg+xml"), Some(Svg));
        assert_eq!(get_image_type("text/plain"), None);
        assert_eq!(get_image_type("image/bmp"), None);
        assert_eq!(get_image_type(""), None);
    }

    #[test]
    fn image_type_extensions() {
        use ImageType::*;
        assert_eq!(Gif.get_extension(), "gif");
        assert_eq!(Jpg.get_extension(), "jpg");
        assert_eq!(Png.get_extension(), "png");
        assert_eq!(Webp.get_extension(), "webp");
        assert_eq!(Svg.get_extension(), "svg");
    }

    #[test]
    fn data_url_valid() {
        assert_eq!(
            parse_data_url("data:image/png;base64,AAAA"),
            Some((ImageType::Png, "AAAA"))
        );
    }

    #[test]
    fn data_url_invalid_forms() {
        // missing data: prefix
        assert_eq!(parse_data_url("image/png;base64,AAAA"), None);
        // no comma
        assert_eq!(parse_data_url("data:image/png;base64"), None);
        // no base64 marker
        assert_eq!(parse_data_url("data:image/png,AAAA"), None);
        // unknown mime
        assert_eq!(parse_data_url("data:image/tiff;base64,AAAA"), None);
    }

    #[test]
    fn data_url_body_may_contain_commas() {
        assert_eq!(
            parse_data_url("data:image/gif;base64,ab,cd"),
            Some((ImageType::Gif, "ab,cd"))
        );
    }

    #[test]
    fn image_data_direct_string() {
        let bundle = mime_bundle(json!({"image/png": "b64data", "text/plain": "repr"}));
        let out = get_image_data(&bundle);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, ImageType::Png);
        match out[0].1.to_ref() {
            SourceValueRef::String(s) => assert_eq!(s, "b64data"),
            other => panic!("expected borrowed string, got {other:?}"),
        }
    }

    #[test]
    fn image_data_svg_string_array() {
        let bundle = mime_bundle(json!({"image/svg+xml": ["<svg>", "</svg>"]}));
        let out = get_image_data(&bundle);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, ImageType::Svg);
        match out[0].1.to_ref() {
            SourceValueRef::StringArray(sa) => assert_eq!(sa, ["<svg>", "</svg>"]),
            other => panic!("expected string array, got {other:?}"),
        }
    }

    #[test]
    fn image_data_ignores_non_image_mimes() {
        let bundle = mime_bundle(json!({"text/plain": "hi"}));
        assert!(get_image_data(&bundle).is_empty());
        let bundle = mime_bundle(json!({
            "application/vnd.jupyter.widget-view+json": {"model_id": "abc", "version_major": 2}
        }));
        assert!(get_image_data(&bundle).is_empty());
    }

    #[test]
    fn image_data_ignores_json_valued_image_mime() {
        let bundle = mime_bundle(json!({"image/png": {"unexpected": true}}));
        assert!(get_image_data(&bundle).is_empty());
    }

    #[test]
    fn image_data_extracts_data_url_from_html() {
        let bundle = mime_bundle(json!({
            "text/html": ["<div><img src=\"data:image/png;base64,AAAA\"></div>"]
        }));
        let out = get_image_data(&bundle);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, ImageType::Png);
        match &out[0].1 {
            SourceValueWrap::Owned(SourceValue::String(s)) => assert_eq!(s, "AAAA"),
            other => panic!("expected owned string, got {other:?}"),
        }
    }

    #[test]
    fn image_data_html_without_data_url_images() {
        let bundle = mime_bundle(json!({"text/html": ["<p>no images here</p>"]}));
        assert!(get_image_data(&bundle).is_empty());
        let bundle = mime_bundle(json!({
            "text/html": ["<img src=\"https://example.com/a.png\">"]
        }));
        assert!(get_image_data(&bundle).is_empty());
    }

    #[test]
    fn image_data_multiple_image_mimes() {
        // MimeBundle is a HashMap, so iteration order is nondeterministic:
        // assert order-insensitively
        let bundle = mime_bundle(json!({"image/png": "a", "image/gif": "b"}));
        let mut types: Vec<_> = get_image_data(&bundle)
            .iter()
            .map(|(t, _)| t.get_extension())
            .collect();
        types.sort_unstable();
        assert_eq!(types, ["gif", "png"]);
    }

    #[test]
    #[should_panic(expected = "Should be a string array")]
    fn image_data_panics_on_json_valued_html() {
        // documents current behavior: text/html with non-string data panics
        let bundle = mime_bundle(json!({"text/html": 3}));
        get_image_data(&bundle);
    }

    #[test]
    fn assign_name_dedups_repeats() {
        let mut used = HashMap::new();
        assert_eq!(assign_image_name("foo", 0, 1, &mut used), "foo");
        assert_eq!(assign_image_name("foo", 0, 1, &mut used), "foo-2");
        assert_eq!(assign_image_name("foo", 0, 1, &mut used), "foo-3");
    }

    #[test]
    fn assign_name_numbers_multi_image_cells() {
        let mut used = HashMap::new();
        let names: Vec<_> = (0..3)
            .map(|j| assign_image_name("foo", j, 3, &mut used))
            .collect();
        assert_eq!(names, ["foo-1", "foo-2", "foo-3"]);
    }

    #[test]
    fn assign_name_pads_multi_image_numbers() {
        let mut used = HashMap::new();
        let names: Vec<_> = (0..10)
            .map(|j| assign_image_name("foo", j, 10, &mut used))
            .collect();
        assert_eq!(names[0], "foo-01");
        assert_eq!(names[9], "foo-10");
    }

    #[test]
    fn assign_name_fixture_duplicate_case() {
        let mut used = HashMap::new();
        assert_eq!(assign_image_name("sine-wave", 0, 1, &mut used), "sine-wave");
        assert_eq!(
            assign_image_name("sine-wave", 0, 1, &mut used),
            "sine-wave-2"
        );
    }

    #[test]
    fn assign_name_dedup_suffix_can_collide_with_multi_image_suffix() {
        // documents current behavior: the `-2` dedup suffix is not checked
        // against names produced by multi-image numbering
        let mut used = HashMap::new();
        assert_eq!(assign_image_name("x", 0, 1, &mut used), "x");
        assert_eq!(assign_image_name("x", 0, 2, &mut used), "x-1");
        assert_eq!(assign_image_name("x", 1, 2, &mut used), "x-2");
        assert_eq!(assign_image_name("x", 0, 1, &mut used), "x-2");
    }

    #[test]
    fn create_dir_missing_path() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("out");
        checked_create_dir(&target, NonEmptyDirAction::Error, false).unwrap();
        assert!(target.is_dir());
    }

    #[test]
    fn create_dir_missing_path_dry_run() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("out");
        checked_create_dir(&target, NonEmptyDirAction::Error, true).unwrap();
        assert!(!target.exists());
    }

    #[test]
    fn create_dir_existing_empty_ok() {
        let tmp = tempfile::tempdir().unwrap();
        checked_create_dir(tmp.path(), NonEmptyDirAction::Error, false).unwrap();
    }

    #[test]
    fn create_dir_non_empty_errors() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("dummy.txt"), "x").unwrap();
        let err = checked_create_dir(tmp.path(), NonEmptyDirAction::Error, false).unwrap_err();
        assert!(err.to_string().contains("Target folder is not empty"));
    }

    #[test]
    fn create_dir_non_empty_proceed_keeps_contents() {
        let tmp = tempfile::tempdir().unwrap();
        let dummy = tmp.path().join("dummy.txt");
        std::fs::write(&dummy, "x").unwrap();
        checked_create_dir(tmp.path(), NonEmptyDirAction::Proceed, false).unwrap();
        assert!(dummy.exists());
    }

    #[test]
    fn create_dir_non_empty_clear_removes_contents() {
        let tmp = tempfile::tempdir().unwrap();
        let dummy = tmp.path().join("dummy.txt");
        std::fs::write(&dummy, "x").unwrap();
        checked_create_dir(tmp.path(), NonEmptyDirAction::ClearDir, false).unwrap();
        assert!(tmp.path().is_dir());
        assert!(!dummy.exists());
    }

    #[test]
    fn create_dir_non_empty_clear_dry_run_keeps_contents() {
        let tmp = tempfile::tempdir().unwrap();
        let dummy = tmp.path().join("dummy.txt");
        std::fs::write(&dummy, "x").unwrap();
        checked_create_dir(tmp.path(), NonEmptyDirAction::ClearDir, true).unwrap();
        assert!(dummy.exists());
    }
}
