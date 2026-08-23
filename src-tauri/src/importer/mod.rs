mod detect;
mod extract;

pub use detect::{detect_from_dir, detect_from_zip};
pub use extract::{copy_dir_recursive, extract_zip_safely};
