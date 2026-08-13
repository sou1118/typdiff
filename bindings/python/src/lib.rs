use std::path::PathBuf;

use pyo3::exceptions::{PyOSError, PyValueError};
use pyo3::prelude::*;

fn diff_sources(old: &str, new: &str) -> Vec<u8> {
    let filter = |b: &typdiff::Block| !matches!(b, typdiff::Block::Parbreak);
    let old_blocks: Vec<_> = typdiff::parse::parse(old)
        .into_iter()
        .filter(filter)
        .collect();
    let new_blocks: Vec<_> = typdiff::parse::parse(new)
        .into_iter()
        .filter(filter)
        .collect();
    let diff_results = typdiff::diff::diff(&old_blocks, &new_blocks);
    typdiff::render::render(&diff_results).into_bytes()
}

/// Diff two Typst documents given as source bytes, returning diff markup as
/// bytes (matching what typst.compile() expects for inline source).
#[pyfunction]
fn diff(old: &[u8], new: &[u8]) -> PyResult<Vec<u8>> {
    let old = std::str::from_utf8(old).map_err(|e| PyValueError::new_err(e.to_string()))?;
    let new = std::str::from_utf8(new).map_err(|e| PyValueError::new_err(e.to_string()))?;
    Ok(diff_sources(old, new))
}

/// Diff two Typst documents given as file paths, returning diff markup as
/// bytes (matching what typst.compile() expects for inline source).
#[pyfunction]
fn diff_files(old_path: PathBuf, new_path: PathBuf) -> PyResult<Vec<u8>> {
    let old = std::fs::read_to_string(&old_path).map_err(|e| PyOSError::new_err(e.to_string()))?;
    let new = std::fs::read_to_string(&new_path).map_err(|e| PyOSError::new_err(e.to_string()))?;
    Ok(diff_sources(&old, &new))
}

#[pymodule]
fn _typdiff(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(diff, m)?)?;
    m.add_function(wrap_pyfunction!(diff_files, m)?)?;
    Ok(())
}
