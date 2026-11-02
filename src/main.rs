// release 빌드에서는 Windows 콘솔 창을 띄우지 않는다.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use chrono::{Local, NaiveDate, NaiveDateTime, NaiveTime, TimeDelta};
use eframe::egui;
use enigo::{Button, Direction::Click, Enigo, Mouse, Settings};

/// 목표 시각까지 이만큼 남으면 sleep을 멈추고 바쁜 대기로 정확한 시각을 맞춘다.
const SPIN_THRESHOLD: Duration = Duration::from_millis(2);
/// 대기 중 시계 변경을 놓치지 않도록 한 번에 이보다 오래 sleep하지 않는다.
const MAX_SLEEP: Duration = Duration::from_millis(10);

fn main() -> eframe::Result {
    let enigo = Enigo::new(&Settings::default()).ok();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("시간 입력")
            .with_app_id("com.lidar.weverse_macro")
            .with_inner_size([300.0, 170.0]),
        ..Default::default()
    };

    eframe::run_native(
        "weverse-macro",
        options,
        Box::new(|cc| Ok(Box::new(MacroApp::new(cc, enigo)))),
    )
}

/// 별도 스레드에서 목표 시각을 기다렸다가 마우스 왼쪽 버튼을 클릭한다.
struct Clicker {
    targets: Sender<NaiveDateTime>,
    results: Receiver<Result<(), String>>,
}

impl Clicker {
    fn spawn(mut enigo: Enigo, ctx: egui::Context) -> Self {
        let (targets, target_rx) = mpsc::channel::<NaiveDateTime>();
        let (result_tx, results) = mpsc::channel();

        thread::spawn(move || {
            for target in target_rx {
                wait_until(target);
                let result = enigo
                    .button(Button::Left, Click)
                    .map_err(|err| err.to_string());
                if result_tx.send(result).is_err() {
                    break;
                }
                ctx.request_repaint();
            }
        });

        Self { targets, results }
    }
}

/// 시스템 시계가 target에 도달할 때까지 기다린다.
/// 시계를 짧은 간격으로 다시 읽으므로 대기 중에 시계를 맞춰도 목표 시각을 따라간다.
fn wait_until(target: NaiveDateTime) {
    loop {
        let Ok(remaining) = (target - Local::now().naive_local()).to_std() else {
            return;
        };
        if remaining.is_zero() {
            return;
        }
        if remaining > SPIN_THRESHOLD {
            thread::sleep((remaining - SPIN_THRESHOLD).min(MAX_SLEEP));
        } else {
            std::hint::spin_loop();
        }
    }
}

struct MacroApp {
    date: String,
    time: String,
    clicker: Option<Clicker>,
    timer_active: bool,
    /// 팝업 메시지. 가장 최근 메시지가 맨 위에 표시된다.
    messages: Vec<String>,
}

impl MacroApp {
    fn new(cc: &eframe::CreationContext<'_>, enigo: Option<Enigo>) -> Self {
        let clicker = enigo.map(|enigo| Clicker::spawn(enigo, cc.egui_ctx.clone()));

        let mut messages = Vec::new();
        if clicker.is_none() {
            messages.push(
                "입력 제어(Enigo) 초기화에 실패했습니다.\nWayland 환경에서는 원격 데스크톱 제어 권한이 필요할 수 있습니다."
                    .to_owned(),
            );
        }

        Self {
            date: String::new(),
            time: String::new(),
            clicker,
            timer_active: false,
            messages,
        }
    }

    fn fill_current_time(&mut self) {
        let now = Local::now();
        self.date = now.format("%Y-%m-%d").to_string();
        self.time = now.format("%H:%M:%S%.3f").to_string();
    }

    fn start_timer(&mut self) {
        let Some(clicker) = &self.clicker else {
            return;
        };
        let (Ok(date), Ok(time)) = (
            self.date.trim().parse::<NaiveDate>(),
            self.time.trim().parse::<NaiveTime>(),
        ) else {
            return;
        };

        let target = NaiveDateTime::new(date, time);
        let wait = target - Local::now().naive_local();
        if wait <= TimeDelta::zero() {
            self.messages
                .push("목표 시간은 이미 지났습니다. 다른 날짜를 설정하세요.".to_owned());
            return;
        }
        if self.timer_active {
            self.messages
                .push("이미 타이머가 설정되어 있습니다.".to_owned());
            return;
        }
        if clicker.targets.send(target).is_err() {
            self.messages
                .push("오류: Enigo 인스턴스를 사용할 수 없습니다.".to_owned());
            return;
        }

        self.timer_active = true;
        let wait_seconds = wait.num_milliseconds() as f64 / 1000.0;
        self.messages
            .push(format!("타이머 설정됨: {wait_seconds:.3}초 후 클릭"));
    }

    fn show_message(&mut self, ctx: &egui::Context) {
        let Some(message) = self.messages.last() else {
            return;
        };

        let modal = egui::Modal::new(egui::Id::unique("message")).show(ctx, |ui| {
            ui.set_max_width(240.0);
            ui.label(message);
            ui.add_space(8.0);
            ui.vertical_centered(|ui| ui.button("확인").clicked()).inner
        });

        if modal.inner || modal.should_close() {
            self.messages.pop();
        }
    }
}

impl eframe::App for MacroApp {
    fn logic(&mut self, _ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let Some(clicker) = &self.clicker else {
            return;
        };
        while let Ok(result) = clicker.results.try_recv() {
            self.timer_active = false;
            self.messages.push(match result {
                Ok(()) => "클릭 완료!".to_owned(),
                Err(err) => format!("클릭에 실패했습니다: {err}"),
            });
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 10.0;
            let row = egui::vec2(0.0, 28.0);

            ui.vertical_centered_justified(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.date)
                        .hint_text("2021-04-01")
                        .min_size(row)
                        .align(egui::Align2::LEFT_CENTER),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut self.time)
                        .hint_text("17:15:07.349")
                        .min_size(row)
                        .align(egui::Align2::LEFT_CENTER),
                );

                let set_button = ui
                    .add_enabled(
                        self.clicker.is_some(),
                        egui::Button::new("설정").min_size(row),
                    )
                    .on_disabled_hover_text(
                        "입력 제어 초기화에 실패했습니다. Wayland 권한을 확인하세요.",
                    );
                if set_button.clicked() {
                    self.start_timer();
                }

                if ui
                    .add(egui::Button::new("현재 시간 가져오기").min_size(row))
                    .clicked()
                {
                    self.fill_current_time();
                }
            });
        });

        let ctx = ui.ctx().clone();
        self.show_message(&ctx);
    }
}
