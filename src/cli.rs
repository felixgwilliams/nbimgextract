use std::path::PathBuf;

use clap::{
    builder::{styling::AnsiColor, Styles},
    command, Parser,
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
}
