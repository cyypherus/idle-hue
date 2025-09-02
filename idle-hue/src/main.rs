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
use std::time::Duration;
use tokio::fs;
use tokio::sync::Mutex;
use tokio::time::interval;
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
    color_mode: ColorMode,
    mode_dropdown: DropdownState,
    component_fields: [TextState; 3],
    component_hover: [bool; 3],
    component_dragging: Option<usize>,
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
        match self.color_mode {
            ColorMode::Hex => {
                if let CurrentColor::Srgb(color) = self.color {
                    self.color_code = format!(
                        "#{:02x}{:02x}{:02x}",
                        (color.components[0] * 255.0) as u8,
                        (color.components[1] * 255.0) as u8,
                        (color.components[2] * 255.0) as u8,
                    );
                }
            }
            ColorMode::Rgb => {
                if let CurrentColor::Srgb(color) = self.color {
                    self.color_code = format!(
                        "rgb({}, {}, {})",
                        (color.components[0] * 255.0) as u8,
                        (color.components[1] * 255.0) as u8,
                        (color.components[2] * 255.0) as u8,
                    );
                }
            }
            ColorMode::Oklch => {
                if let CurrentColor::Oklch(color) = self.color {
                    self.color_code = format!(
                        "oklch({:.2} {:.2} {:.0})",
                        color.components[0], color.components[1], color.components[2],
                    );
                }
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
                delta: Point { x, .. },
                ..
            }
            | DragState::Completed {
                delta: Point { x, .. },
                ..
            } => {
                let x = -x as f32;
                match color {
                    CurrentColor::Oklch(color) => match component_index {
                        0 => {
                            color.components[0] = (color.components[0] - x * 0.001).clamp(0.0, 1.0)
                        }
                        1 => {
                            color.components[1] = (color.components[1] - x * 0.0005).clamp(0.0, 0.5)
                        }
                        2 => {
                            color.components[2] -= x * 0.5;
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
                            (color.components[component_index] - x * 0.001).clamp(0.0, 1.0);
                    }
                }
            }
        }
    }

    fn clamp_color_components(&mut self) {
        match self.color_mode {
            ColorMode::Hex | ColorMode::Rgb => {
                for i in 0..3 {
                    self.color.components_mut()[i] = self.color.components()[i].clamp(0.0, 1.0);
                }
            }
            ColorMode::Oklch => {
                self.color.components_mut()[0] = self.color.components()[0].clamp(0.0, 1.0);
                self.color.components_mut()[1] = self.color.components()[1].clamp(0.0, 0.5);
                self.color.components_mut()[2] = self.color.components()[2].clamp(0.0, 360.0);
            }
        }
    }

    fn sync_component_fields(&mut self) {
        match self.color_mode {
            ColorMode::Hex | ColorMode::Rgb => {
                for i in 0..3 {
                    self.component_fields[i].text =
                        format!("{}", (self.color.components()[i] * 255.) as u8);
                }
            }
            ColorMode::Oklch => {
                self.component_fields[0].text = format!("{:.2}", self.color.components()[0])
                    .trim_start_matches('0')
                    .to_string();
                self.component_fields[1].text = format!("{:.2}", self.color.components()[1])
                    .trim_start_matches('0')
                    .to_string();
                self.component_fields[2].text = format!("{:.0}", self.color.components()[2]);
            }
        }
    }

    fn copy(&self) {
        if let Ok(mut clipboard) = Clipboard::new()
            && let Err(e) = clipboard.set_text(self.color_code.clone())
        {
            eprintln!("Failed to copy to clipboard: {e}");
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
        if let Ok(mut clipboard) = Clipboard::new()
            && let Ok(text) = clipboard.get_text()
        {
            let trimmed = text.trim();
            match self.parse_color(trimmed.to_string()) {
                Ok(_) => (),
                Err(e) => {
                    self.error = Arc::new(Mutex::new(Some(e)));
                    delay_clear_error(self.error.clone(), app);
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
                if text.contains("rgb") {
                    self.color_mode = ColorMode::Rgb;
                    self.mode_dropdown.selected = 1;
                } else {
                    self.color_mode = ColorMode::Hex;
                    self.mode_dropdown.selected = 0;
                }
                Ok(())
            }
            ColorSpaceTag::Oklch => {
                self.color = CurrentColor::Oklch(parsed.to_alpha_color::<Oklch>());
                self.color_mode = ColorMode::Oklch;
                self.mode_dropdown.selected = 2;
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

    async fn update_button_clicked(
        update_status: Arc<Mutex<auto_update::UpdateStatus>>,
        redraw: RedrawTrigger,
    ) {
        let mut current_status = update_status.lock().await;
        if matches!(*current_status, auto_update::UpdateStatus::Updated { .. }) {
            Self::restart_app().await;
        } else if !matches!(*current_status, auto_update::UpdateStatus::Checking) {
            *current_status = auto_update::UpdateStatus::Checking;
            drop(current_status);
            redraw.trigger().await;
            Self::check_for_updates(update_status.clone(), redraw).await
        }
    }

    async fn check_for_updates(
        update_status: Arc<Mutex<auto_update::UpdateStatus>>,
        redraw: RedrawTrigger,
    ) {
        let updater = auto_update::AutoUpdater::new();
        updater
            .check_and_install_updates_with_callback(Some(move |new_status: UpdateStatus| {
                let status = update_status.clone();
                let redraw = redraw.clone();
                async move {
                    *status.lock().await = new_status;
                    redraw.trigger().await;
                }
            }))
            .await;
    }

    async fn restart_app() {
        if let Err(e) = AutoUpdater::restart_application().await {
            log::error!("Failed to restart application: {e}");
        }
    }

    fn on_start(&mut self, app: &AppState<Self>) {
        let saved = self.saved_state.clone();
        let redraw = app.redraw_trigger();
        app.spawn(async move {
            if let Some(saved_state) = State::load_saved_state().await {
                *saved.lock().await = Some(saved_state);
            }
            redraw.trigger().await;
        });

        let update_status = self.update_status.clone();
        let redraw = app.redraw_trigger();
        app.spawn(async move {
            let mut interval = interval(Duration::from_secs(60 * 60 * 4)); // Every 4 hours
            loop {
                interval.tick().await;
                Self::update_button_clicked(update_status.clone(), redraw.clone()).await;
            }
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
            color_mode: ColorMode::Hex,
            mode_dropdown: DropdownState::default(),
            component_fields: [
                TextState::default(),
                TextState::default(),
                TextState::default(),
            ],
            component_hover: [false; 3],
            component_dragging: None,
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
                    10.,
                    vec![
                        row(vec![space().height(0.), update_button()]),
                        stack(vec![
                            rect(id!())
                                .fill(s.color.display())
                                .stroke(s.theme(Theme::Gray50), 1.)
                                .corner_rounding(10.)
                                .view()
                                .finish(),
                            text(id!(), s.color_code.clone())
                                .font_size(match s.color_mode {
                                    ColorMode::Hex | ColorMode::Rgb => 30,
                                    ColorMode::Oklch => 25,
                                })
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
                                            6.,
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
                                        mode_toggle_button(),
                                    ],
                                )
                                .align_contents(Align::Top),
                                space(),
                                row_spaced(
                                    10.,
                                    vec![
                                        app_button(
                                            id!(),
                                            binding!(State, paste_button),
                                            6.,
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
                                            8.,
                                            include_str!("assets/copy.svg"),
                                            |state, _app| {
                                                state.copy();
                                            },
                                        ),
                                    ],
                                ),
                            ])
                            .pad(5.),
                        ]),
                        color_component_sliders(),
                    ],
                )
                .pad(10.)
                .pad_top(5.),
            ])
        })
    })
    .on_start(|state, app| {
        state.on_start(app);
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
    .title("idle-hue")
    .inner_size(350, 250)
    .icon(include_bytes!("assets/icon32.png"))
    .start()
}

fn app_button<'n>(
    id: u64,
    binding: Binding<State, ButtonState>,
    icon_padding: f32,
    icon: &'n str,
    on_click: fn(&mut State, &mut AppState<State>),
) -> Node<'n, State, AppState<State>> {
    dynamic(move |s: &mut State, _app| {
        let color = s.theme_inverted(Theme::Gray0);
        button(id!(id), binding.clone())
            .corner_rounding(7.)
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
                    .pad(icon_padding)
            })
            .on_click(on_click)
            .finish()
            .height(30.)
            .width(30.)
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ColorMode {
    Hex,
    Rgb,
    Oklch,
}

impl ColorMode {
    fn to_string(self) -> &'static str {
        match self {
            ColorMode::Hex => "hex",
            ColorMode::Rgb => "rgb",
            ColorMode::Oklch => "oklch",
        }
    }
    fn cases() -> Vec<ColorMode> {
        vec![ColorMode::Hex, ColorMode::Rgb, ColorMode::Oklch]
    }
}

fn mode_toggle_button<'n>() -> Node<'n, State, AppState<State>> {
    dynamic(|s: &mut State, _app| {
        dropdown(
            id!(),
            binding!(State, mode_dropdown),
            ColorMode::cases()
                .iter()
                .enumerate()
                .map(|(index, mode)| text(id!(index as u64), mode.to_string()))
                .collect(),
        )
        .corner_rounding(7.)
        .fill(s.theme(Theme::Gray30))
        .text_fill(s.theme_inverted(Theme::Gray0))
        .highlight_fill(s.theme(Theme::Gray70))
        .on_select(|s, app, selection| {
            let selection = ColorMode::cases().remove(selection);
            match &selection {
                ColorMode::Hex | ColorMode::Rgb => {
                    s.oklch_to_rgb();
                }
                ColorMode::Oklch => {
                    s.rgb_to_oklch();
                }
            }
            s.color_mode = selection;
            s.sync_component_fields();
            s.update_text();
            s.save_state(app);
        })
        .finish()
        .height(20.)
        .width(63.)
    })
}

fn color_component_sliders<'n>() -> Node<'n, State, AppState<State>> {
    dynamic(|s: &mut State, _app| {
        let color = s.color.clone();
        let contrasting_highlight = {
            let luminance = s.color.display().discard_alpha().relative_luminance();
            let range = if s.dark_mode { 0.1..1. } else { 0.0..0.9 };
            if !range.contains(&luminance) {
                s.theme_inverted(Theme::Gray0)
            } else {
                s.color.display()
            }
        }
        .with_alpha(0.5);
        row_spaced(
            10.,
            (0usize..3)
                .map(|i| {
                    column_spaced(
                        5.,
                        vec![
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
                            .highlight_fill(contrasting_highlight.with_alpha(0.5))
                            .background_stroke(s.theme(Theme::Gray70), contrasting_highlight, 1.)
                            .background_padding(5.)
                            .on_edit(move |s, app, edit| match edit {
                                EditInteraction::Update(new) => {
                                    match s.color_mode {
                                        ColorMode::Hex | ColorMode::Rgb => {
                                            if let Ok(value) = new.parse::<u8>() {
                                                s.color.components_mut()[i] = value as f32 / 255.;
                                            }
                                        }
                                        ColorMode::Oklch => {
                                            if let Ok(value) = new.parse::<f32>() {
                                                s.color.components_mut()[i] = value;
                                            }
                                        }
                                    }
                                    s.update_text();
                                }
                                EditInteraction::End => {
                                    s.clamp_color_components();
                                    s.sync_component_fields();
                                    s.update_text();
                                    s.save_state(app);
                                }
                            })
                            .enter_end_editing()
                            .esc_end_editing()
                            .font_size(18)
                            .finish(),
                            row(vec![
                                rect(id!(i as u64))
                                    .fill({
                                        let mut color = color.clone();
                                        let drag = -150.;
                                        State::update_component(
                                            &mut color,
                                            i,
                                            DragState::Updated {
                                                start: Point::new(0.0, 0.0),
                                                current: Point::new(drag, 0.),
                                                start_global: Point::new(0.0, 0.0),
                                                current_global: Point::new(drag, 0.),
                                                delta: Point::new(drag, 0.),
                                                distance: drag as f32,
                                            },
                                        );
                                        color.display()
                                    })
                                    .corner_rounding(5.)
                                    .finish()
                                    .width(5.)
                                    .height(20.)
                                    .pad(5.),
                                svg(id!(i as u64), include_str!("assets/arrow-left.svg"))
                                    .fill(s.theme_inverted(Theme::Gray0))
                                    .finish()
                                    .height(30.)
                                    .width(8.),
                                space().height(0.).width(20.),
                                svg(id!(i as u64), include_str!("assets/arrow-right.svg"))
                                    .fill(s.theme_inverted(Theme::Gray0))
                                    .finish()
                                    .height(30.)
                                    .width(8.),
                                rect(id!(i as u64))
                                    .fill({
                                        let mut color = color.clone();
                                        let drag = 150.;
                                        State::update_component(
                                            &mut color,
                                            i,
                                            DragState::Updated {
                                                start: Point::new(0.0, 0.0),
                                                current: Point::new(drag, 0.),
                                                start_global: Point::new(0.0, 0.0),
                                                current_global: Point::new(drag, 0.),
                                                delta: Point::new(drag, 0.),
                                                distance: drag as f32,
                                            },
                                        );
                                        color.display()
                                    })
                                    .corner_rounding(5.)
                                    .finish()
                                    .width(5.)
                                    .height(20.)
                                    .pad(5.),
                            ])
                            .attach_under(
                                rect(id!(i as u64))
                                    .fill(
                                        if (s.component_hover[i] && s.component_dragging.is_none())
                                            || s.component_dragging == Some(i)
                                        {
                                            s.theme(Theme::Gray50)
                                        } else {
                                            s.theme(Theme::Gray30)
                                        },
                                    )
                                    .stroke(s.theme(Theme::Gray70), 1.)
                                    .corner_rounding(5.)
                                    .view()
                                    .finish(),
                            )
                            .attach_over(
                                rect(id!(i as u64))
                                    .fill(Color::TRANSPARENT)
                                    .view()
                                    .on_hover(move |s: &mut State, _app, hover| {
                                        s.component_hover[i] = hover;
                                    })
                                    .on_drag(move |s: &mut State, app, drag| {
                                        State::update_component(&mut s.color, i, drag);
                                        s.sync_component_fields();
                                        s.update_text();
                                        match drag {
                                            DragState::Began { .. } => {
                                                s.component_dragging = Some(i);
                                                app.end_editing();
                                            }
                                            DragState::Completed { .. } => {
                                                s.component_dragging = None;
                                                s.save_state(app);
                                            }
                                            _ => (),
                                        }
                                    })
                                    .finish(),
                            ),
                        ],
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
                        format!("idle-hue {}", env!("CARGO_PKG_VERSION"))
                    },
                )
                .fill(if button_state.hovered {
                    s.theme_inverted(Theme::Gray0)
                } else {
                    s.theme(Theme::Gray70)
                })
                .font_size(13)
                .view()
                .transition_duration(0.)
                .finish()
            })
            .on_click(move |s: &mut State, app| {
                let update_status = s.update_status.clone();
                let redraw = app.redraw_trigger();
                app.spawn(async move { State::update_button_clicked(update_status, redraw).await });
            })
            .finish()
            .height(10.)
    })
}
