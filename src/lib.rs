//! fxlfit -- preflight for fixed-layout EPUB comics and picture books.
//!
//! The library half exists so the checks can be exercised from tests and
//! reused; the binary is a thin wrapper over [`epub::read`], [`checks::run`]
//! and [`report`].
//!
//! What this tool is not: an EPUB validator. epubcheck decides whether a file
//! is conformant. fxlfit assumes it is, and asks the questions that come
//! after that -- is the book actually fixed-layout, does every page declare
//! the canvas it was drawn on, is the artwork the size it claims to be, and
//! is there anything for a reader who cannot see it.

pub mod checks;
pub mod epub;
pub mod image;
pub mod model;
pub mod page;
pub mod report;
pub mod util;
pub mod xml;

pub use model::{Book, Finding, Severity};
