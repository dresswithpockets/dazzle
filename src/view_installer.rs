use eframe::egui;
use typed_path::Utf8PlatformPathBuf;

use crate::{app::App, styles};

pub(crate) struct SimpleInstaller {
    app_dirs: App,
    tf_dir: Utf8PlatformPathBuf,
}

impl SimpleInstaller {
    pub(crate) fn new(ctx: &egui::Context, app_dirs: App, tf_dir: Utf8PlatformPathBuf) -> Self {
        styles::configure_fonts(ctx);
        styles::configure_text_styles(ctx);

        Self { app_dirs, tf_dir }
    }
}

impl eframe::App for SimpleInstaller {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // todo!()
    }
}
