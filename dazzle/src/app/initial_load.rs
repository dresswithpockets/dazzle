use std::thread::{self, JoinHandle};

use super::process::ProcessState;
use eframe::egui;
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use thiserror::Error;

use crate::app::{Paths, process::ProcessView};
use addon::{self, Addon, ExtractionError, Sources};

struct InitialLoader {
    paths: Paths,
}

#[derive(Debug, Error)]
pub(crate) enum LoadError {
    #[error(transparent)]
    Sources(#[from] addon::Error),

    #[error(transparent)]
    Extraction(#[from] ExtractionError),

    #[error(transparent)]
    Parse(#[from] addon::ParseError),
}

// A LoadOperation is an operation which processes some state and has a UI presentation to reflect the current state
// of the operation. Like
// - loading/processing setup files
// - handling new addons when theyre imported
// - installing addons to tf2

pub(crate) fn start_initial_load(
    ctx: &egui::Context,
    paths: &Paths,
) -> (ProcessView, JoinHandle<Result<Vec<Addon>, LoadError>>) {
    let loader = InitialLoader { paths: paths.clone() };

    let (load_state, load_view) =
        ProcessState::with_progress_bar(ctx, InitialLoader::operation_steps().try_into().unwrap());

    let handle = thread::spawn(move || -> Result<Vec<Addon>, LoadError> { loader.run(&load_state) });

    (load_view, handle)
}

impl InitialLoader {
    fn operation_steps() -> usize {
        90
    }

    fn run(&self, load_operation: &ProcessState) -> Result<Vec<Addon>, LoadError> {
        load_operation.push_status("Loading addons...");
        let sources = Sources::read_dir(&self.paths.addons)?;
        load_operation.add_progress(30);

        if !sources.failures.is_empty() {
            // TODO: we should present information about addons that failed to load to the user
            eprintln!("There were some errors reading some or all addon sources:");
            for (path, error) in sources.failures {
                eprintln!("  {path}: {error}");
            }
        }

        let extracted_addons: Result<Vec<_>, _> = sources
            .sources
            .into_par_iter()
            .map(|source| {
                load_operation.push_status(format!("Extracting addon {}", source.name().unwrap_or_default()));
                source.extract_as_subfolder_in(&self.paths.extracted_content)
            })
            .collect();
        load_operation.add_progress(30);

        let mut addons = Vec::new();
        for addon in extracted_addons? {
            load_operation.push_status(format!("Parsing contents of {}", addon.name().unwrap_or_default()));
            addons.push(addon.parse_content()?);
        }
        load_operation.add_progress(30);
        load_operation.push_status("Done!");

        Ok(addons)
    }
}
