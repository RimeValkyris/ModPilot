mod detect;
mod install;
mod requirement;

pub use detect::{probe_java, detect_java_installations, detect_java_installations_under};
pub(crate) use detect::is_oracle_path_redirector;
pub(crate) use install::install_temurin;
pub use requirement::{parse_java_major, required_java_major};
