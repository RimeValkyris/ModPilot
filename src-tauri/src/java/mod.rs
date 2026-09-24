mod detect;
mod requirement;

pub use detect::{detect_java_installations, detect_java_installations_under};
pub(crate) use detect::is_oracle_path_redirector;
pub use requirement::{parse_java_major, required_java_major};
