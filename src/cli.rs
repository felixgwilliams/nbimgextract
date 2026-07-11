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

#[allow(clippy::struct_excessive_bools)]
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
            Self {
                error: false,
                clear_dir: true,
                proceed: false,
            } => NonEmptyDirAction::ClearDir,
            Self {
                error: false,
                clear_dir: false,
                proceed: true,
            } => NonEmptyDirAction::Proceed,
            Self {
                error: true | false,
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn flags(error: bool, clear_dir: bool, proceed: bool) -> NonEmptyDirActionFlags {
        NonEmptyDirActionFlags {
            error,
            clear_dir,
            proceed,
        }
    }

    #[test]
    fn get_action_defaults_to_error() {
        assert_eq!(
            flags(false, false, false).get_action(),
            NonEmptyDirAction::Error
        );
    }

    #[test]
    fn get_action_single_flags() {
        assert_eq!(
            flags(true, false, false).get_action(),
            NonEmptyDirAction::Error
        );
        assert_eq!(
            flags(false, true, false).get_action(),
            NonEmptyDirAction::ClearDir
        );
        assert_eq!(
            flags(false, false, true).get_action(),
            NonEmptyDirAction::Proceed
        );
    }

    #[test]
    #[should_panic(expected = "Multiple flags set")]
    fn get_action_panics_on_multiple_flags() {
        // clap prevents this combination; constructing it directly hits the
        // unreachable arm
        flags(true, true, false).get_action();
    }

    #[test]
    fn parse_defaults() {
        let cli = Cli::try_parse_from(["nbimgextract", "f.ipynb"]).unwrap();
        assert_eq!(cli.file, PathBuf::from("f.ipynb"));
        assert_eq!(cli.output_path, None);
        assert_eq!(cli.tag_prefix, None);
        assert!(!cli.quiet);
        assert!(!cli.dry_run);
        assert_eq!(cli.non_empty_action.get_action(), NonEmptyDirAction::Error);
    }

    #[test]
    fn parse_requires_file() {
        assert!(Cli::try_parse_from(["nbimgextract"]).is_err());
    }

    #[test]
    fn parse_output_path_and_tag_prefix() {
        let cli =
            Cli::try_parse_from(["nbimgextract", "f.ipynb", "-o", "out", "-t", "fig"]).unwrap();
        assert_eq!(cli.output_path, Some(PathBuf::from("out")));
        assert_eq!(cli.tag_prefix.as_deref(), Some("fig"));
    }

    #[test]
    fn parse_dir_action_flags() {
        let cli = Cli::try_parse_from(["nbimgextract", "f.ipynb", "--clear-dir"]).unwrap();
        assert_eq!(
            cli.non_empty_action.get_action(),
            NonEmptyDirAction::ClearDir
        );
        let cli = Cli::try_parse_from(["nbimgextract", "f.ipynb", "--proceed"]).unwrap();
        assert_eq!(
            cli.non_empty_action.get_action(),
            NonEmptyDirAction::Proceed
        );
    }

    #[test]
    fn parse_dir_action_flags_are_exclusive() {
        assert!(
            Cli::try_parse_from(["nbimgextract", "f.ipynb", "--clear-dir", "--proceed"]).is_err()
        );
    }

    #[test]
    fn parse_dry_run_last_flag_wins() {
        let cli =
            Cli::try_parse_from(["nbimgextract", "f.ipynb", "--dry-run", "--write-files"]).unwrap();
        assert!(!cli.dry_run);
        let cli =
            Cli::try_parse_from(["nbimgextract", "f.ipynb", "--write-files", "--dry-run"]).unwrap();
        assert!(cli.dry_run);
    }

    #[test]
    fn parse_quiet_last_flag_wins() {
        let cli = Cli::try_parse_from(["nbimgextract", "f.ipynb", "-q"]).unwrap();
        assert!(cli.quiet);
        let cli = Cli::try_parse_from(["nbimgextract", "f.ipynb", "-q", "-Q"]).unwrap();
        assert!(!cli.quiet);
    }
}
