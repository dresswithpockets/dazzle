use atomic_counter::{AtomicCounter, RelaxedCounter};
use eframe::egui;
use eframe::egui::{
    Align2, ProgressBar, TextFormat, WidgetText, Window,
    text::{LayoutJob, TextWrapping},
};
use std::num::NonZero;
use std::rc::Rc;
use std::sync::mpsc;
use std::sync::{
    Arc,
    mpsc::{Receiver, Sender},
};

#[derive(Clone, Debug)]
pub(crate) struct ProcessView {
    pub(crate) steps: usize,
    pub(crate) latest_status: String,
    pub(crate) completed: Arc<RelaxedCounter>,
    pub(crate) status_receiver: Rc<Receiver<String>>,
}

impl ProcessView {
    pub(crate) fn flush_statuses(&self) -> Option<String> {
        self.status_receiver.try_iter().last()
    }

    fn ui(&mut self, ui: &mut egui::Ui) {
        if let Some(status) = self.flush_statuses() {
            self.latest_status = status;
        }

        let mut job = LayoutJob::single_section(self.latest_status.clone(), TextFormat { ..Default::default() });

        job.wrap = TextWrapping {
            max_rows: 1,
            overflow_character: Some('…'),
            break_anywhere: true,
            ..Default::default()
        };

        if self.steps == 0 {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(job);
            });
        } else {
            ui.label(job);

            #[allow(clippy::cast_precision_loss)]
            let progress = f32::clamp((self.completed.get() as f32) / (self.steps as f32), 0.0, 1.0);
            ui.add(ProgressBar::new(progress).animate(true).show_percentage());
        }
    }

    pub fn show(&mut self, id: impl Into<WidgetText>, ctx: &egui::Context) {
        Window::new(id)
            .title_bar(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, (0.0, 0.0))
            .min_size([640.0, 360.0])
            .show(ctx, |ui| {
                self.ui(ui);
            });
    }
}

#[derive(Clone)]
pub(crate) struct ProcessState {
    pub(crate) ctx: egui::Context,
    pub(crate) status_sender: Sender<String>,
    pub(crate) completed: Arc<RelaxedCounter>,
}

impl ProcessState {
    fn new(ctx: &egui::Context, steps: usize) -> (Self, ProcessView) {
        let (sender, receiver) = mpsc::channel();

        let op = Self {
            ctx: ctx.clone(),
            status_sender: sender,
            completed: Arc::new(RelaxedCounter::new(0)),
        };

        let view = ProcessView {
            steps,
            latest_status: String::new(),
            completed: op.completed.clone(),
            status_receiver: Rc::new(receiver),
        };

        (op, view)
    }

    pub(crate) fn with_spinner(ctx: &egui::Context) -> (Self, ProcessView) {
        Self::new(ctx, 0)
    }

    pub(crate) fn with_progress_bar(ctx: &egui::Context, steps: NonZero<usize>) -> (Self, ProcessView) {
        Self::new(ctx, steps.into())
    }

    pub(crate) fn push_status(&self, status: impl Into<String>) {
        self.status_sender.send(status.into()).unwrap();
        self.ctx.request_repaint();
    }

    pub(crate) fn increment_progress(&self) {
        self.completed.inc();
        self.ctx.request_repaint();
    }

    pub(crate) fn add_progress(&self, amount: usize) {
        self.completed.add(amount);
        self.ctx.request_repaint();
    }
}
