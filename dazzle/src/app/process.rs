use atomic_counter::{AtomicCounter, RelaxedCounter};
use eframe::egui::{self, Id, Modal, RichText, Sides};
use eframe::egui::{
    Align2, ProgressBar, TextFormat, WidgetText, Window,
    text::{LayoutJob, TextWrapping},
};
use std::num::NonZero;
use std::rc::Rc;
use std::sync::{Arc, mpmc, mpsc};

#[derive(Clone, Debug)]
pub(crate) struct ProcessView {
    pub(crate) steps: usize,
    pub(crate) latest_status: String,
    pub(crate) completed: Arc<RelaxedCounter>,
    pub(crate) status_receiver: Rc<mpsc::Receiver<String>>,

    last_request: Option<ProcessConfirmation>,
    confirm_request_receiver: Rc<mpsc::Receiver<ProcessConfirmation>>,
    confirm_result_sender: Rc<mpmc::Sender<usize>>,
}

impl ProcessView {
    pub(crate) fn flush_statuses(&self) -> Option<String> {
        self.status_receiver.try_iter().last()
    }

    pub(crate) fn flush_confirm_requests(&self) -> Option<ProcessConfirmation> {
        self.confirm_request_receiver.try_iter().last()
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

        if let Some(request) = self.flush_confirm_requests() {
            self.last_request = Some(request);
        }

        if let Some(request) = &self.last_request {
            let modal = Modal::new(Id::new("process confirmation request")).show(ui.ctx(), |ui| {
                ui.set_width(500.0);
                ui.add_space(16.0);
                ui.strong(request.query.clone());
                ui.add_space(16.0);
                Sides::new().show(
                    ui,
                    |_ui| {},
                    |ui| {
                        for (idx, choice) in request.choices.iter().enumerate() {
                            if ui.button(choice.to_owned()).clicked() {
                                self.confirm_result_sender.send(idx).unwrap();
                                ui.close();
                            }
                        }
                    },
                )
            });

            if modal.should_close() {
                self.last_request = None;
            }
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

#[derive(Debug, Clone)]
pub(crate) struct ProcessConfirmation {
    query: RichText,
    choices: Vec<WidgetText>,
}

#[derive(Clone)]
pub(crate) struct ProcessState {
    pub(crate) ctx: egui::Context,
    pub(crate) status_sender: mpsc::Sender<String>,
    pub(crate) confirm_request_sender: mpsc::Sender<ProcessConfirmation>,
    pub(crate) confirm_result_receiver: Arc<mpmc::Receiver<usize>>,
    pub(crate) completed: Arc<RelaxedCounter>,
}

impl ProcessState {
    fn new(ctx: &egui::Context, steps: usize) -> (Self, ProcessView) {
        let (status_sender, status_receiver) = mpsc::channel();
        let (confirm_request_sender, confirm_request_receiver) = mpsc::channel();
        let (confirm_result_sender, confirm_result_receiver) = mpmc::channel();

        let op = Self {
            ctx: ctx.clone(),
            status_sender,
            confirm_request_sender,
            confirm_result_receiver: Arc::new(confirm_result_receiver),
            completed: Arc::new(RelaxedCounter::new(0)),
        };

        let view = ProcessView {
            steps,
            latest_status: String::new(),
            completed: op.completed.clone(),
            status_receiver: Rc::new(status_receiver),
            confirm_request_receiver: Rc::new(confirm_request_receiver),
            confirm_result_sender: Rc::new(confirm_result_sender),
            last_request: None,
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

    pub(crate) fn confirm(
        &self,
        query: impl Into<RichText>,
        choices: impl IntoIterator<Item = impl Into<WidgetText>>,
    ) -> usize {
        self.confirm_request_sender
            .send(ProcessConfirmation {
                query: query.into(),
                choices: choices.into_iter().map(Into::into).collect(),
            })
            .unwrap();

        self.confirm_result_receiver.recv().unwrap()
    }
}
