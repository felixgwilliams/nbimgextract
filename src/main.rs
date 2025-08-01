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
    fs::{create_dir_all, remove_dir_all, File},
    io::{BufReader, BufWriter, Write},
    path::Path,
};
static TO_TRIM: &[char] = &['-', ' ', '_'];
#[derive(Debug, Clone)]
struct ToWrite<'a> {
    image_type: ImageType,
    // image_json_data: Cow<'a, SourceValueRef>,
    // image_json_data: &'a SourceValue,
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
                name: {
                    if n_cell_images > 1 {
                        let n_img_digits = n_cell_images.checked_ilog10().unwrap_or(0) + 1;
                        format!(
                            "{}-{:0width$}",
                            image_name,
                            j + 1,
                            width = n_img_digits as usize
                        )
                    } else {
                        image_name.clone()
                    }
                },
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
