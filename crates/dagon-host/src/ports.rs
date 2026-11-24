//! The host's ports the CLI and the page implement: the agent the AI
//! features hand prompts to ([`Agent`]) and, with the `std` feature, the
//! builder of node images ([`BuildRunner`]).

use alloc::string::String;

/// What every AI feature sends: the same two texts whatever the agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    pub system: String,
    pub user: String,
    /// A model for agents that pick one (`dagon ai topology --model`).
    pub model: Option<String>,
}

/// The Agent port: an external agent the CLI and the page hand a prompt
/// to and take its text from. DAGON's default is the linked agent of the
/// agent definitions; a closure of the same shape is one too.
pub trait Agent {
    fn call(&mut self, prompt: &Prompt) -> Result<String, String>;
}

impl<F: FnMut(&Prompt) -> Result<String, String>> Agent for F {
    fn call(&mut self, prompt: &Prompt) -> Result<String, String> {
        self(prompt)
    }
}

#[cfg(feature = "std")]
pub use build::{BuildJob, BuildRunner};

#[cfg(feature = "std")]
mod build {
    extern crate std;

    use alloc::string::String;
    use core::sync::atomic::AtomicBool;
    use std::path::{Path, PathBuf};

    /// One node image to build.
    #[derive(Debug, Clone, Copy)]
    pub struct BuildJob<'a> {
        /// The package folder.
        pub dir: &'a Path,
        /// The binary to build.
        pub bin: &'a str,
        pub features: &'a [String],
        /// Where the build's output goes, when not the package's own.
        pub target_dir: Option<&'a Path>,
        /// The firmware build inputs: each name set to its value, or
        /// removed from the inherited environment when `None`.
        pub env: &'a [(&'a str, Option<String>)],
    }

    /// The BuildRunner port: builds a node image and returns the
    /// executable it made. DAGON's default runs cargo.
    pub trait BuildRunner {
        fn build(
            &self,
            job: &BuildJob<'_>,
            log: &mut dyn FnMut(&str),
            cancel: &AtomicBool,
        ) -> Result<PathBuf, String>;
    }
}
