mod detect;
mod script;
mod extract;

pub use detect::{detect_from_dir, detect_from_zip};
pub use extract::{copy_dir_recursive, create_zip_from_dir, extract_zip_safely};
