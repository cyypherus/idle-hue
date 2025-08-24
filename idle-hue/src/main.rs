#![windows_subsystem = "windows"]

mod auto_update;

use arboard::Clipboard;
use auto_update::{AutoUpdater, UpdateStatus};
use color::{AlphaColor, ColorSpaceTag, Oklch, Srgb, parse_color};
use directories::ProjectDirs;
use kurbo::Point;
use parley::FontWeight;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs;
use tokio::sync::Mutex;
use ui::*;

const GRAY_0_D: Color = Color::from_rgb8(0x00, 0x00, 0x00); // #000000
const GRAY_30_D: Color = Color::from_rgb8(0x1e, 0x1e, 0x1e); // #1e1e1e
const GRAY_50_D: Color = Color::from_rgb8(0x3b, 0x3b, 0x3b); // #3b3b3b
const GRAY_70_D: Color = Color::from_rgb8(0x61, 0x61, 0x61); // #616161

const GRAY_0_L: Color = Color::from_rgb8(0xff, 0xff, 0xff); // #ffffff
const GRAY_30_L: Color = Color::from_rgb8(0xea, 0xe4, 0xe6); // #eae4e6
const GRAY_50_L: Color = Color::from_rgb8(0xd9, 0xd2, 0xd4); // #d9d2d4
const GRAY_70_L: Color = Color::from_rgb8(0xb6, 0xb6, 0xb8); // #bdb6b8

enum Theme {
    Gray0,
    Gray30,
    Gray50,
    Gray70,
}

struct State {
    color_code: String,
    error: Arc<Mutex<Option<String>>>,
    copy_button: ButtonState,
    paste_button: ButtonState,
    light_dark_mode_button: ButtonState,
    color: CurrentColor,
    oklch_mode: bool,
    mode_picker: ToggleState,
    component_fields: [TextState; 3],
    dark_mode: bool,
    update_status: Arc<Mutex<auto_update::UpdateStatus>>,
    saved_state: Arc<Mutex<Option<SavedState>>>,
    update_button: ButtonState,
}

#[derive(Clone, Debug)]
enum CurrentColor {
    Srgb(AlphaColor<Srgb>),
    Oklch(AlphaColor<Oklch>),
}

impl CurrentColor {
    fn components(&self) -> [f32; 4] {
        match self {
            CurrentColor::Srgb(color) => color.components,
            CurrentColor::Oklch(color) => color.components,
        }
    }
    fn components_mut(&mut self) -> &mut [f32; 4] {
        match self {
            CurrentColor::Srgb(color) => &mut color.components,
            CurrentColor::Oklch(color) => &mut color.components,
        }
    }
    fn display(&self) -> Color {
        match self {
            CurrentColor::Srgb(color) => color.convert::<Srgb>(),
            CurrentColor::Oklch(color) => color.convert::<Srgb>(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct SavedState {
    text: String,
    dark_mode: Option<bool>,
}

impl State {
    fn theme(&self, palette: Theme) -> Color {
        self.theme_color_invert(palette, false)
    }

    fn theme_inverted(&self, palette: Theme) -> Color {
        self.theme_color_invert(palette, true)
    }

    fn theme_color_invert(&self, palette: Theme, invert: bool) -> Color {
        let dark_mode = if invert {
            !self.dark_mode
        } else {
            self.dark_mode
        };
        if dark_mode {
            match palette {
                Theme::Gray0 => GRAY_0_D,
                Theme::Gray30 => GRAY_30_D,
                Theme::Gray50 => GRAY_50_D,
                Theme::Gray70 => GRAY_70_D,
            }
        } else {
            match palette {
                Theme::Gray0 => GRAY_0_L,
                Theme::Gray30 => GRAY_30_L,
                Theme::Gray50 => GRAY_50_L,
                Theme::Gray70 => GRAY_70_L,
            }
        }
    }
    fn update_text(&mut self) {
        match self.color {
            CurrentColor::Srgb(color) => {
                self.color_code = format!(
                    "#{:02x}{:02x}{:02x}",
                    (color.components[0] * 255.0) as u8,
                    (color.components[1] * 255.0) as u8,
                    (color.components[2] * 255.0) as u8,
                );
            }

            CurrentColor::Oklch(color) => {
                let color_text = format!(
                    "oklch({:.2} {:.2} {:.0})",
                    color.components[0], color.components[1], color.components[2],
                );
                self.color_code = color_text;
            }
        }
    }

    fn rgb_to_oklch(&mut self) {
        if let CurrentColor::Srgb(color) = self.color {
            self.color = CurrentColor::Oklch(color.convert::<Oklch>());
        }
    }

    fn oklch_to_rgb(&mut self) {
        if let CurrentColor::Oklch(color) = self.color {
            let mut converted = color.convert::<Srgb>();
            converted.components[0] = converted.components[0].clamp(0.0, 1.0);
            converted.components[1] = converted.components[1].clamp(0.0, 1.0);
            converted.components[2] = converted.components[2].clamp(0.0, 1.0);
            self.color = CurrentColor::Srgb(converted);
        }
    }

    fn update_component(color: &mut CurrentColor, component_index: usize, drag: DragState) {
        match drag {
            DragState::Began { .. } => (),
            DragState::Updated {
                delta: Point { y, .. },
                ..
            }
            | DragState::Completed {
                delta: Point { y, .. },
                ..
            } => {
                let y = y as f32;
                match color {
                    CurrentColor::Oklch(color) => match component_index {
                        0 => {
                            color.components[0] = (color.components[0] - y * 0.001).clamp(0.0, 1.0)
                        }
                        1 => {
                            color.components[1] = (color.components[1] - y * 0.0005).clamp(0.0, 0.5)
                        }
                        2 => {
                            color.components[2] -= y * 0.5;
                            if color.components[2] < 0.0 {
                                color.components[2] += 360.0
                            }
                            if color.components[2] >= 360.0 {
                                color.components[2] -= 360.0
                            }
                        }
                        _ => (),
                    },
                    CurrentColor::Srgb(color) => {
                        color.components[component_index] =
                            (color.components[component_index] - y * 0.001).clamp(0.0, 1.0);
                    }
                }
            }
        }
    }

    fn clamp_color_components(&mut self) {
        if self.oklch_mode {
            self.color.components_mut()[0] = self.color.components()[0].clamp(0.0, 1.0);
            self.color.components_mut()[1] = self.color.components()[1].clamp(0.0, 0.5);
            self.color.components_mut()[2] = self.color.components()[2].clamp(0.0, 360.0);
        } else {
            for i in 0..3 {
                self.color.components_mut()[i] = self.color.components()[i].clamp(0.0, 1.0);
            }
        }
    }

    fn sync_component_fields(&mut self) {
        if self.oklch_mode {
            self.component_fields[0].text = format!("{:.2}", self.color.components()[0])
                .trim_start_matches('0')
                .to_string();
            self.component_fields[1].text = format!("{:.2}", self.color.components()[1])
                .trim_start_matches('0')
                .to_string();
            self.component_fields[2].text = format!("{:.0}", self.color.components()[2]);
        } else {
            for i in 0..3 {
                self.component_fields[i].text =
                    format!("{}", (self.color.components()[i] * 255.) as u8);
            }
        }
    }

    fn copy(&self) {
        if let Ok(mut clipboard) = Clipboard::new() {
            if let Err(e) = clipboard.set_text(self.color_code.clone()) {
                eprintln!("Failed to copy to clipboard: {e}");
            }
        }
    }

    fn paste(&mut self, app: &mut AppState<State>) {
        fn delay_clear_error(error: Arc<Mutex<Option<String>>>, app: &mut AppState<State>) {
            let redraw = app.redraw_trigger();
            app.spawn(async move {
                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                let mut current_error = error.lock().await;
                *current_error = None;
                redraw.trigger().await;
            });
        }
        if let Ok(mut clipboard) = Clipboard::new() {
            if let Ok(text) = clipboard.get_text() {
                let trimmed = text.trim();
                match self.parse_color(trimmed.to_string()) {
                    Ok(_) => (),
                    Err(e) => {
                        self.error = Arc::new(Mutex::new(Some(e)));
                        delay_clear_error(self.error.clone(), app);
                    }
                }
            }
        }
        self.update_text();
        self.sync_component_fields();
        self.save_state(app);
    }

    fn parse_color(&mut self, text: String) -> Result<(), String> {
        let Some(parsed) = parse_color(&text)
            .ok()
            .or(parse_color(format!("#{text}").as_str()).ok())
        else {
            return Err("Color parsing failed".to_string());
        };
        match parsed.cs {
            ColorSpaceTag::Srgb => {
                self.color = CurrentColor::Srgb(parsed.to_alpha_color::<Srgb>());
                self.oklch_mode = false;
                self.mode_picker.on = self.oklch_mode;
                Ok(())
            }
            ColorSpaceTag::Oklch => {
                self.color = CurrentColor::Oklch(parsed.to_alpha_color::<Oklch>());
                self.oklch_mode = true;
                self.mode_picker.on = self.oklch_mode;
                Ok(())
            }
            _ => Err("Unsupported color space".to_string()),
        }
    }

    fn contrast_color(&self) -> Color {
        let rl =
            self.color.display().discard_alpha().relative_luminance() * self.color.components()[3];
        if rl > 0.5 { Color::BLACK } else { Color::WHITE }
    }

    fn get_config_path() -> Option<PathBuf> {
        ProjectDirs::from("com", "cyy", "idle-hue")
            .map(|proj_dirs| proj_dirs.config_dir().join("state.json"))
    }

    async fn load_saved_state() -> Option<SavedState> {
        let config_path = Self::get_config_path()?;
        let content = fs::read_to_string(config_path).await.ok()?;
        serde_json::from_str(&content).ok()
    }

    fn save_state(&self, app: &mut AppState<State>) {
        let saved_state = SavedState {
            text: self.color_code.clone(),
            dark_mode: Some(self.dark_mode),
        };
        let redraw = app.redraw_trigger();
        app.spawn(async move {
            if let Some(config_path) = Self::get_config_path() {
                if let Some(parent) = config_path.parent() {
                    let _ = fs::create_dir_all(parent).await;
                }

                if let Ok(json) = serde_json::to_string_pretty(&saved_state) {
                    let _ = fs::write(config_path, json).await;
                }
            }
            redraw.trigger().await;
        });
    }

    fn default() -> Self {
        let mut s = State {
            color_code: "ffffff".to_string(),
            error: Arc::new(Mutex::new(None)),
            copy_button: ButtonState::default(),
            paste_button: ButtonState::default(),
            light_dark_mode_button: ButtonState::default(),
            color: CurrentColor::Oklch(AlphaColor::<Oklch>::new([1.0, 0.0, 0.0, 1.0])),
            oklch_mode: true,
            mode_picker: ToggleState::on(),
            component_fields: [
                TextState::default(),
                TextState::default(),
                TextState::default(),
            ],
            dark_mode: true,
            update_status: Arc::new(Mutex::new(auto_update::UpdateStatus::Idle)),
            saved_state: Arc::new(Mutex::new(None)),
            update_button: ButtonState::default(),
        };
        s.sync_component_fields();
        s.update_text();
        s
    }
}

fn main() {
    let state = State::default();
    env_logger::init();

    App::builder(state, || {
        dynamic(|s: &mut State, _: &mut AppState<State>| {
            stack(vec![
                rect(id!()).fill(s.theme(Theme::Gray0)).finish(),
                column_spaced(
                    15.,
                    vec![
                        row(vec![space().height(0.), update_button()]),
                        stack(vec![
                            rect(id!())
                                .fill(s.color.display())
                                .stroke(s.theme(Theme::Gray50), 3.)
                                .corner_rounding(15.)
                                .view()
                                .finish(),
                            text(id!(), s.color_code.clone())
                                .font_size(if s.oklch_mode { 35 } else { 40 })
                                .font_weight(FontWeight::BOLD)
                                .fill(s.contrast_color())
                                .view()
                                .transition_duration(0.)
                                .finish(),
                            column(vec![
                                row_spaced(
                                    10.,
                                    vec![
                                        app_button(
                                            id!(),
                                            binding!(State, light_dark_mode_button),
                                            if s.dark_mode {
                                                include_str!("assets/sun.svg")
                                            } else {
                                                include_str!("assets/moon.svg")
                                            },
                                            |state, app| {
                                                state.dark_mode = !state.dark_mode;
                                                state.save_state(app);
                                            },
                                        ),
                                        space().height(0.),
                                        row(vec![
                                            text(id!(), "oklch").fill(s.contrast_color()).finish(),
                                            space().height(0.).width(10.),
                                            mode_toggle_button(),
                                        ]),
                                    ],
                                ),
                                space(),
                                row_spaced(
                                    10.,
                                    vec![
                                        app_button(
                                            id!(),
                                            binding!(State, paste_button),
                                            include_str!("assets/paste.svg"),
                                            |state, app| {
                                                state.paste(app);
                                            },
                                        ),
                                        space().height(0.),
                                        if let Some(error) = s.error.blocking_lock().clone() {
                                            text(id!(), error).fill(s.contrast_color()).finish()
                                        } else {
                                            empty()
                                        },
                                        space().height(0.),
                                        app_button(
                                            id!(),
                                            binding!(State, copy_button),
                                            include_str!("assets/copy.svg"),
                                            |state, _app| {
                                                state.copy();
                                            },
                                        ),
                                    ],
                                ),
                            ])
                            .pad(10.),
                        ]),
                        color_component_sliders(),
                    ],
                )
                .pad(20.),
            ])
        })
    })
    .on_start(|state, app| {
        let saved = state.saved_state.clone();
        let redraw = app.redraw_trigger();
        app.spawn(async move {
            if let Some(saved_state) = State::load_saved_state().await {
                *saved.lock().await = Some(saved_state);
            }
            redraw.trigger().await;
        });
    })
    .on_frame(|state, _app| {
        let saved = state.saved_state.blocking_lock().clone();
        if let Some(ref saved) = saved {
            _ = state.parse_color(saved.text.clone());
            if let Some(dark_mode) = saved.dark_mode {
                state.dark_mode = dark_mode;
            }
            state.update_text();
            state.sync_component_fields();
            *state.saved_state.blocking_lock() = None;
        }
    })
    .inner_size(450, 350)
    // .resizable(false)
    .start()
}

fn app_button<'n>(
    id: u64,
    binding: Binding<State, ButtonState>,
    icon: &'n str,
    on_click: fn(&mut State, &mut AppState<State>),
) -> Node<'n, State, AppState<State>> {
    dynamic(move |s: &mut State, _app| {
        let color = s.theme_inverted(Theme::Gray0);
        button(id!(id), binding.clone())
            .corner_rounding(10.)
            .fill(s.theme(Theme::Gray30))
            .label(move |_, button| {
                svg(id!(id), icon)
                    .fill({
                        match (button.depressed, button.hovered) {
                            (true, _) => color.map_lightness(|l| l - 0.2),
                            (false, true) => color.map_lightness(|l| l + 0.2),
                            (false, false) => color,
                        }
                    })
                    .finish()
                    .pad(6.)
            })
            .on_click(on_click)
            .finish()
            .height(30.)
            .width(30.)
    })
}

fn mode_toggle_button<'n>() -> Node<'n, State, AppState<State>> {
    dynamic(|s: &mut State, _app| {
        toggle(id!(), binding!(State, mode_picker))
            .on_fill(s.theme(Theme::Gray50))
            .off_fill(s.theme(Theme::Gray30))
            .knob_fill(s.theme_inverted(Theme::Gray0))
            .on_toggle(|s, app, on| {
                if on {
                    s.oklch_mode = true;
                    s.rgb_to_oklch();
                } else {
                    s.oklch_mode = false;
                    s.oklch_to_rgb();
                }
                s.sync_component_fields();
                s.update_text();
                s.save_state(app);
            })
            .finish()
            .height(25.)
            .width(45.)
    })
}

fn color_component_sliders<'n>() -> Node<'n, State, AppState<State>> {
    dynamic(|s: &mut State, _app| {
        let color = s.color.clone();
        let oklch_mode = s.oklch_mode;
        let contrasting_highlight = {
            let luminance = s.color.display().discard_alpha().relative_luminance();
            if (0.15..0.9).contains(&luminance) {
                s.theme_inverted(Theme::Gray0)
            } else {
                s.color.display()
            }
        };
        row_spaced(
            10.,
            (0usize..3)
                .map(|i| {
                    row(vec![
                        text_field(
                            id!(i as u64),
                            Binding::new(
                                move |s: &State| s.component_fields[i].clone(),
                                move |s, value| s.component_fields[i] = value,
                            ),
                        )
                        .fill(s.theme_inverted(Theme::Gray0))
                        .background_fill(Some(s.theme(Theme::Gray30)))
                        .cursor_fill(s.theme_inverted(Theme::Gray0))
                        .highlight_fill(contrasting_highlight.with_alpha(0.25))
                        .background_stroke(s.theme(Theme::Gray70), contrasting_highlight, 2.)
                        .on_edit(move |s, app, edit| match edit {
                            EditInteraction::Update(new) => {
                                if oklch_mode {
                                    if let Ok(value) = new.parse::<f32>() {
                                        s.color.components_mut()[i] = value;
                                    }
                                } else if let Ok(value) = new.parse::<u8>() {
                                    s.color.components_mut()[i] = value as f32 / 255.;
                                }
                            }
                            EditInteraction::End => {
                                s.clamp_color_components();
                                s.sync_component_fields();
                                s.update_text();
                                s.save_state(app);
                            }
                        })
                        .font_size(24)
                        .finish()
                        .pad(10.),
                        stack(vec![
                            rect(id!(i as u64))
                                .fill(Color::TRANSPARENT)
                                .view()
                                .on_drag(move |s: &mut State, app, drag| {
                                    State::update_component(&mut s.color, i, drag);
                                    s.sync_component_fields();
                                    s.update_text();
                                    match drag {
                                        DragState::Began { .. } => {
                                            app.end_editing(s);
                                        }
                                        DragState::Completed { .. } => {
                                            s.save_state(app);
                                        }
                                        _ => (),
                                    }
                                })
                                .finish(),
                            rect(id!(i as u64))
                                .fill(s.theme(Theme::Gray50))
                                .corner_rounding(5.)
                                .view()
                                .finish(),
                            column(vec![
                                rect(id!(i as u64))
                                    .fill({
                                        let mut color = color.clone();
                                        let drag = -150.;
                                        State::update_component(
                                            &mut color,
                                            i,
                                            DragState::Updated {
                                                start: Point::new(0.0, 0.0),
                                                current: Point::new(0.0, drag),
                                                start_global: Point::new(0.0, 0.0),
                                                current_global: Point::new(0.0, drag),
                                                delta: Point::new(0.0, drag),
                                                distance: drag as f32,
                                            },
                                        );
                                        color.display()
                                    })
                                    .corner_rounding(5.)
                                    .finish()
                                    .height(5.)
                                    .pad(5.),
                                space(),
                                rect(id!(i as u64))
                                    .fill({
                                        let mut color = color.clone();
                                        let drag = 150.;
                                        State::update_component(
                                            &mut color,
                                            i,
                                            DragState::Updated {
                                                start: Point::new(0.0, 0.0),
                                                current: Point::new(0.0, drag),
                                                start_global: Point::new(0.0, 0.0),
                                                current_global: Point::new(0.0, drag),
                                                delta: Point::new(0.0, drag),
                                                distance: drag as f32,
                                            },
                                        );
                                        color.display()
                                    })
                                    .corner_rounding(5.)
                                    .finish()
                                    .height(5.)
                                    .pad(5.),
                            ]),
                            svg(id!(i as u64), include_str!("assets/arrow-up-down.svg"))
                                .fill(s.theme_inverted(Theme::Gray0))
                                .finish()
                                .height(30.)
                                .width(20.),
                        ])
                        .height(55.)
                        .width(30.)
                        .pad_y(10.)
                        .pad_trailing(10.),
                    ])
                    .attach_under(
                        rect(id!(i as u64))
                            .fill(s.theme(Theme::Gray30))
                            .corner_rounding(10.)
                            .view()
                            .finish(),
                    )
                })
                .collect(),
        )
    })
}

fn update_button<'n>() -> Node<'n, State, AppState<State>> {
    dynamic(|s: &mut State, _app| {
        let current_status = s.update_status.blocking_lock().clone();

        let status_text = match current_status {
            UpdateStatus::Idle => "check for updates".to_string(),
            UpdateStatus::Checking => "checking for updates...".to_string(),
            UpdateStatus::Downloading { .. } => "downloading...".to_string(),
            UpdateStatus::Installing { .. } => "installing...".to_string(),
            UpdateStatus::Updated { .. } => "restart and install".to_string(),
            UpdateStatus::UpToDate { .. } => "you're up to date :)".to_string(),
            UpdateStatus::Error(ref message) => {
                if message.len() > 40 {
                    format!("{}...", &message[..37])
                } else {
                    message.clone()
                }
            }
        };

        button(id!(), binding!(State, update_button))
            .corner_rounding(10.)
            .surface(|_, _| space().height(30.).width(0.))
            .label(move |s, button_state| {
                text(
                    id!(),
                    if !matches!(current_status, UpdateStatus::Idle) {
                        status_text.clone()
                    } else if button_state.hovered {
                        "check for updates".to_string()
                    } else {
                        "idle-hue".to_string()
                    },
                )
                .fill(if button_state.hovered {
                    s.theme_inverted(Theme::Gray0)
                } else {
                    s.theme(Theme::Gray70)
                })
                .view()
                .transition_duration(0.)
                .finish()
            })
            .on_click(move |s: &mut State, app| {
                let current_status = s.update_status.try_lock().unwrap().clone();
                let redraw = app.redraw_trigger();
                if matches!(current_status, auto_update::UpdateStatus::Updated { .. }) {
                    app.spawn(async move {
                        if let Err(e) = AutoUpdater::restart_application().await {
                            log::error!("Failed to restart application: {e}");
                        }
                    });
                } else if !matches!(current_status, auto_update::UpdateStatus::Checking) {
                    *s.update_status.blocking_lock() = auto_update::UpdateStatus::Checking;
                    let status = s.update_status.clone();
                    app.spawn(async move {
                        let updater = auto_update::AutoUpdater::new();
                        updater
                            .check_and_install_updates_with_callback(Some(
                                move |new_status: UpdateStatus| {
                                    let status = status.clone();
                                    let redraw = redraw.clone();
                                    async move {
                                        *status.lock().await = new_status;
                                        redraw.trigger().await;
                                    }
                                },
                            ))
                            .await;
                    });
                }
            })
            .finish()
            .height(10.)
    })
}
