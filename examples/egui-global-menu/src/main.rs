mod global_menu;

use eframe::egui;

use std::sync::mpsc::{Receiver, channel};

use global_menu::{GlobalMenu, MenuItem};

const OPEN: i32 = 10;
const QUIT: i32 = 11;
const WORD_WRAP: i32 = 20;
const ABOUT: i32 = 30;

fn menu_items(word_wrap: bool) -> Vec<MenuItem> {
    vec![
        MenuItem::submenu(
            1,
            "_File",
            vec![
                MenuItem::action(OPEN, "_Open…").shortcut(&["Control", "O"]),
                MenuItem::Separator,
                MenuItem::action(QUIT, "_Quit").shortcut(&["Control", "Q"]),
            ],
        ),
        MenuItem::submenu(
            2,
            "_View",
            vec![MenuItem::Entry {
                id: WORD_WRAP,
                label: "_Word Wrap".into(),
                enabled: true,
                checked: Some(word_wrap),
                shortcut: Vec::new(),
                children: Vec::new(),
            }],
        ),
        MenuItem::submenu(3, "_Help", vec![MenuItem::action(ABOUT, "_About")]),
    ]
}

struct App {
    global_menu: Option<GlobalMenu>,
    /// Checked once at startup: never make D-Bus calls every frame.
    menu_in_panel: bool,
    clicks: Receiver<i32>,
    word_wrap: bool,
    status: String,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let (tx, clicks) = channel();
        let ctx = cc.egui_ctx.clone();
        let global_menu = GlobalMenu::export(menu_items(false), move |id| {
            let _ = tx.send(id);
            ctx.request_repaint(); // the click arrives on a D-Bus thread: wake egui
        })
        .inspect_err(|e| eprintln!("global menu unavailable: {e}"))
        .ok();
        let menu_in_panel = global_menu
            .as_ref()
            .is_some_and(GlobalMenu::panel_available);
        Self {
            global_menu,
            menu_in_panel,
            clicks,
            word_wrap: false,
            status: String::new(),
        }
    }

    fn handle(&mut self, ctx: &egui::Context, id: i32) {
        match id {
            OPEN => self.status = "Open clicked".into(),
            QUIT => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            WORD_WRAP => {
                self.word_wrap = !self.word_wrap;
                if let Some(menu) = &self.global_menu {
                    // Keep the checkmark in the panel in sync.
                    let _ = menu.set_items(menu_items(self.word_wrap));
                }
            }
            ABOUT => self.status = "egui global menu demo".into(),
            _ => {}
        }
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(id) = self.clicks.try_recv() {
            self.handle(ctx, id);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Fallback: the same actions inside the window when no panel shows them.
        if !self.menu_in_panel {
            let mut clicked = None;
            egui::Panel::top("menu").show(ui, |ui| {
                egui::MenuBar::new().ui(ui, |ui| {
                    ui.menu_button("File", |ui| {
                        if ui.button("Open…").clicked() {
                            clicked = Some(OPEN);
                        }
                        if ui.button("Quit").clicked() {
                            clicked = Some(QUIT);
                        }
                    });
                    ui.menu_button("View", |ui| {
                        let mut wrap = self.word_wrap;
                        if ui.checkbox(&mut wrap, "Word Wrap").clicked() {
                            clicked = Some(WORD_WRAP);
                        }
                    });
                    ui.menu_button("Help", |ui| {
                        if ui.button("About").clicked() {
                            clicked = Some(ABOUT);
                        }
                    });
                });
            });
            if let Some(id) = clicked {
                let ctx = ui.ctx().clone();
                self.handle(&ctx, id);
            }
        }

        egui::CentralPanel::default_margins().show(ui, |ui| {
            ui.label(format!("Word wrap: {}", self.word_wrap));
            ui.label(&self.status);
        });
    }
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        // Must match your .desktop file name so the panel can pair window and menu.
        viewport: egui::ViewportBuilder::default().with_app_id("egui-global-menu"),
        ..Default::default()
    };
    eframe::run_native(
        "egui-global-menu",
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}
