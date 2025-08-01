# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog],
and this project adheres to [Semantic Versioning].

## [Unreleased]

- /

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
