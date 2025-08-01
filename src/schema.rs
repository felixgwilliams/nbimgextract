use std::collections::HashMap;

use serde::Deserialize;
use serde_json::Value;

// The other members of the notebook are not needed, so we pretend they don't exist.
// If we wanted to write notebooks with this code, we would have to add them back.

/// The root of the JSON of a Jupyter Notebook
///
/// Generated by <https://app.quicktype.io/> from
/// <https://github.com/jupyter/nbformat/blob/16b53251aabf472ad9406ddb1f78b0421c014eeb/nbformat/v4/nbformat.v4.schema.json>
/// Jupyter Notebook v4.5 JSON schema.
#[derive(Clone, Debug, Deserialize)]
pub struct RawNotebook {
    /// Array of cells of the current notebook.
    pub cells: Vec<Cell>,
    // /// Notebook root-level metadata.
    // pub metadata: Value,
    // /// Notebook format (major number). Incremented between backwards incompatible changes to the
    // /// notebook format.
    // pub nbformat: i64,
    // /// Notebook format (minor number). Incremented for backward compatible changes to the
    // /// notebook format.
    // pub nbformat_minor: i64,
}
// By assigning unit variants to these, we can avoid parsing them. Nice!

/// String identifying the type of cell.
#[derive(Debug, Deserialize, Clone)]
#[serde(tag = "cell_type")]
pub enum Cell {
    #[serde(rename = "code")]
    Code(CodeCell),
    #[serde(rename = "markdown")]
    Markdown,
    #[serde(rename = "raw")]
    Raw,
}

impl Cell {
    pub fn get_code_cell(&self) -> Option<&CodeCell> {
        match self {
            Self::Code(cell) => Some(cell),
            _ => None,
        }
    }
}

/// Notebook code cell.
#[derive(Clone, Debug, Deserialize)]
pub struct CodeCell {
    // /// The id may or may not be present
    // pub id: Option<String>,
    /// Cell-level metadata.
    pub metadata: CellMetadata,
    /// Execution, display, or stream outputs.
    pub outputs: Vec<Output>,
    pub source: SourceValue,
}
impl CodeCell {
    pub fn get_output_data(&self) -> Vec<&MimeBundle> {
        self.outputs
            .iter()
            .filter_map(|x| x.get_display_data())
            .collect()
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct CellMetadata {
    /// Cell tags. We look for the image name here. They are actually supposed to be unique but we don't need to validate that, hence "Vec"
    pub tags: Option<Vec<String>>,
}

/// Result of executing a code cell.
/// String identifying the type of cell output.
#[derive(Debug, Deserialize, Clone)]
#[serde(tag = "output_type")]
pub enum Output {
    #[serde(rename = "display_data")]
    DisplayData(DisplayDataOut),
    #[allow(dead_code)]
    #[serde(rename = "error")]
    Error,

    #[serde(rename = "execute_result")]
    ExecuteResult(ExecuteResultOut),

    #[allow(dead_code)]
    #[serde(rename = "stream")]
    Stream,
}

impl Output {
    fn get_display_data(&self) -> Option<&MimeBundle> {
        match self {
            Self::DisplayData(out) => Some(&out.data),
            Self::ExecuteResult(out) => Some(&out.data),
            _ => None,
        }
    }
}

/// mimetype output (e.g. text/plain), represented as either an array of strings or a
/// string.
///
/// Contents of the cell, represented as an array of lines.
///
/// The stream's text output, represented as an array of strings.
#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
pub enum SourceValue {
    String(String),
    StringArray(Vec<String>),
    // If the media type string ends with json, the contents can be anything. e.g. with widgets
    // this variant has to go last, because with untagged unions, the variants are tried in order.
    // We won't enforce that the media type string ends with json
    #[allow(dead_code)]
    JsonData(Value),
}
pub type MimeBundle = HashMap<String, SourceValue>;

impl SourceValue {
    pub fn to_string_array(&self) -> Option<Vec<&str>> {
        match self {
            Self::JsonData(_) => None,
            Self::StringArray(sa) => Some(sa.iter().map(|s| s.as_str()).collect()),
            Self::String(s) => Some(s.split_inclusive('\n').collect()),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct DisplayDataOut {
    pub data: MimeBundle,
    // pub metadata: OutputMetadata,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ExecuteResultOut {
    pub data: MimeBundle,
}
