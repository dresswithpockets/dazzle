//! TF2 asset preloader inspired by cueki's casual preloader.
//!
//! It supports these addons:
//!
//! - Particles
//! - Models
//! - Animations
//! - VGUI elements
//! - Lightwarps
//! - Skyboxes
//! - Warpaints
//! - Game sounds
//!
//! This preloader only supports TF2, unlike cueki's which supports TF2 and Goldrush.
//!
//! # Why?
//!
//! Cueki has done a good amount of work creating a usable preloader. My goal is to create a simpler and more
//! performant implementation.
//!
//! I'm also using this as a means to practice more idiomatic Rust.

#![feature(assert_matches)]
#![feature(duration_constructors)]
#![feature(trim_prefix_suffix)]
#![feature(file_buffered)]
#![feature(cstr_display)]
#![warn(clippy::pedantic)]
#![feature(push_mut)]
#![feature(lock_value_accessors)]
#![feature(mpmc_channel)]
#![cfg_attr(windows, windows_subsystem = "windows")]

mod app;
mod particles_manifest;
mod pcf_defaults;
mod styles;

use eframe::egui::{self, Align2, CentralPanel, Window};

use crate::app::{App, BuildError};

const APP_INSTANCE_NAME: &str = "net.dresswithpockets.dazzletf2.lock";
const APP_TLD: &str = "net";
const APP_ORG: &str = "dresswithpockets";
const APP_NAME: &str = "dazzletf2";

// TODO:
//   - discover user's tf/ directory
//   - prompt user for confirmation that the tf/ directory is correct, or to choose their own
//   - display list of addons
//   - let user choose which addons to enable/disable
//   - simple conflict resolution mode:
//      - let user reorder addons - higher priority addons get priority first
//   - advanced conflict resolution mode:
//      - for particle systems, let user choose specific groups of particle systems that conflict with one another
//          - if two addon PCFs provide overrides for the same root vanilla particle system, then they are in conflict
//      - for all other mod files, let users choose the specific files

const APP_ID: &str = "net.dresswithpockets.dazzletf2";

fn present_fatal_error_dialogue(err: BuildError) {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_app_id(APP_ID)
            .with_inner_size([640.0, 360.0])
            .with_title("Error starting up Dazzle")
            .with_resizable(false),
        ..Default::default()
    };

    let _ = eframe::run_simple_native(APP_ID, native_options.clone(), move |ctx, _frame| {
        egui_extras::install_image_loaders(ctx);
        CentralPanel::default().show(ctx, |ui| {
            Window::new("fatal error")
                .title_bar(false)
                .min_size((426.0, 240.0))
                .resizable(false)
                .collapsible(false)
                .movable(false)
                .anchor(Align2::CENTER_CENTER, (0.0, 0.0))
                .show(ui.ctx(), |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.image(egui::include_image!("static/images/warning.png"));
                        ui.heading(format!(
                            "Dazzle tried to start up but there was an error preventing it from continuing: \n\n{err}"
                        ))
                    });
                    ui.vertical_centered(|ui| {
                        if ui.button("Close").clicked() {
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    })
                });
        });
    });
}

fn main() {
    /*
       Setup/Ensure base data_local_dir

       Are we in first time setup? (config.toml must exist in data dir)
           show first time setup with tf/ dir chooser & save config.toml

       Are we configured?
           Is our version compatible with the version indicated by config.toml?
               if not, try upgrading. If upgrading fails, show first-time setup again

           Is the tf/ dir still valid?
               if not, show first time setup

           Create working directories if they dont exist (content, vpk, addons)
           Setup path strings (backup, tf/custom, tf/tf2_misc_dir.vpk)
           Show installer viewport
    */

    let app = match App::new() {
        Ok(app) => app,
        Err(err) => {
            present_fatal_error_dialogue(err);
            std::process::exit(1);
        }
    };

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_app_id(APP_ID)
            .with_inner_size([1280.0, 720.0])
            .with_drag_and_drop(true)
            .with_title("Dazzle, a TF2 mod installer")
            .with_resizable(false),
        ..Default::default()
    };

    let _ = eframe::run_native(
        APP_ID,
        native_options,
        Box::new(|cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            styles::configure_fonts(&cc.egui_ctx);
            styles::configure_text_styles(&cc.egui_ctx);
            Ok(Box::new(app))
        }),
    );

    // TODO: evaluate the contents of each extracted addon to ensure they're valid
    // TODO: evaluate if there are any conflicting particles in each addon, and warn the user
    //       for now we're just assuming there are no conflicts

    // TODO: filter out PCFs based on user selection, for now we'll just pick the first one in the list if there are conflicting PCFs
}
