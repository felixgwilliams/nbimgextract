# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog],
and this project adheres to [Semantic Versioning].

## [Unreleased]

- /

## [0.3.1] - 2026-08-01

- stricter clippy lint policy
- dependency updates

## [0.3.0] - 2026-07-11

- BREAKING: a label comment must now start with `label:` (after `#` and an optional Quarto-style `|`); unrelated comments such as `# xlabel: time` no longer set the image name
- BREAKING: labels are only read from the cell's leading comment block, following the Quarto convention; a `# label:` comment after the first code line is ignored
- Ignore (with a warning) labels that would escape the output directory, such as those containing `..` or path separators; files are now always written inside the output directory
- Fix duplicate-name handling so distinct images never silently overwrite each other, including collisions between de-duplication suffixes, multi-image numbering and explicit labels
- Fix labels containing dots being truncated: `# label: fig-v1.2` now yields `fig-v1.2.png` instead of `fig-v1.png`
- Support base64 image data stored as an array of lines, and tolerate surrounding whitespace in base64 data, as written by Jupyter
- Fix SVG output: the XML declaration is no longer duplicated when the SVG already has one, and SVG stored as a single string is written as text instead of failing base64 decoding
- Fix extraction of HTML-embedded images: `<img>` tags spanning multiple lines, uppercase or mixed-case tags and attributes, and data URLs with mime parameters (e.g. `data:image/png;charset=utf-8;base64,...`) all work now

## [0.2.1] - 2025-08-01

- Allow exporting images embedded as HTML such as with hvplot
- Rename output images where labels are duplicated

## [0.2.0] - 2025-07-29

- BREAKING: Raise an error if the target image directory is non empty
- Change the default output directory to be in the same folder as the notebook, not the CWD
- Print list of written files by default
- Add options for how to handle non-empty target directories. default: error, previous behaviour: proceed. New option: clear directory before write
- Use comments in cells with `label:` to get image file names

## [0.1.2] - 2025-06-12

- Allow parsing notebooks with with json data in outputs such as widgets

## [0.1.1] - 2025-06-11

- remove debug statements

## [0.1.0] - 2025-06-11

- initial release

<!-- Links -->
[keep a changelog]: https://keepachangelog.com/en/1.0.0/
[semantic versioning]: https://semver.org/spec/v2.0.0.html
