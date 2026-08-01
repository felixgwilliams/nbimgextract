#![warn(clippy::all, clippy::pedantic, clippy::nursery, clippy::cargo)]
#![allow(clippy::module_name_repetitions)]
#![warn(clippy::unwrap_used)]
#![warn(missing_docs)]
#![allow(clippy::multiple_crate_versions)] // can't do anything about these
#![cfg_attr(
    test,
    allow(
        clippy::expect_used,
        clippy::unwrap_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

/*! nbimgextract is a command-line tool for extracting images from Jupyter Notebooks.
 *
 *
 */

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
use std::{
    borrow::Cow,
    collections::HashMap,
    fs::{create_dir_all, remove_dir_all, File},
    io::{BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};
static TO_TRIM: &[char] = &['-', ' ', '_'];
#[derive(Debug, Clone)]
struct ToWrite<'a> {
    image_type: ImageType,
    image_json_data: SourceValueWrap<'a>,
    name: String,
}
#[allow(
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    reason = "The numbers involved will never be big enough to cause problems"
)]
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
        None => default_output_path(&cli.file)?,
    };
    let n_cells = nb.cells.len();
    // https://stackoverflow.com/a/69298721
    let n_digits = n_cells.checked_ilog10().unwrap_or(0) + 1;
    let mut to_write = Vec::new();
    let mut used_names: HashMap<String, usize> = HashMap::new();

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
        let file_name = output_file(&output_path, &item.name, item.image_type.get_extension())?;
        if !cli.quiet {
            make_write_message(&cli, &file_name);
        }
        if cli.dry_run {
            continue;
        }
        let image_data = match item.image_json_data.to_ref() {
            SourceValueRef::String(image_data) => Cow::Borrowed(image_data),
            SourceValueRef::StringArray(arr) => Cow::Owned(
                arr.iter()
                    .map(std::string::String::as_str)
                    .collect::<String>(),
            ),
        };
        if item.image_type == ImageType::Svg {
            let mut buf = BufWriter::new(File::create(file_name)?);
            if !has_xml_decl(&image_data) {
                buf.write_all(b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n")?;
            }
            buf.write_all(image_data.as_bytes())?;
        } else {
            let image_bytes = BASE64_STANDARD.decode(image_data.trim().as_bytes())?;
            BufWriter::new(File::create(file_name)?).write_all(&image_bytes)?;
        }
    }

    Ok(())
}
/// Build the output path for an image, refusing (defense in depth — labels
/// are already validated) any name that is not a direct child of the
/// output directory.
fn output_file(output_path: &Path, name: &str, extension: &str) -> anyhow::Result<PathBuf> {
    let file_name = output_path.join(name).with_added_extension(extension);
    if file_name.parent() != Some(output_path) {
        bail!(
            "Output file {} is not a direct child of output path {}",
            file_name.display(),
            output_path.display()
        );
    }
    Ok(file_name)
}
fn has_xml_decl(svg_data: &str) -> bool {
    svg_data
        .trim_start_matches('\u{feff}') // tolerate a UTF-8 BOM
        .trim_start()
        .strip_prefix("<?xml")
        .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_whitespace()))
}
/// Default output directory: `<file_stem>_images` next to the input file.
fn default_output_path(file: &Path) -> anyhow::Result<PathBuf> {
    let file_stem = file
        .file_stem()
        .ok_or_else(|| anyhow!("Input filename is empty"))?
        .to_str()
        .ok_or_else(|| anyhow!("Bad file name"))?; // can't see how to manipulate OsStr themselves

    // a path with a file stem always has a parent (possibly "")
    let parent = file.parent().unwrap_or_else(|| Path::new(""));
    Ok(parent.join(file_stem.to_owned() + "_images"))
}

/// Determine the final file stem for an image, numbering images within a
/// multi-image cell and de-duplicating names already used by earlier cells.
#[allow(
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    reason = "The numbers involved will never be big enough to cause problems"
)]
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
    let mut name_count = used_names.get(&name_stem).copied().unwrap_or(0) + 1;
    let final_name = if name_count <= 1 {
        name_stem.clone()
    } else {
        // skip suffixes that collide with names already handed out
        loop {
            let n_dups_digits = name_count.checked_ilog10().unwrap_or(0) + 1;
            let candidate = format!(
                "{}-{:0width$}",
                name_stem,
                name_count,
                width = n_dups_digits as usize
            );
            if !used_names.contains_key(&candidate) {
                break candidate;
            }
            name_count += 1;
        }
    };
    used_names.insert(name_stem.clone(), name_count);
    if final_name != name_stem {
        used_names.insert(final_name.clone(), 1);
    }
    final_name
}

fn make_write_message(cli: &cli::Cli, file_name: &Path) {
    if cli.dry_run {
        println!("Would write to {}", file_name.display());
    } else {
        println!("Writing to {}", file_name.display());
    }
}
static LABEL: &str = "label:";

/// The trimmed label if the line is a `# label:`/`#| label:` comment.
fn line_label(line: &str) -> Option<&str> {
    let rest = line.trim_start().strip_prefix('#')?;
    let rest = rest.strip_prefix('|').unwrap_or(rest);
    Some(rest.trim_start().strip_prefix(LABEL)?.trim())
}

/// Get a list of labels provided as comments
fn get_comment_label(source: &str) -> Vec<&str> {
    source
        .lines()
        .filter_map(line_label)
        .filter(|identifier| !identifier.is_empty())
        .collect()
}
/// A label is only usable as a file stem if it stays inside the output
/// directory: exactly one normal path component (no `..`, `/`, absolute
/// paths, or drive prefixes).
fn is_safe_name(name: &str) -> bool {
    let mut components = Path::new(name).components();
    matches!(components.next(), Some(std::path::Component::Normal(_)))
        && components.next().is_none()
}
fn get_image_candidate(cell: &CodeCell, tag_prefix: &str) -> Option<String> {
    get_image_candidate_comment(cell)
        .or_else(|| get_image_candidate_tags(cell.metadata.tags.as_deref(), tag_prefix))
        .filter(|name| {
            let ok = is_safe_name(name);
            if !ok {
                eprintln!("Warning: ignoring unsafe label {name:?}");
            }
            ok
        })
}
fn get_image_candidate_comment(cell: &CodeCell) -> Option<String> {
    cell.source.to_string_array().and_then(|sa| {
        let label = sa
            .iter()
            .flat_map(|s| s.lines())
            .take_while(|line| line.trim().starts_with('#'))
            .flat_map(get_comment_label)
            .next();
        label.map(std::string::ToString::to_string)
    })
}
fn get_image_candidate_tags(tags: Option<&[String]>, tag_prefix: &str) -> Option<String> {
    tags.as_ref().and_then(|tags| {
        let candidates: Vec<_> = tags
            .iter()
            .filter_map(|t| t.strip_prefix(tag_prefix))
            .map(|s| s.trim_start_matches(TO_TRIM))
            .filter(|s| !s.is_empty())
            .collect();
        if candidates.len() > 1 {
            eprintln!("Warning: Multiple tag candidates: {candidates:?}");
        }
        candidates.first().map(|s| (*s).to_owned())
    })
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
    const fn get_extension(self) -> &'static str {
        use ImageType::{Gif, Jpg, Png, Svg, Webp};
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
    use ImageType::{Gif, Jpg, Png, Svg, Webp};
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
    let (mime_str_params_base64, body_str) = strip.split_once(',')?;
    let mime_str_params = mime_str_params_base64.strip_suffix(";base64")?;
    let mime_str = mime_str_params
        .split_once(';')
        .map_or(mime_str_params, |x| x.0);
    let image_type = get_image_type(mime_str)?;
    Some((image_type, body_str))
}

#[derive(Debug, Clone, Copy)]
enum SourceValueRef<'a> {
    String(&'a str),
    StringArray(&'a [String]),
}

// when extracting an image from a cell, the image data can either be contained in a String,
// a StringArray or embedded in some HTML.  If it is in a String or StringArray, then we can return
// a reference, which has the lifetime of the original cell data ('a). If it's in HTML, I rely on
// the tl parser, which can return a string slice, but the slice has the lifetime of the parser and
// not the original string of HTML. Therefore, I have no choice but to clone the string. This is why
// it can either be owned or borrowed. I don't want to clone by default because mostly we don't need
// to, so I have to do this. Only base64 strings are ever extracted from HTML, so the owned variant
// holds a plain String.
#[derive(Debug, Clone)]
enum SourceValueWrap<'a> {
    Owned(String),
    Borrowed(SourceValueRef<'a>),
}
// I can't use a cow because I can't borrow SourceValue to SourceRef because the signature does not
// allow the lifetimes I need
impl<'a> SourceValueWrap<'a> {
    fn to_ref(&'a self) -> SourceValueRef<'a> {
        match self {
            Self::Borrowed(svr) => *svr,
            Self::Owned(s) => SourceValueRef::String(s),
        }
    }
}
fn get_image_data(data: &MimeBundle) -> Vec<(ImageType, SourceValueWrap<'_>)> {
    let mut out = Vec::new();
    for (mime, val) in data {
        if mime == "text/html" {
            // per nbformat, non-JSON mimes must be strings or string arrays: see "mimebundle" in
            // <https://github.com/jupyter/nbformat/blob/16b53251aabf472ad9406ddb1f78b0421c014eeb/nbformat/v4/nbformat.v4.schema.json>
            let joined_string: Cow<str> = match val {
                SourceValue::String(s) => Cow::Borrowed(s.as_str()),
                SourceValue::StringArray(sa) => match sa.as_slice() {
                    [] => continue,
                    [one] => Cow::Borrowed(one),
                    _ => Cow::Owned(sa.concat()),
                },
                SourceValue::JsonData(_) => {
                    eprintln!("Warning: skipping {mime} output with unexpected JSON data");
                    continue;
                }
            };
            #[allow(clippy::expect_used, reason = "It's not going to happen")]
            let frag2 = tl::parse(&joined_string, tl::ParserOptions::default())
                .expect("tl only fails to parse HTML larger than u32::MAX");
            let img_iter = frag2
                .nodes()
                .iter()
                .filter_map(|node| node.as_tag())
                .filter(|tag| tag.name().as_bytes().eq_ignore_ascii_case(b"img"))
                .filter_map(|tag| {
                    tag.attributes()
                        .iter()
                        .find(|(key, _)| key.eq_ignore_ascii_case("src"))
                        .and_then(|(_, value)| value)
                })
                .filter_map(|src| {
                    parse_data_url(&src)
                        .map(|(img_type, s)| (img_type, SourceValueWrap::Owned(s.to_string())))
                });

            out.extend(img_iter);
        } else if let Some(image_type) = get_image_type(mime) {
            // don't push the json data here so we don't have to process it later
            match val {
                SourceValue::String(s) => out.push((
                    image_type,
                    SourceValueWrap::Borrowed(SourceValueRef::String(s)),
                )),
                SourceValue::StringArray(sa) => out.push((
                    image_type,
                    SourceValueWrap::Borrowed(SourceValueRef::StringArray(sa)),
                )),
                SourceValue::JsonData(_) => {
                    eprintln!("Warning: skipping {mime} output with unexpected JSON data");
                }
            }
        }
    }
    out
}
fn checked_create_dir(
    path: &Path,
    exist_action: NonEmptyDirAction,
    dry_run: bool,
) -> anyhow::Result<()> {
    use NonEmptyDirAction::{Error, Proceed};
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
    #![allow(clippy::unwrap_used)]

    use super::*;
    use serde_json::{json, Value};

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
        assert!(get_comment_label("# label:").is_empty());
    }

    #[test]
    fn comment_label_no_spaces() {
        assert_eq!(get_comment_label("#label:foo"), vec!["foo"]);
    }
    #[test]
    fn comment_label_bar() {
        assert_eq!(get_comment_label("#| label: foo"), vec!["foo"]);
    }

    #[test]
    fn comment_label_none_present() {
        assert_eq!(get_comment_label("x = 1\n# a comment"), Vec::<&str>::new());
    }

    #[test]
    fn comment_label_matches_only_start_of_comment() {
        // `label:` must start the comment
        assert!(get_comment_label("# my label: x").is_empty());
    }

    #[test]
    fn comment_label_word_containing_label_is_not_a_label() {
        // the motivating bug 13 case: `label:` anchored, not substring-matched
        assert!(get_comment_label("# xlabel: time (s)").is_empty());
        assert!(get_comment_label("# mylabel: x").is_empty());
    }

    #[test]
    fn comment_label_bar_variations() {
        // the Quarto bar needs no surrounding whitespace ...
        assert_eq!(get_comment_label("#|label:foo"), vec!["foo"]);
        assert_eq!(get_comment_label("  #| label: y"), vec!["y"]);
        // ... but must immediately follow the `#`, and only once
        assert!(get_comment_label("# | label: x").is_empty());
        assert!(get_comment_label("#|| label: x").is_empty());
    }

    #[test]
    fn comment_label_extra_hash_is_not_a_label() {
        // only a single `#` (plus optional `|`) may precede `label:`
        assert!(get_comment_label("## label: x").is_empty());
    }

    #[test]
    fn comment_label_is_case_sensitive() {
        assert!(get_comment_label("# Label: x").is_empty());
        assert!(get_comment_label("# LABEL: x").is_empty());
    }

    fn tags(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn tag_candidate_strips_prefix_and_trim_chars() {
        assert_eq!(
            get_image_candidate_tags(Some(&tags(&["img-my-png"])), "img"),
            Some("my-png".to_owned())
        );
        assert_eq!(
            get_image_candidate_tags(Some(&tags(&["img_ -name"])), "img"),
            Some("name".to_owned())
        );
        assert_eq!(
            get_image_candidate_tags(Some(&tags(&["img name"])), "img"),
            Some("name".to_owned())
        );
    }

    #[test]
    fn tag_candidate_first_of_multiple_wins() {
        assert_eq!(
            get_image_candidate_tags(Some(&tags(&["img-a", "img-b"])), "img"),
            Some("a".to_owned())
        );
    }

    #[test]
    fn tag_candidate_no_match() {
        assert_eq!(
            get_image_candidate_tags(Some(&tags(&["hello", "world"])), "img"),
            None
        );
        assert_eq!(get_image_candidate_tags(None, "img"), None);
    }

    #[test]
    fn tag_candidate_tag_equal_to_prefix_gives_empty_name() {
        // degenerate edge: documents current behavior
        assert_eq!(get_image_candidate_tags(Some(&tags(&["img"])), "img"), None);
    }

    #[test]
    fn tag_candidate_empty_prefix_matches_everything() {
        assert_eq!(
            get_image_candidate_tags(Some(&tags(&["-first", "second"])), ""),
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
    fn image_candidate_blank_line_ends_leading_comment_block() {
        // strict Quarto-like reading: the label must appear in the contiguous
        // leading comment block, and a blank line ends the block
        let cell = code_cell(json!({
            "metadata": {},
            "outputs": [],
            "source": "# setup comment\n\n# label: x\nplot()"
        }));
        assert_eq!(get_image_candidate_comment(&cell), None);
        // same when the source is stored as an nbformat line array
        let cell = code_cell(json!({
            "metadata": {},
            "outputs": [],
            "source": ["# setup comment\n", "\n", "# label: x\n", "plot()"]
        }));
        assert_eq!(get_image_candidate_comment(&cell), None);
        // a leading blank line means there is no leading comment block at all
        let cell = code_cell(json!({
            "metadata": {},
            "outputs": [],
            "source": "\n# label: x\nplot()"
        }));
        assert_eq!(get_image_candidate_comment(&cell), None);
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
        assert!(matches!(
            out[0].1.to_ref(),
            SourceValueRef::String("b64data")
        ));
    }

    #[test]
    fn image_data_svg_string_array() {
        let bundle = mime_bundle(json!({"image/svg+xml": ["<svg>", "</svg>"]}));
        let out = get_image_data(&bundle);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, ImageType::Svg);
        assert!(matches!(
            out[0].1.to_ref(),
            SourceValueRef::StringArray(sa) if sa == ["<svg>", "</svg>"]
        ));
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
        assert!(matches!(&out[0].1, SourceValueWrap::Owned(s) if s == "AAAA"));
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
    fn image_data_multiple_image_mimes_in_document_order() {
        // MimeBundle is an IndexMap, so images come out in the order the
        // mime types appear in the notebook
        let bundle = mime_bundle(json!({"image/png": "a", "image/gif": "b"}));
        let types: Vec<_> = get_image_data(&bundle)
            .iter()
            .map(|(t, _)| t.get_extension())
            .collect();
        assert_eq!(types, ["png", "gif"]);
    }

    #[test]
    fn image_data_skips_json_valued_html() {
        // malformed input (nbformat requires strings here): skipped with a
        // warning rather than aborting
        let bundle = mime_bundle(json!({"text/html": 3}));
        assert!(get_image_data(&bundle).is_empty());
    }

    #[test]
    fn default_output_path_next_to_input() {
        assert_eq!(
            default_output_path(Path::new("dir/nb.ipynb")).unwrap(),
            PathBuf::from("dir/nb_images")
        );
        assert_eq!(
            default_output_path(Path::new("nb.ipynb")).unwrap(),
            PathBuf::from("nb_images")
        );
    }

    #[test]
    fn default_output_path_rejects_stemless_paths() {
        let err = default_output_path(Path::new("..")).unwrap_err();
        assert!(err.to_string().contains("Input filename is empty"));
    }

    #[cfg(unix)]
    #[test]
    fn default_output_path_rejects_non_utf8_names() {
        use std::os::unix::ffi::OsStrExt;
        let path = Path::new(std::ffi::OsStr::from_bytes(b"nb\xff.ipynb"));
        let err = default_output_path(path).unwrap_err();
        assert!(err.to_string().contains("Bad file name"));
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
        assert_eq!(assign_image_name("x", 0, 1, &mut used), "x-3");
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
    #[test]
    fn output_file_joins_name_and_extension() {
        assert_eq!(
            output_file(Path::new("out"), "pic", "png").unwrap(),
            PathBuf::from("out/pic.png")
        );
    }

    #[test]
    fn output_file_rejects_names_that_escape() {
        assert!(output_file(Path::new("out"), "../evil", "png").is_err());
        assert!(output_file(Path::new("out"), "a/b", "png").is_err());
    }
}
