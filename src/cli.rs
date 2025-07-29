use std::path::PathBuf;

use clap::{
    builder::{styling::AnsiColor, Styles},
    Args, Parser,
};

const STYLES: Styles = Styles::styled()
    .header(AnsiColor::Yellow.on_default())
    .usage(AnsiColor::Green.on_default())
    .literal(AnsiColor::Green.on_default())
    .placeholder(AnsiColor::Green.on_default());

#[derive(Parser, Debug, Clone)]
#[command(author, version, about, long_about = None, styles=STYLES)]
pub struct Cli {
    /// path to ipynb file from which to extract files
    pub file: PathBuf,
    /// output directory for images
    #[arg(long, short)]
    pub output_path: Option<PathBuf>,
    /// Prefix for cell tags to create the file name
    #[arg(long, short)]
    pub tag_prefix: Option<String>,

    /// Disable output of written files
    #[arg(long = "quiet", short = 'q', overrides_with = "_no_quiet")]
    pub quiet: bool,
    /// Output written files to terminal [default]
    #[arg(long = "print-filenames", short = 'Q')]
    pub _no_quiet: bool,

    /// Do not write any files
    #[arg(long, overrides_with = "_no_dry_run")]
    pub dry_run: bool,
    /// Write image files [default]
    #[arg(long = "write-files")]
    pub _no_dry_run: bool,
    /// Flags to control behaviour when output dir is non-empty
    #[command(flatten)]
    pub non_empty_action: NonEmptyDirActionFlags,
}

#[derive(Debug, Clone, Args)]
#[group(required = false, multiple = false)]
pub struct NonEmptyDirActionFlags {
    /// Raise an error when the target directory is not empty [default]
    #[arg(long, help_heading = "Non Empty Dir Actions")]
    error: bool,
    /// If target dir is not empty, delete it
    #[arg(long, help_heading = "Non Empty Dir Actions")]
    clear_dir: bool,
    /// Ignore non-empty target dir
    #[arg(long, help_heading = "Non Empty Dir Actions")]
    proceed: bool,
}

impl NonEmptyDirActionFlags {
    pub fn get_action(&self) -> NonEmptyDirAction {
        match self {
            NonEmptyDirActionFlags {
                error: false,
                clear_dir: true,
                proceed: false,
            } => NonEmptyDirAction::ClearDir,
            NonEmptyDirActionFlags {
                error: false,
                clear_dir: false,
                proceed: true,
            } => NonEmptyDirAction::Proceed,
            NonEmptyDirActionFlags {
                error: true,
                clear_dir: false,
                proceed: false,
            }
            | NonEmptyDirActionFlags {
                error: false,
                clear_dir: false,
                proceed: false,
            } => NonEmptyDirAction::Error,
            _ => unreachable!("Multiple flags set. Clap should prevent this"),
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NonEmptyDirAction {
    Error,
    ClearDir,
    Proceed,
}
